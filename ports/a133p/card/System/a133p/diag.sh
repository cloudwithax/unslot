#!/bin/sh
# What TrimUI's firmware has for Wi-Fi, Bluetooth and the screen, written to Logs/a133p-diag.txt
# at each boot, for bringing the A133P port up from a card alone.
OUT="${1:-/mnt/SDCARD/Logs/a133p-diag.txt}"

section() {
	echo
	echo "===== $*"
}

# show FILE...: each file's text, under its name.
show() {
	for f in "$@"; do
		[ -e "$f" ] || continue
		echo "--- $f"
		cat "$f" 2> /dev/null | sed 's/^\([[:space:]]*psk=\).*/\1<hidden>/'
	done
}

{
	date '+%F %T'
	section system
	uname -a
	show /etc/version /etc/os-release /etc/openwrt_release
	busybox 2>&1 | head -n 1
	ls -l /lib/libc.so.6 /lib/libc-* 2> /dev/null
	for t in wpa_supplicant wpa_cli udhcpc ip iw rfkill hciattach hciconfig bluetoothctl bluetoothd \
		bluealsa btmgr dbus-daemon timeout unzip curl amixer strings; do
		printf '%s: %s\n' "$t" "$(command -v "$t" || echo none)"
	done
	wpa_supplicant -v 2>&1 | head -n 2
	section processes
	ps w 2> /dev/null || ps
	section modules
	lsmod 2> /dev/null || cat /proc/modules
	section network
	ls -l /sys/class/net
	for n in /sys/class/net/*; do
		[ -e "$n/device" ] && echo "$(basename "$n"): driver $(basename "$(readlink "$n/device/driver" 2> /dev/null)")"
	done
	ip addr 2> /dev/null
	iw dev 2> /dev/null
	iw phy 2> /dev/null | grep -iE 'wiphy|cipher|AKM|offload|4-way|SAE|WPA' | head -n 40
	section rfkill
	for r in /sys/class/rfkill/rfkill*; do
		echo "$(basename "$r"): name=$(cat "$r/name" 2> /dev/null) type=$(cat "$r/type" 2> /dev/null) state=$(cat "$r/state" 2> /dev/null) soft=$(cat "$r/soft" 2> /dev/null) hard=$(cat "$r/hard" 2> /dev/null)"
	done
	section "wifi config and init"
	ls -la /etc/wifi /etc/init.d 2> /dev/null
	show /etc/init.d/wpa_supplicant /etc/init.d/wifi /etc/init.d/network /etc/wifi/*.sh /etc/wifi/*.conf
	section bluetooth
	ls -la /etc/bluetooth /sys/class/bluetooth /proc/bluetooth /proc/bluetooth/sleep 2> /dev/null
	show /etc/bluetooth/*.sh /etc/bluetooth/bluetoothd /etc/bluetooth/main.conf /etc/init.d/bluetooth* /etc/init.d/bt*
	ls -l /dev/ttyS* 2> /dev/null
	section "stock launcher"
	show /usr/trimui/bin/runtrimui-original.sh /etc/init.d/runtrimui
	section screen
	show /sys/class/graphics/fb0/virtual_size /sys/class/graphics/fb0/bits_per_pixel /sys/class/graphics/fb0/stride /sys/class/disp/disp/attr/sys
	section "kernel log"
	dmesg 2> /dev/null | tail -n 400
} > "$OUT" 2>&1
