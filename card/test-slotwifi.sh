#!/bin/sh
set -eu

HERE="$(cd "$(dirname "$0")" && pwd)"
SCRIPT="$HERE/System/slotwifi.sh"
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
	rm -rf "$TMP/net" "$TMP/run" "$TMP/ctrl" "$TMP"/*.log "$TMP/bin" "$TMP/card"
	mkdir -p "$TMP/net/wlan0" "$TMP/run" "$TMP/ctrl" "$TMP/bin"
	# The supplicant's socket appears as it starts, and wpa_cli answers PONG once it has.
	make_fake wpa_supplicant "$TMP/sup.log" ": > \"$TMP/ctrl/wlan0\""
	make_fake wpa_cli "$TMP/cli.log" "[ \"\${FAKE_PONG:-}\" = yes ] && echo PONG"
	make_fake udhcpc "$TMP/dhcp.log"
	make_fake ip "$TMP/ip.log"
	make_fake rfkill "$TMP/rfkill.log"
}

run() {
	PATH="$TMP/bin:$PATH" \
		AGS_NET_SYS="$TMP/net" AGS_RUN="$TMP/run" AGS_CTRL_DIR="$TMP/ctrl" \
		AGS_WPA_SUPPLICANT="$TMP/bin/wpa_supplicant" AGS_WPA_CLI="$TMP/bin/wpa_cli" \
		AGS_UDHCPC="$TMP/bin/udhcpc" AGS_IP="$TMP/bin/ip" AGS_RFKILL="$TMP/bin/rfkill" \
		AGS_WIFI_WAIT_S=1 AGS_WIFI_LINK_WAIT_S=3 \
		sh "$SCRIPT" "$@"
}

reset
run up "$TMP/card/Config/wifi.conf" && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "up exited $rc"
grep -q 'update_config=1' "$TMP/card/Config/wifi.conf" || fail "a new config cannot save networks"
grep -q "ctrl_interface=$TMP/ctrl" "$TMP/card/Config/wifi.conf" || fail "a new config has no socket"
grep -q -- "-i wlan0 -c $TMP/card/Config/wifi.conf" "$TMP/sup.log" || fail "the supplicant was not started on the card's config"
grep -q 'unblock wifi' "$TMP/rfkill.log" || fail "the radio was left blocked"
grep -q -- "-a $SCRIPT" "$TMP/cli.log" || fail "no watcher runs DHCP as the link comes up"

reset
mkdir -p "$TMP/card/Config"
printf 'ctrl_interface=%s\nupdate_config=1\nnetwork={\n\tssid="home"\n}\n' "$TMP/ctrl" > "$TMP/card/Config/wifi.conf"
export FAKE_PONG=yes
: > "$TMP/ctrl/wlan0"
run up "$TMP/card/Config/wifi.conf" && rc=0 || rc=$?
unset FAKE_PONG
[ "$rc" = 0 ] || fail "up over a running supplicant exited $rc"
[ -f "$TMP/sup.log" ] && fail "a second supplicant was started over the first"
grep -q 'ssid="home"' "$TMP/card/Config/wifi.conf" || fail "the saved networks were written over"

reset
echo 99999 > "$TMP/run/slotlink.session"
( sleep 1; rm -f "$TMP/run/slotlink.session" ) &
run up "$TMP/card/Config/wifi.conf" && rc=0 || rc=$?
wait
[ "$rc" = 0 ] || fail "up after a link session exited $rc"
[ -f "$TMP/run/slotlink.session" ] && fail "up did not wait for the link session to end"

reset
rm -rf "$TMP/net/wlan0"
run up "$TMP/card/Config/wifi.conf" && rc=0 || rc=$?
[ "$rc" = 1 ] || fail "up with no wlan0 exited $rc, want 1"

reset
run wlan0 CONNECTED
grep -q -- '-i wlan0 -b -p' "$TMP/dhcp.log" || fail "a connection asked for no lease"
echo 12345 > "$TMP/run/slotwifi-udhcpc.pid"
run wlan0 DISCONNECTED
[ -f "$TMP/run/slotwifi-udhcpc.pid" ] && fail "the lease outlived the link"
grep -q 'addr flush dev wlan0' "$TMP/ip.log" || fail "the old address stayed on wlan0"

reset
run down
grep -q 'terminate' "$TMP/cli.log" || fail "down left the supplicant running"
grep -q 'block wifi' "$TMP/rfkill.log" || fail "down left the radio on"

run nonsense && rc=0 || rc=$?
[ "$rc" = 2 ] || fail "an unknown verb exited $rc, want 2"

if [ "$FAILS" -gt 0 ]; then
	echo "$FAILS failure(s)"
	exit 1
fi
echo "slotwifi: ok"
