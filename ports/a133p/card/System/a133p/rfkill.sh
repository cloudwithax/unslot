#!/bin/sh
# rfkill block|unblock TYPE, as slotwifi.sh and slotbt.sh call it, for a firmware without the
# rfkill tool: the kernel's switches are in sysfs.
case "${1:-}" in
block) soft=1 ;;
unblock) soft=0 ;;
*)
	echo "usage: rfkill.sh block|unblock TYPE" >&2
	exit 2
	;;
esac
for d in "${AGS_RFKILL_SYS:-/sys/class/rfkill}"/rfkill*; do
	[ "$(cat "$d/type" 2> /dev/null)" = "${2:-}" ] && echo "$soft" > "$d/soft"
done
exit 0
