#!/bin/sh
# Bluetooth for slot, through BaseOS's bluez. Every bluetoothctl call is under a timeout: with
# bluetoothd down it waits forever.
#
#   slotbt.sh up              attach the chip, start bluetoothd and bluealsa, power on
#   slotbt.sh down            power off and stop them
#   slotbt.sh scan SECS       look for devices for SECS seconds
#   slotbt.sh devices         every device bluez knows, one a line:
#                             DEV mac|paired|connected|icon|name
#   slotbt.sh pair MAC        pair, trust and connect
#   slotbt.sh connect MAC | disconnect MAC | forget MAC
set -u

BTCTL="${AGS_BLUETOOTHCTL:-bluetoothctl}"
SYSTEMCTL="${AGS_SYSTEMCTL:-systemctl}"
SETBT="${AGS_SETBT:-/mnt/vendor/ctrl/setBluetooth.sh}"
BLUEALSA="${AGS_BLUEALSA:-bluealsa}"
RFKILL="${AGS_RFKILL:-rfkill}"
HCI="${AGS_HCI:-/sys/class/bluetooth/hci0}"
TIMEOUT="${AGS_TIMEOUT:-timeout}"
HCI_WAIT_S="${AGS_HCI_WAIT_S:-7}"

ctl() {
	t="$1"
	shift
	$TIMEOUT "$t" $BTCTL "$@"
}

bt_up() {
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
	$RFKILL block bluetooth 2>/dev/null || true
	return 0
}

# bluetoothctl colours its output and prefixes prompts; keep only the plain field values.
field() {
	sed 's/\x1b\[[0-9;]*m//g' | sed -n "s/^[[:space:]]*$1: //p" | head -n 1
}

bt_devices() {
	ctl 5 devices 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | sed -n 's/^Device \([0-9A-Fa-f:]\{17\}\) .*/\1/p' |
		while read -r mac; do
			info="$(ctl 5 info "$mac" 2>/dev/null)"
			name="$(printf '%s\n' "$info" | field Name)"
			[ -n "$name" ] || name="$(printf '%s\n' "$info" | field Alias)"
			printf 'DEV %s|%s|%s|%s|%s\n' "$mac" \
				"$(printf '%s\n' "$info" | field Paired)" \
				"$(printf '%s\n' "$info" | field Connected)" \
				"$(printf '%s\n' "$info" | field Icon)" \
				"$name"
		done
}

connected() {
	ctl 5 info "$1" 2>/dev/null | field Connected | grep -q yes
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
	ctl 30 --agent NoInputNoOutput pair "$2" >/dev/null 2>&1 || true
	ctl 10 trust "$2" >/dev/null 2>&1 || true
	# Pairing leaves only the bare link up; connecting brings up the profile.
	ctl 15 connect "$2" >/dev/null 2>&1 || true
	connected "$2"
	;;
connect)
	[ -n "${2:-}" ] || exit 2
	ctl 15 connect "$2" >/dev/null 2>&1 || true
	connected "$2"
	;;
disconnect)
	[ -n "${2:-}" ] || exit 2
	ctl 10 disconnect "$2" >/dev/null 2>&1 || true
	! connected "$2"
	;;
forget)
	[ -n "${2:-}" ] || exit 2
	ctl 10 remove "$2" >/dev/null 2>&1
	;;
*)
	echo "usage: slotbt.sh up | down | scan SECS | devices | pair MAC | connect MAC | disconnect MAC | forget MAC" >&2
	exit 2
	;;
esac
