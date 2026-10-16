#!/bin/sh
# Wi-Fi for slot: starts and stops wpa_supplicant on wlan0 and keeps a DHCP lease while it is
# connected. Slot talks to the supplicant itself over its control socket for scanning and
# joining; this only does what needs a shell.
#
#   slotwifi.sh up CONF     start the supplicant on CONF, and the watcher that runs DHCP
#   slotwifi.sh down        stop both, release the lease and turn the radio off
#   slotwifi.sh wlan0 CONNECTED|DISCONNECTED
#                           what wpa_cli's watcher calls as the link comes and goes
set -u

NET="${AGS_NET_SYS:-/sys/class/net}"
RUN="${AGS_RUN:-/run}"
CTRL_DIR="${AGS_CTRL_DIR:-/var/run/wpa_supplicant}"
WPA_SUPPLICANT="${AGS_WPA_SUPPLICANT:-wpa_supplicant}"
WPA_CLI="${AGS_WPA_CLI:-wpa_cli}"
UDHCPC="${AGS_UDHCPC:-udhcpc}"
IP="${AGS_IP:-ip}"
RFKILL="${AGS_RFKILL:-rfkill}"
WAIT_S="${AGS_WIFI_WAIT_S:-5}"

LINK_MARK="$RUN/slotlink.session"
LINK_WAIT_S="${AGS_WIFI_LINK_WAIT_S:-10}"
WATCH_PID="$RUN/slotwifi-watch.pid"
LEASE_PID="$RUN/slotwifi-udhcpc.pid"

stop_pid() {
	[ -f "$1" ] || return 0
	kill "$(cat "$1")" 2>/dev/null || true
	/bin/rm -f "$1"
}

wifi_up() {
	conf="$1"
	# A link session owns wlan0 until slotlink.sh takes it down and clears its mark.
	m=0
	while [ -f "$LINK_MARK" ] && [ "$m" -lt $((LINK_WAIT_S * 4)) ]; do
		sleep 0.25 2>/dev/null || sleep 1
		m=$((m + 1))
	done
	w=0
	while [ ! -d "$NET/wlan0" ] && [ "$w" -lt 40 ]; do
		sleep 0.25 2>/dev/null || sleep 1
		w=$((w + 1))
	done
	[ -d "$NET/wlan0" ] || return 1
	$RFKILL unblock wifi 2>/dev/null || true
	$IP link set wlan0 up 2>/dev/null || true
	/bin/mkdir -p "$CTRL_DIR"
	if [ ! -s "$conf" ]; then
		/bin/mkdir -p "$(dirname "$conf")"
		{
			echo "ctrl_interface=$CTRL_DIR"
			echo 'update_config=1'
		} > "$conf"
	fi
	if ! $WPA_CLI -p "$CTRL_DIR" -i wlan0 ping 2>/dev/null | grep -q PONG; then
		/bin/rm -f "$CTRL_DIR/wlan0"
		$WPA_SUPPLICANT -B -i wlan0 -c "$conf" -Dnl80211 >/dev/null 2>&1 || return 1
	fi
	s=0
	while [ ! -e "$CTRL_DIR/wlan0" ] && [ "$s" -lt $((WAIT_S * 4)) ]; do
		sleep 0.25 2>/dev/null || sleep 1
		s=$((s + 1))
	done
	[ -e "$CTRL_DIR/wlan0" ] || return 1
	stop_pid "$WATCH_PID"
	$WPA_CLI -p "$CTRL_DIR" -i wlan0 -B -P "$WATCH_PID" -a "$0" >/dev/null 2>&1 || true
	return 0
}

wifi_down() {
	stop_pid "$WATCH_PID"
	stop_pid "$LEASE_PID"
	$WPA_CLI -p "$CTRL_DIR" -i wlan0 terminate >/dev/null 2>&1 || true
	$IP addr flush dev wlan0 2>/dev/null || true
	$IP link set wlan0 down 2>/dev/null || true
	$RFKILL block wifi 2>/dev/null || true
	return 0
}

lease() {
	stop_pid "$LEASE_PID"
	$UDHCPC -i wlan0 -b -p "$LEASE_PID" >/dev/null 2>&1 || true
}

case "${1:-} ${2:-}" in
"up "*) wifi_up "${2:-}" ;;
"down "*) wifi_down ;;
"wlan0 CONNECTED") lease ;;
"wlan0 DISCONNECTED")
	stop_pid "$LEASE_PID"
	$IP addr flush dev wlan0 2>/dev/null || true
	;;
*)
	echo "usage: slotwifi.sh up CONF | down" >&2
	exit 2
	;;
esac
