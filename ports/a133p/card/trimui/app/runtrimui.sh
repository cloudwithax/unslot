#!/bin/sh
# /usr/trimui/bin/runtrimui.sh once trimui/app/MainUI has put it there: what the firmware runs to
# start its frontend. It gives the card a few seconds to mount, then runs the card's updater, or
# the firmware's own frontend for a card without one.
SD=/mnt/SDCARD
n=0
while ! grep -qi " $SD " /proc/mounts && [ "$n" -lt 10 ]; do
	sleep 1
	n=$((n + 1))
done
if [ -f "$SD/.tmp_update/updater" ]; then
	exec /bin/sh "$SD/.tmp_update/updater"
fi
exec /usr/trimui/bin/runtrimui-original.sh
