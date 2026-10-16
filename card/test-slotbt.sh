#!/bin/sh
set -eu

HERE="$(cd "$(dirname "$0")" && pwd)"
SCRIPT="$HERE/System/slotbt.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT HUP INT TERM
FAILS=0

fail() {
	echo "FAIL: $*"
	FAILS=$((FAILS + 1))
}

reset() {
	rm -rf "$TMP/bin" "$TMP"/*.log "$TMP/hci0"
	mkdir -p "$TMP/bin" "$TMP/hci0"
	# A bluetoothctl that knows a paired, connected pad and a headset it has only seen, and
	# colours its answers the way the real one does.
	cat > "$TMP/bin/bluetoothctl" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >> "$TMP/ctl.log"
case "\$*" in
"power on") echo "Changing power on succeeded" ;;
devices)
	printf '\033[0;94mDevice\033[0m AA:BB:CC:DD:EE:01 8BitDo Pro 2\n'
	echo "Device AA:BB:CC:DD:EE:02 WH-1000XM4"
	;;
"info AA:BB:CC:DD:EE:01")
	printf 'Device AA:BB:CC:DD:EE:01 (public)\n\tName: 8BitDo Pro 2\n\tIcon: input-gaming\n\tPaired: yes\n\tConnected: yes\n'
	;;
"info AA:BB:CC:DD:EE:02")
	printf 'Device AA:BB:CC:DD:EE:02 (public)\n\tName: WH-1000XM4\n\tIcon: audio-headset\n\tPaired: no\n\tConnected: %s\n' "\${FAKE_CONNECTED:-no}"
	;;
esac
exit 0
EOF
	chmod 755 "$TMP/bin/bluetoothctl"
	for tool in systemctl rfkill bluealsa; do
		printf '#!/bin/sh\nprintf "%%s\\n" "$*" >> "%s/%s.log"\nexit 0\n' "$TMP" "$tool" > "$TMP/bin/$tool"
		chmod 755 "$TMP/bin/$tool"
	done
	printf '#!/bin/sh\nshift\nexec "$@"\n' > "$TMP/bin/timeout"
	chmod 755 "$TMP/bin/timeout"
}

run() {
	PATH="$TMP/bin:$PATH" \
		AGS_BLUETOOTHCTL="$TMP/bin/bluetoothctl" AGS_SYSTEMCTL="$TMP/bin/systemctl" \
		AGS_BLUEALSA="$TMP/bin/bluealsa" AGS_RFKILL="$TMP/bin/rfkill" \
		AGS_TIMEOUT="$TMP/bin/timeout" AGS_HCI="$TMP/hci0" AGS_SETBT="$TMP/none.sh" \
		AGS_HCI_WAIT_S=1 \
		sh "$SCRIPT" "$@"
}

reset
run up && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "up exited $rc"
grep -q 'unblock bluetooth' "$TMP/rfkill.log" || fail "the radio was left blocked"
grep -q 'start bluetooth' "$TMP/systemctl.log" || fail "bluetoothd was not started"
grep -q 'pairable on' "$TMP/ctl.log" || fail "the radio cannot be paired with"

reset
out="$(run devices)"
echo "$out" | grep -qx 'DEV AA:BB:CC:DD:EE:01|yes|yes|input-gaming|8BitDo Pro 2' ||
	fail "the coloured, paired pad was misread: $out"
echo "$out" | grep -qx 'DEV AA:BB:CC:DD:EE:02|no|no|audio-headset|WH-1000XM4' ||
	fail "the headset was misread: $out"

reset
run pair AA:BB:CC:DD:EE:02 && rc=0 || rc=$?
[ "$rc" = 1 ] || fail "a pairing that never connected exited $rc, want 1"
grep -q -- '--agent NoInputNoOutput pair AA:BB:CC:DD:EE:02' "$TMP/ctl.log" || fail "pairing asked for a PIN"
grep -q 'trust AA:BB:CC:DD:EE:02' "$TMP/ctl.log" || fail "a paired device was not trusted, so it will not come back"
grep -q '^connect AA:BB:CC:DD:EE:02' "$TMP/ctl.log" || fail "pairing did not connect"

reset
export FAKE_CONNECTED=yes
run connect AA:BB:CC:DD:EE:02 && rc=0 || rc=$?
unset FAKE_CONNECTED
[ "$rc" = 0 ] || fail "a connection that came up exited $rc"

reset
run down
grep -q 'power off' "$TMP/ctl.log" || fail "down left the radio powered"
grep -q 'block bluetooth' "$TMP/rfkill.log" || fail "down left the radio unblocked"

run pair && rc=0 || rc=$?
[ "$rc" = 2 ] || fail "pair with no device exited $rc, want 2"

if [ "$FAILS" -gt 0 ]; then
	echo "$FAILS failure(s)"
	exit 1
fi
echo "slotbt: ok"
