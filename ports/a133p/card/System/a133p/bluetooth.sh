#!/bin/sh
# BaseOS's setBluetooth.sh, as slotbt.sh calls it, for the A133P handhelds' XRadio chip on ttyS1.
# The firmware attaches it itself at boot, `hciattach -n ttyS1 xradio`, which takes some seconds
# to load the chip's firmware: init leaves one that is running to finish, and only powers the
# chip up and attaches it when none is. enable has nothing left to do.
case "${1:-}" in
init)
	pidof hciattach > /dev/null && exit 0
	echo 1 > /proc/bluetooth/sleep/btwrite 2> /dev/null
	echo 0 > /sys/class/rfkill/rfkill0/state
	sleep 1
	echo 1 > /sys/class/rfkill/rfkill0/state
	sleep 1
	hciattach -n ttyS1 xradio > /dev/null 2>&1 &
	;;
esac
exit 0
