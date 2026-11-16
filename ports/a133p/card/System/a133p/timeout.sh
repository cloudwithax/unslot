#!/bin/sh
# timeout SECS COMMAND [ARG...], for TrimUI's firmware, whose BusyBox has none: runs COMMAND,
# stops it after SECS, and exits 124 if it had to, as coreutils' timeout does. COMMAND keeps
# this script's input, as slotbt.sh pipes bluetoothctl its commands.
secs="$1"
shift
# A command put in the background reads /dev/null unless given its input another way, so this
# script's input goes to it through a copy on fd 3.
exec 3<&0
"$@" <&3 3<&- &
pid=$!
(
	sleep "$secs"
	kill "$pid" 2> /dev/null
) &
watch=$!
wait "$pid"
rc=$?
kill "$watch" 2> /dev/null
[ "$rc" -eq 143 ] && exit 124
exit "$rc"
