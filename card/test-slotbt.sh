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
	rm -rf "$TMP/bin" "$TMP"/*.log "$TMP/hci0" "$TMP/input" "$TMP/store" "$TMP/card"
	mkdir -p "$TMP/store" "$TMP/card/Config"
	mkdir -p "$TMP/bin" "$TMP/hci0" "$TMP/input/input0" "$TMP/input/input3"
	# The pad's buttons reach the kernel, as an input device named by its address.
	echo "aa:bb:cc:dd:ee:01" > "$TMP/input/input3/uniq"
	# A bluetoothctl that knows a paired, connected pad, a headset it has only seen and an
	# LE pad whose link is up but never bonded, and colours its answers the way the real one
	# does. With no arguments it runs the commands on stdin, as one session.
	cat > "$TMP/bin/bluetoothctl" <<EOF
#!/bin/sh
one() {
printf '%s\n' "\$*" >> "$TMP/ctl.log"
case "\$*" in
"power on") echo "Changing power on succeeded" ;;
devices)
	printf '\033[0;94mDevice\033[0m AA:BB:CC:DD:EE:01 8BitDo Pro 2\n'
	echo "Device AA:BB:CC:DD:EE:02 WH-1000XM4"
	echo "Device AA:BB:CC:DD:EE:03 Xbox Wireless Controller"
	;;
"info AA:BB:CC:DD:EE:01")
	printf 'Device AA:BB:CC:DD:EE:01 (public)\n\tName: 8BitDo Pro 2\n\tIcon: input-gaming\n\tPaired: yes\n\tBonded: yes\n\tConnected: yes\n'
	;;
"info AA:BB:CC:DD:EE:02")
	printf 'Device AA:BB:CC:DD:EE:02 (public)\n\tName: WH-1000XM4\n\tIcon: audio-headset\n\tPaired: %s\n\tConnected: %s\n' "\${FAKE_PAIRED:-no}" "\${FAKE_CONNECTED:-no}"
	;;
"info AA:BB:CC:DD:EE:03")
	printf 'Device AA:BB:CC:DD:EE:03 (public)\n\tName: Xbox Wireless Controller\n\tIcon: input-gaming\n\tPaired: yes\n\tBonded: no\n\tConnected: yes\n\tUUID: Human Interface Device    (00001812-0000-1000-8000-00805f9b34fb)\n'
	;;
esac
}
if [ \$# -gt 0 ]; then
	one "\$@"
else
	while read -r line; do
		printf '[bluetooth]# %s\n' "\$line"
		one \$line
	done
fi
exit 0
EOF
	chmod 755 "$TMP/bin/bluetoothctl"
	for tool in systemctl rfkill bluealsa killall; do
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
		AGS_HCI_WAIT_S=1 AGS_INPUT="$TMP/input" \
		AGS_BTSTORE="$TMP/store" AGS_BTSAVED="$TMP/card/Config/bluetooth" \
		sh "$SCRIPT" "$@"
}

reset
run up && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "up exited $rc"
grep -q 'unblock bluetooth' "$TMP/rfkill.log" || fail "the radio was left blocked"
grep -q 'start bluetooth' "$TMP/systemctl.log" || fail "bluetoothd was not started"
grep -q 'pairable on' "$TMP/ctl.log" || fail "the radio cannot be paired with"
grep '^power' "$TMP/ctl.log" | head -2 | tr '\n' ' ' | grep -q '^power off power on' ||
	fail "bluetoothd did not power the radio on itself, so LE pads cannot pair"
grep -n 'power' "$TMP/ctl.log" | head -2 | tr '\n' ' ' | grep -q '^1:power off 2:power on' ||
	fail "bluetoothd did not power the radio on itself, so LE pads cannot pair"

reset
out="$(run devices)"
echo "$out" | grep -qx 'DEV AA:BB:CC:DD:EE:01|yes|yes|input-gaming|8BitDo Pro 2' ||
	fail "the coloured, paired pad was misread: $out"
echo "$out" | grep -qx 'DEV AA:BB:CC:DD:EE:02|no|no|audio-headset|WH-1000XM4' ||
	fail "the headset was misread: $out"
echo "$out" | grep -qx 'DEV AA:BB:CC:DD:EE:03|yes|no|input-gaming|Xbox Wireless Controller' ||
	fail "a pad with a link but no buttons was called connected: $out"
[ "$(grep -c '^info ' "$TMP/ctl.log")" = 3 ] || fail "the devices were not all looked up"

reset
rm "$TMP/input/input3/uniq"
run devices | grep -q '^DEV AA:BB:CC:DD:EE:01|yes|yes|' &&
	fail "a pad whose buttons never reached the kernel was called connected"

reset
export FAKE_CONNECTED=yes
run devices | grep -q '^DEV AA:BB:CC:DD:EE:02|no|yes|' &&
	fail "a headset with a link but no pairing was called connected"
unset FAKE_CONNECTED

reset
run pair AA:BB:CC:DD:EE:03 && rc=0 || rc=$?
[ "$rc" = 3 ] || fail "an LE pad that never bonded exited $rc, want 3"
grep -q '^remove AA:BB:CC:DD:EE:03' "$TMP/ctl.log" || fail "an LE pad that cannot work was kept"

reset
run pair AA:BB:CC:DD:EE:02 && rc=0 || rc=$?
[ "$rc" = 1 ] || fail "a pairing that never connected exited $rc, want 1"
grep -q -- '--agent NoInputNoOutput pair AA:BB:CC:DD:EE:02' "$TMP/ctl.log" || fail "pairing asked for a PIN"
grep -q 'trust AA:BB:CC:DD:EE:02' "$TMP/ctl.log" || fail "a paired device was not trusted, so it will not come back"
grep -q '^connect AA:BB:CC:DD:EE:02' "$TMP/ctl.log" || fail "pairing did not connect"

reset
export FAKE_CONNECTED=yes FAKE_PAIRED=yes
run connect AA:BB:CC:DD:EE:02 && rc=0 || rc=$?
unset FAKE_CONNECTED FAKE_PAIRED
[ "$rc" = 0 ] || fail "a connection that came up exited $rc"

reset
run down
grep -q 'power off' "$TMP/ctl.log" || fail "down left the radio powered"
grep -q 'block bluetooth' "$TMP/rfkill.log" || fail "down left the radio unblocked"
grep -q 'rtk_hciattach' "$TMP/killall.log" ||
	fail "down left the chip attached, so up would not load its firmware again"

run pair && rc=0 || rc=$?
[ "$rc" = 2 ] || fail "pair with no device exited $rc, want 2"

# bluez's store, as it keeps a paired device: ADAPTER/MAC/info.
paired() {
	mkdir -p "$1/68:8F:C9:B2:16:50/$2"
	echo "[LinkKey]" > "$1/68:8F:C9:B2:16:50/$2/info"
}
SAVED="$TMP/card/Config/bluetooth"

reset
paired "$SAVED" AA:BB:CC:DD:EE:01
run up
[ -f "$TMP/store/68:8F:C9:B2:16:50/AA:BB:CC:DD:EE:01/info" ] ||
	fail "the pairings saved on the card were not put back after a restart"
head -1 "$TMP/systemctl.log" | grep -q 'stop bluetooth' ||
	fail "a bluetoothd already running was left without the pairings put back"

reset
paired "$SAVED" AA:BB:CC:DD:EE:01
paired "$TMP/store" AA:BB:CC:DD:EE:02
run up
[ -d "$TMP/store/68:8F:C9:B2:16:50/AA:BB:CC:DD:EE:01" ] &&
	fail "the saved pairings were put over ones bluez made since boot"

reset
mkdir -p "$SAVED.new"
paired "$SAVED.new" AA:BB:CC:DD:EE:01
run up
[ -f "$TMP/store/68:8F:C9:B2:16:50/AA:BB:CC:DD:EE:01/info" ] ||
	fail "pairings left half saved by a power cut were lost"

reset
paired "$TMP/store" AA:BB:CC:DD:EE:01
run pair AA:BB:CC:DD:EE:01
[ -f "$SAVED/68:8F:C9:B2:16:50/AA:BB:CC:DD:EE:01/info" ] || fail "a pairing was not saved to the card"
[ -d "$SAVED.new" ] && fail "the copy being saved was left behind"

reset
paired "$SAVED" AA:BB:CC:DD:EE:01
run down
[ -f "$SAVED/68:8F:C9:B2:16:50/AA:BB:CC:DD:EE:01/info" ] ||
	fail "turning Bluetooth off before bluez had run wiped the saved pairings"

reset
paired "$SAVED" AA:BB:CC:DD:EE:01
run forget AA:BB:CC:DD:EE:01
[ -d "$SAVED/68:8F:C9:B2:16:50/AA:BB:CC:DD:EE:01" ] &&
	fail "a forgotten device came back from the card"

if [ "$FAILS" -gt 0 ]; then
	echo "$FAILS failure(s)"
	exit 1
fi
echo "slotbt: ok"
