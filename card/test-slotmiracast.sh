#!/bin/sh
set -eu

HERE="$(cd "$(dirname "$0")" && pwd)"
SCRIPT="$HERE/System/slotmiracast.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT HUP INT TERM
FAILS=0

fail() {
	echo "FAIL: $*"
	FAILS=$((FAILS + 1))
}

make_fake() {
	name="$1"
	out="$2"
	shift 2
	{
		echo '#!/bin/sh'
		echo "printf '%s\\n' \"\$*\" >> \"$out\""
		for line in "$@"; do echo "$line"; done
		echo 'exit 0'
	} > "$TMP/bin/$name"
	chmod 755 "$TMP/bin/$name"
}

reset() {
	rm -rf "$TMP/net" "$TMP/run" "$TMP"/*.log "$TMP/bin"
	mkdir -p "$TMP/net/wlan1" "$TMP/run" "$TMP/bin"
	make_fake wpa_supplicant "$TMP/sup.log" ": > \"$TMP/run/wpa_p2p/wlan1\""
	make_fake wpa_cli "$TMP/cli.log" "[ \"\${FAKE_PONG:-}\" = yes ] && echo PONG"
	# The TV gives 192.168.49.20, through the script udhcpc is told to call.
	make_fake udhcpc "$TMP/dhcp.log" \
		'while [ $# -gt 0 ]; do [ "$1" = -s ] && s="$2"; [ "$1" = -i ] && i="$2"; shift; done' \
		'interface="$i" sh "$s" deconfig' \
		'interface="$i" ip=192.168.49.20 mask=24 sh "$s" bound'
	make_fake ip "$TMP/ip.log"
}

run() {
	PATH="$TMP/bin:$PATH" \
		AGS_NET_SYS="$TMP/net" AGS_RUN="$TMP/run" \
		AGS_WPA_SUPPLICANT="$TMP/bin/wpa_supplicant" AGS_WPA_CLI="$TMP/bin/wpa_cli" \
		AGS_UDHCPC="$TMP/bin/udhcpc" AGS_IP="$TMP/bin/ip" AGS_WIFI_WAIT_S=1 \
		sh "$SCRIPT" "$@"
}

reset
run up && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "up exited $rc"
grep -q -- "-i wlan1 -c $TMP/run/slotmiracast.conf" "$TMP/sup.log" || fail "the supplicant was not started on wlan1"
grep -q 'p2p_go_intent=0' "$TMP/run/slotmiracast.conf" || fail "the handheld may lead the group"
grep -q "ctrl_interface=$TMP/run/wpa_p2p" "$TMP/run/slotmiracast.conf" || fail "the config has no socket"
grep -q 'link set wlan1 up' "$TMP/ip.log" || fail "wlan1 was left down"

reset
export FAKE_PONG=yes
mkdir -p "$TMP/run/wpa_p2p"
: > "$TMP/run/wpa_p2p/wlan1"
run up && rc=0 || rc=$?
unset FAKE_PONG
[ "$rc" = 0 ] || fail "up over a running supplicant exited $rc"
[ -f "$TMP/sup.log" ] && fail "a second supplicant was started over the first"

reset
echo 99999 > "$TMP/run/slotlink.session"
run up && rc=0 || rc=$?
[ "$rc" = 1 ] || fail "up during a link session exited $rc, want 1"
[ -f "$TMP/sup.log" ] && fail "the supplicant was started under a link session"

reset
rm -rf "$TMP/net/wlan1"
run up && rc=0 || rc=$?
[ "$rc" = 1 ] || fail "up with no wlan1 exited $rc, want 1"

reset
got="$(run lease wlan1)" && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "lease exited $rc"
[ "$got" = 192.168.49.20 ] || fail "lease printed '$got', want 192.168.49.20"
grep -q 'addr add 192.168.49.20/24 dev wlan1' "$TMP/ip.log" || fail "the address was not put on wlan1"
grep -q 'route' "$TMP/ip.log" && fail "the lease touched the routes"

reset
make_fake udhcpc "$TMP/dhcp.log" 'exit 1'
run lease wlan1 >/dev/null && rc=0 || rc=$?
[ "$rc" = 1 ] || fail "lease with no answer exited $rc, want 1"

reset
echo 192.168.49.20 > "$TMP/run/slotmiracast.lease"
run down
grep -q 'terminate' "$TMP/cli.log" || fail "down left the supplicant running"
grep -q 'link set wlan1 down' "$TMP/ip.log" || fail "down left wlan1 up"
[ -f "$TMP/run/slotmiracast.lease" ] && fail "down left the lease"

run nonsense && rc=0 || rc=$?
[ "$rc" = 2 ] || fail "an unknown verb exited $rc, want 2"

if [ "$FAILS" -gt 0 ]; then
	echo "$FAILS failure(s)"
	exit 1
fi
echo "slotmiracast: ok"
