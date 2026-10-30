#!/bin/sh
# Wi-Fi Direct for slot's Miracast: a supplicant of its own on wlan1, beside the Wi-Fi on wlan0.
# Slot talks to it over its control socket to find TVs and join their groups; this only does
# what needs a shell.
#
#   slotmiracast.sh up            start the supplicant on wlan1
#   slotmiracast.sh down          stop it and take wlan1 down
#   slotmiracast.sh lease IFACE   ask the TV leading the group on IFACE for an address, and
#                                 print it
#   slotmiracast.sh deconfig|bound|renew|...
#                                 what udhcpc calls as the lease comes and goes
set -u

NET="${AGS_NET_SYS:-/sys/class/net}"
RUN="${AGS_RUN:-/run}"
WPA_SUPPLICANT="${AGS_WPA_SUPPLICANT:-wpa_supplicant}"
WPA_CLI="${AGS_WPA_CLI:-wpa_cli}"
UDHCPC="${AGS_UDHCPC:-udhcpc}"
IP="${AGS_IP:-ip}"
WAIT_S="${AGS_WIFI_WAIT_S:-5}"

CTRL_DIR="$RUN/wpa_p2p"
CONF="$RUN/slotmiracast.conf"
LEASE="$RUN/slotmiracast.lease"
LINK_MARK="$RUN/slotlink.session"

p2p_up() {
	# A link session has wlan1 to itself.
	[ -f "$LINK_MARK" ] && return 1
	[ -d "$NET/wlan1" ] || return 1
	$IP link set wlan1 up 2>/dev/null || true
	/bin/mkdir -p "$CTRL_DIR"
	if ! $WPA_CLI -p "$CTRL_DIR" -i wlan1 ping 2>/dev/null | grep -q PONG; then
		# The TV leads the group, on wlan1 itself, and gives the address.
		{
			echo "ctrl_interface=$CTRL_DIR"
			echo 'device_name=unslot'
			echo 'device_type=1-0050F204-1'
			echo 'p2p_go_intent=0'
			echo 'p2p_no_group_iface=1'
		} > "$CONF"
		/bin/rm -f "$CTRL_DIR/wlan1"
		$WPA_SUPPLICANT -B -i wlan1 -c "$CONF" -Dnl80211 >/dev/null 2>&1 || return 1
	fi
	s=0
	while [ ! -e "$CTRL_DIR/wlan1" ] && [ "$s" -lt $((WAIT_S * 4)) ]; do
		sleep 0.25 2>/dev/null || sleep 1
		s=$((s + 1))
	done
	[ -e "$CTRL_DIR/wlan1" ]
}

p2p_down() {
	$WPA_CLI -p "$CTRL_DIR" -i wlan1 terminate >/dev/null 2>&1 || true
	# Gone before it returns, for an up straight after not to start a second over it.
	s=0
	while [ -e "$CTRL_DIR/wlan1" ] && [ "$s" -lt $((WAIT_S * 4)) ]; do
		sleep 0.25 2>/dev/null || sleep 1
		s=$((s + 1))
	done
	$IP addr flush dev wlan1 2>/dev/null || true
	$IP link set wlan1 down 2>/dev/null || true
	/bin/rm -f "$LEASE"
	return 0
}

lease() {
	/bin/rm -f "$LEASE"
	$UDHCPC -i "$1" -s "$0" -q -n -t 8 -T 1 >/dev/null 2>&1 || return 1
	[ -s "$LEASE" ] || return 1
	cat "$LEASE"
}

case "${1:-}" in
up) p2p_up ;;
down) p2p_down ;;
lease) lease "${2:-wlan1}" ;;
# udhcpc's own calls: the address goes on the group alone, the routes and DNS staying wlan0's.
deconfig) $IP addr flush dev "${interface:-wlan1}" 2>/dev/null || true ;;
bound | renew)
	$IP addr add "$ip/${mask:-24}" dev "$interface" 2>/dev/null || true
	echo "$ip" > "$LEASE"
	;;
leasefail | nak) ;;
*)
	echo "usage: slotmiracast.sh up | down | lease IFACE" >&2
	exit 2
	;;
esac
