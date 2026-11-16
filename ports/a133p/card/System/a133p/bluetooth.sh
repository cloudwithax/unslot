#!/bin/sh
# BaseOS's setBluetooth.sh, as slotbt.sh calls it, for the A133P handhelds' XRadio chip on ttyS1:
# init powers it up and attaches it, after which the kernel has hci0; enable has nothing left
# to do.
case "${1:-}" in
init)
	echo 1 > /proc/bluetooth/sleep/btwrite 2> /dev/null
	echo 0 > /sys/class/rfkill/rfkill0/state
	sleep 1
	echo 1 > /sys/class/rfkill/rfkill0/state
	sleep 1
	killall hciattach 2> /dev/null
	hciattach -n ttyS1 xradio > /dev/null 2>&1 &
	;;
esac
exit 0
