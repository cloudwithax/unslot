#!/bin/sh
# Bluetooth for slot, through BaseOS's bluez. Every bluetoothctl call is under a timeout: with
# bluetoothd down it waits forever.
#
#   slotbt.sh up              attach the chip, start bluetoothd and bluealsa, power on
#   slotbt.sh down            power off and stop them
#   slotbt.sh scan SECS       look for devices for SECS seconds
#   slotbt.sh devices         every device bluez knows, one a line:
#                             DEV mac|paired|connected|icon|name
#                             connected means usable: paired, and for a controller, its
#                             buttons reaching the kernel as an input device
#   slotbt.sh pair MAC        pair, trust and connect; exits 3 for a Bluetooth LE
#                             controller that linked up but never bonded
#   slotbt.sh connect MAC | disconnect MAC | forget MAC
#
# bluez keeps its pairings in /var/lib/bluetooth, which is in memory, so they are copied to
# Config/bluetooth on the card after pairing, forgetting and turning off, and back before
# bluetoothd starts.
set -u

BTCTL="${AGS_BLUETOOTHCTL:-bluetoothctl}"
SYSTEMCTL="${AGS_SYSTEMCTL:-systemctl}"
SETBT="${AGS_SETBT:-/mnt/vendor/ctrl/setBluetooth.sh}"
BLUEALSA="${AGS_BLUEALSA:-bluealsa}"
RFKILL="${AGS_RFKILL:-rfkill}"
HCIATTACH="${AGS_HCIATTACH:-rtk_hciattach}"
HCI="${AGS_HCI:-/sys/class/bluetooth/hci0}"
TIMEOUT="${AGS_TIMEOUT:-timeout}"
HCI_WAIT_S="${AGS_HCI_WAIT_S:-7}"
INPUT="${AGS_INPUT:-/sys/class/input}"
STORE="${AGS_BTSTORE:-/var/lib/bluetooth}"
SAVED="${AGS_BTSAVED:-$(cd "$(dirname "$0")/.." && pwd)/Config/bluetooth}"

ctl() {
	t="$1"
	shift
	$TIMEOUT "$t" $BTCTL "$@"
}

# Whether bluez's store at $1 has a device in it, one of its ADAPTER/MAC/info files.
has_devices() {
	ls "$1"/*/*/info >/dev/null 2>&1
}

# The saved pairings back, unless bluez already has some of its own since boot.
restore() {
	# Power lost between save's two steps leaves only the new copy.
	[ -d "$SAVED" ] || [ ! -d "$SAVED.new" ] || mv "$SAVED.new" "$SAVED"
	has_devices "$SAVED" || return 0
	has_devices "$STORE" && return 0
	mkdir -p "$STORE"
	cp -a "$SAVED"/. "$STORE"/ 2>/dev/null || return 0
	# A bluetoothd already running would not see them.
	$SYSTEMCTL stop bluetooth >/dev/null 2>&1 || true
}

# bluez's store onto the card, swapped in whole so a cut-off copy never replaces a good one.
# One with no devices only replaces saved ones after a forget, not before bluez has run.
save() {
	[ -d "$STORE" ] || return 0
	[ "${1:-}" = forgot ] || has_devices "$STORE" || ! has_devices "$SAVED" || return 0
	rm -rf "$SAVED.new"
	mkdir -p "$(dirname "$SAVED")"
	cp -a "$STORE" "$SAVED.new" 2>/dev/null || { rm -rf "$SAVED.new"; return 0; }
	rm -rf "$SAVED"
	mv "$SAVED.new" "$SAVED"
	sync
}

bt_up() {
	restore
	$RFKILL unblock bluetooth 2>/dev/null || true
	if [ ! -d "$HCI" ] && [ -f "$SETBT" ]; then
		/bin/rm -f /tmp/.init_bt
		/bin/sh "$SETBT" init >/dev/null 2>&1 || true
		w=0
		while [ ! -d "$HCI" ] && [ "$w" -lt $((HCI_WAIT_S * 4)) ]; do
			sleep 0.25 2>/dev/null || sleep 1
			w=$((w + 1))
		done
	fi
	[ -f "$SETBT" ] && /bin/sh "$SETBT" enable >/dev/null 2>&1
	$SYSTEMCTL start bluetooth >/dev/null 2>&1 || true
	# setBluetooth.sh brought hci0 up before bluetoothd was there, and the 4.9 kernel only
	# registers LE security (SMP) when bluetoothd powers the radio on. Without it a Bluetooth
	# LE controller links up but never pairs, so power it off for bluetoothd to power it on.
	ctl 3 power off >/dev/null 2>&1 || true
	n=0
	until ctl 3 power on 2>/dev/null | grep -q succeeded; do
		n=$((n + 1))
		[ "$n" -ge 5 ] && return 1
		sleep 1
	done
	ctl 3 pairable on >/dev/null 2>&1 || true
	if ! pidof "$(basename "$BLUEALSA")" >/dev/null 2>&1; then
		$BLUEALSA -p a2dp-source >/dev/null 2>&1 &
	fi
	return 0
}

bt_down() {
	ctl 3 power off >/dev/null 2>&1 || true
	killall "$(basename "$BLUEALSA")" 2>/dev/null || true
	$SYSTEMCTL stop bluetooth >/dev/null 2>&1 || true
	save
	$RFKILL block bluetooth 2>/dev/null || true
	# Blocking cuts the chip's power and its firmware with it, but hci0 stays, so up would
	# skip the firmware and every command to the chip would time out. Detach it all the way.
	killall "$(basename "$HCIATTACH")" 2>/dev/null || true
	return 0
}

# The `info MAC` commands on stdin, run in one bluetoothctl, as `DEV` lines. bluez says a
# device is connected as soon as a link is up, even one that never pairs or never sends a
# button press, so a device only counts as connected once it is paired, and a controller
# once the kernel has an input device for it (its uniq is the controller's address).
report() {
	inputs=" $(cat "$INPUT"/input*/uniq 2>/dev/null | tr 'a-f\n' 'A-F ') "
	ctl 5 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | awk -v inputs="$inputs" '
function flush() {
	if (mac == "") return
	if (name == "") name = alias
	up = conn == "yes" && paired == "yes"
	if (up && (icon == "input-gaming" || icon == "input-joystick") && index(inputs, " " mac " ") == 0) up = 0
	printf "DEV %s|%s|%s|%s|%s\n", mac, paired, up ? "yes" : "no", icon, name
	mac = ""
}
$1 == "Device" && $3 ~ /^\(/ { flush(); mac = toupper($2); name = alias = paired = conn = icon = ""; next }
{ line = $0; sub(/^[ \t]+/, "", line) }
line ~ /^Name: / { name = substr(line, 7) }
line ~ /^Alias: / { alias = substr(line, 8) }
line ~ /^Paired: / { paired = substr(line, 9) }
line ~ /^Connected: / { conn = substr(line, 12) }
line ~ /^Icon: / { icon = substr(line, 7) }
END { flush() }'
}

bt_devices() {
	ctl 5 devices 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | sed -n 's/^Device \([0-9A-Fa-f:]\{17\}\) .*/info \1/p' |
		report
}

connected() {
	echo "info $1" | report | grep -q '^DEV [^|]*|yes|yes|'
}

# A controller's input device turns up a moment after its link does.
came_up() {
	n=0
	until connected "$1"; do
		n=$((n + 1))
		[ "$n" -ge 10 ] && return 1
		sleep 0.5
	done
}

# Pairs $1 from inside one bluetoothctl, once its agent is in. bluez turns bonding off while no
# agent is registered, and a one-shot `bluetoothctl pair` asked before its agent was in, so an LE
# pad such as the Xbox Wireless Controller was encrypted for that link but never bonded.
bt_pair() {
	out="$(mktemp)"
	{
		n=0
		until grep -q 'Agent registered' "$out" || [ "$n" -ge 10 ]; do
			sleep 0.5
			n=$((n + 1))
		done
		echo "pair $1"
		n=0
		until grep -qE 'Pairing successful|Failed to pair' "$out" || [ "$n" -ge 56 ]; do
			sleep 0.5
			n=$((n + 1))
		done
	} | ctl 35 --agent NoInputNoOutput >"$out" 2>&1 || true
	rm -f "$out"
}

case "${1:-}" in
up) bt_up ;;
down) bt_down ;;
scan)
	secs="${2:-10}"
	ctl $((secs + 3)) --timeout "$secs" scan on >/dev/null 2>&1 || true
	;;
devices) bt_devices ;;
pair)
	[ -n "${2:-}" ] || exit 2
	bt_pair "$2"
	ctl 10 trust "$2" >/dev/null 2>&1 || true
	# Pairing leaves only the bare link up; connecting brings up the profile.
	ctl 15 connect "$2" >/dev/null 2>&1 || true
	came_up "$2" && { save; exit 0; }
	# A Bluetooth LE controller (HID over GATT) "pairs" without bonding when the kernel's LE
	# security is not up, and never sends a button. Forget it, or it reconnects forever.
	info="$(ctl 5 info "$2" 2>/dev/null)"
	if printf '%s\n' "$info" | grep -q 00001812- && printf '%s\n' "$info" | grep -q 'Bonded: no'; then
		ctl 10 remove "$2" >/dev/null 2>&1 || true
		exit 3
	fi
	# Headphones can pair and not connect yet.
	save
	exit 1
	;;
connect)
	[ -n "${2:-}" ] || exit 2
	ctl 15 connect "$2" >/dev/null 2>&1 || true
	came_up "$2"
	;;
disconnect)
	[ -n "${2:-}" ] || exit 2
	ctl 10 disconnect "$2" >/dev/null 2>&1 || true
	! connected "$2"
	;;
forget)
	[ -n "${2:-}" ] || exit 2
	ctl 10 remove "$2" >/dev/null 2>&1 && rc=0 || rc=$?
	save forgot
	exit "$rc"
	;;
*)
	echo "usage: slotbt.sh up | down | scan SECS | devices | pair MAC | connect MAC | disconnect MAC | forget MAC" >&2
	exit 2
	;;
esac
