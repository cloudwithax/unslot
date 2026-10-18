#!/bin/sh
# Network services for slot: BaseOS's SSH server, and Samba, Syncthing, Tailscale and WireGuard
# from System/services. Slot keeps what is wanted and calls this to start and stop things; it talks to
# tailscaled and wireguard-go through their own sockets for the rest, and sets wg0's routes itself.
#
#   slotservices.sh ssh on|off
#   slotservices.sh password            set root's password to the line on stdin
#   slotservices.sh samba on ROOT       share ROOT as "Card" to root, with the password on stdin
#   slotservices.sh samba off
#   slotservices.sh syncthing on ROOT   run Syncthing from ROOT/Config/syncthing, its web page
#                                       signed in to as root with the password on stdin, and
#                                       print its device ID
#   slotservices.sh syncthing off
#   slotservices.sh tailscale on|off    start or stop tailscaled
#   slotservices.sh tailscale cli ARGS  the tailscale command, against slot's tailscaled
#   slotservices.sh vpn up|down         start or stop wireguard-go on wg0
set -u

HERE="$(cd "$(dirname "$0")" && pwd)"
SVC="${AGS_SERVICES:-$HERE/services}"
RUN="${AGS_RUN:-/run}"
DATA="${AGS_DATA:-/data}"
SHADOW="${AGS_SHADOW:-/run/shadow}"
DROPBEAR="${AGS_DROPBEAR:-dropbear}"
MKPASSWD="${AGS_MKPASSWD:-mkpasswd}"
KILLALL="${AGS_KILLALL:-killall}"
PIDOF="${AGS_PIDOF:-pidof}"
IP="${AGS_IP:-ip}"
TUN="${AGS_TUN:-/dev/net/tun}"
WG_RUN="${AGS_WG_RUN:-/var/run/wireguard}"
WAIT_S="${AGS_SERVICES_WAIT_S:-5}"
# Samba and Syncthing move files in the background; at the lowest priority they take only the
# CPU a game leaves, rather than a core of the four a Dreamcast keeps busy.
IDLE="nice -n 19"

DROPBEAR_PID="$RUN/dropbear.pid"
SMB="$RUN/slotsamba"
TS_SOCK="$RUN/tailscale/tailscaled.sock"
TS_PID="$RUN/slot-tailscaled.pid"
ST_PID="$RUN/slot-syncthing.pid"

alive() {
	[ -f "$1" ] && kill -0 "$(cat "$1")" 2>/dev/null
}

stop_pid() {
	[ -f "$1" ] || return 0
	kill "$(cat "$1")" 2>/dev/null || true
	/bin/rm -f "$1"
}

# Waits up to WAIT_S for a path to appear.
wait_for() {
	s=0
	while [ ! -e "$1" ] && [ "$s" -lt $((WAIT_S * 4)) ]; do
		sleep 0.25 2>/dev/null || sleep 1
		s=$((s + 1))
	done
	[ -e "$1" ]
}

ssh_on() {
	alive "$DROPBEAR_PID" && return 0
	# BaseOS's own, which may not have written its pidfile yet.
	$PIDOF dropbear > /dev/null 2>&1 && return 0
	# As BaseOS starts it at boot. Its output goes to a log, so slot is not left waiting on a
	# pipe the daemon holds open.
	$DROPBEAR -r "$DATA/dropbear/dropbear_rsa_host_key" \
		-r "$DATA/dropbear/dropbear_ed25519_host_key" -B -P "$DROPBEAR_PID" \
		< /dev/null > "$RUN/slot-dropbear.log" 2>&1 || { echo "dropbear did not start" >&2; return 1; }
}

ssh_off() {
	if [ -f "$DROPBEAR_PID" ]; then
		stop_pid "$DROPBEAR_PID"
	else
		$KILLALL dropbear 2>/dev/null || true
	fi
	return 0
}

set_password() {
	pw=""
	IFS= read -r pw || [ -n "$pw" ] || { echo "no password given" >&2; return 1; }
	hash="$(printf '%s' "$pw" | $MKPASSWD -m sha512 -P 0)" || return 1
	[ -n "$hash" ] || return 1
	awk -F: -v h="$hash" 'BEGIN { OFS = ":" } $1 == "root" { $2 = h } { print }' \
		"$SHADOW" > "$SHADOW.slot" || return 1
	# Written through rather than moved over, so the file keeps its owner and mode.
	cat "$SHADOW.slot" > "$SHADOW"
	/bin/rm -f "$SHADOW.slot"
}

samba_on() {
	root="$1"
	pw=""
	IFS= read -r pw || [ -n "$pw" ] || { echo "no password given" >&2; return 1; }
	samba_off
	/bin/mkdir -p "$SMB/private" "$SMB/lock"
	cat > "$SMB/smb.conf" <<EOF
[global]
	server string = unslot
	security = user
	map to guest = never
	max protocol = SMB2
	disable netbios = yes
	smb ports = 445
	private dir = $SMB/private
	lock directory = $SMB/lock
	state directory = $SMB/lock
	cache directory = $SMB/lock
	pid directory = $SMB
	log file = $SMB/log
	max log size = 64
	load printers = no
	printcap name = /dev/null
	disable spoolss = yes

[Card]
	path = $root
	read only = no
	guest ok = no
	valid users = root
EOF
	printf '%s\n%s\n' "$pw" "$pw" |
		LD_LIBRARY_PATH="$SVC/samba/lib" "$SVC/samba/bin/smbpasswd" -c "$SMB/smb.conf" -s -a root \
			>/dev/null || return 1
	LD_LIBRARY_PATH="$SVC/samba/lib" $IDLE "$SVC/samba/bin/smbd" -s "$SMB/smb.conf" -D \
		< /dev/null > "$SMB/smbd.out" 2>&1 || { echo "smbd did not start" >&2; return 1; }
}

samba_off() {
	# Samba 3.6 names its pidfile after the config it was given.
	stop_pid "$SMB/smbd-smb.conf.pid"
	# Each connection has its own smbd, which outlives the one that listens.
	$KILLALL smbd 2>/dev/null || true
	return 0
}

syncthing_on() {
	root="$1"
	home="$root/Config/syncthing"
	pw=""
	IFS= read -r pw || [ -n "$pw" ] || { echo "no password given" >&2; return 1; }
	syncthing_off
	/bin/mkdir -p "$home"
	# Makes the keys and config the first time, and each time sets the web page's login.
	printf '%s\n' "$pw" | "$SVC/syncthing" generate --home="$home" --gui-user=root \
		--gui-password=- --no-port-probing > "$RUN/slot-syncthing.log" 2>&1 ||
		{ echo "could not set up $home" >&2; return 1; }
	# Folders added on the web page go on the card rather than in BaseOS's small root, and the
	# card's FAT has no permissions to keep in step.
	sed -i "/<folder id=\"\" label=\"\"/{s|path=\"[^\"]*\"|path=\"$root\"|;s|ignorePerms=\"false\"|ignorePerms=\"true\"|}" \
		"$home/config.xml"
	$IDLE "$SVC/syncthing" serve --home="$home" --no-browser --no-restart --no-upgrade \
		--gui-address=0.0.0.0:8384 --log-file="$RUN/slot-syncthing.log" \
		--log-max-size=1048576 --log-max-old-files=0 < /dev/null > /dev/null 2>&1 &
	echo $! > "$ST_PID"
	"$SVC/syncthing" device-id --home="$home"
}

syncthing_off() {
	[ -f "$ST_PID" ] || return 0
	pid="$(cat "$ST_PID")"
	stop_pid "$ST_PID"
	# Gone before another starts on its database, which it holds until it exits.
	s=0
	while kill -0 "$pid" 2>/dev/null && [ "$s" -lt $((WAIT_S * 4)) ]; do
		sleep 0.25 2>/dev/null || sleep 1
		s=$((s + 1))
	done
	return 0
}

tailscale_on() {
	alive "$TS_PID" && return 0
	/bin/mkdir -p "$DATA/tailscale" "$(dirname "$TS_SOCK")"
	# The kernel's TUN when there is one, so the tailnet reaches every program; otherwise
	# userspace, where only tailscale's own traffic can use it.
	tun=""
	[ -c "$TUN" ] || tun="--tun=userspace-networking"
	# shellcheck disable=SC2086
	"$SVC/tailscaled" --statedir="$DATA/tailscale" --socket="$TS_SOCK" --port=41641 \
		--no-logs-no-support $tun > "$RUN/slot-tailscaled.log" 2>&1 &
	echo $! > "$TS_PID"
	wait_for "$TS_SOCK"
}

tailscale_off() {
	stop_pid "$TS_PID"
	return 0
}

vpn_up() {
	[ -c "$TUN" ] || { echo "this kernel has no TUN device" >&2; return 1; }
	$IP link del wg0 2>/dev/null || true
	/bin/rm -f "$WG_RUN/wg0.sock"
	"$SVC/wireguard-go" wg0 >/dev/null 2>&1 || return 1
	wait_for "$WG_RUN/wg0.sock"
}

vpn_down() {
	# wireguard-go goes when its interface does.
	$IP link del wg0 2>/dev/null || true
	/bin/rm -f "$WG_RUN/wg0.sock"
	return 0
}

case "${1:-} ${2:-}" in
"ssh on") ssh_on ;;
"ssh off") ssh_off ;;
"password "*) set_password ;;
"samba on") samba_on "${3:?the folder to share}" ;;
"samba off") samba_off ;;
"syncthing on") syncthing_on "${3:?the card's root}" ;;
"syncthing off") syncthing_off ;;
"tailscale on") tailscale_on ;;
"tailscale off") tailscale_off ;;
"tailscale cli")
	shift 2
	TS_BE_CLI=1 exec "$SVC/tailscaled" --socket="$TS_SOCK" "$@"
	;;
"vpn up") vpn_up ;;
"vpn down") vpn_down ;;
*)
	echo "usage: slotservices.sh ssh on|off | password | samba on ROOT|off | syncthing on ROOT|off | tailscale on|off|cli ARGS | vpn up|down" >&2
	exit 2
	;;
esac
