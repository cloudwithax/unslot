#!/bin/sh
SD="${SLOT_ROOT:-/mnt/sdcard}"
SYS="$SD/System"
RUN="${AGS_RUN:-/run}"
LOGS="$SD/Logs"
SESSION_MAX=65536
export SLOT_ROOT="$SD"

mkdir -p "$LOGS" 2>/dev/null
for f in slot.log slot.log.1 slot-session.log slot-session.log.1 slot-card.log ags-shutdown-trace.log input-trace.log; do
	[ -f "$SD/$f" ] && mv -f "$SD/$f" "$LOGS/$f"
done
[ -d "$SD/crash" ] && [ ! -e "$LOGS/crash" ] && mv "$SD/crash" "$LOGS/crash"

[ $(wc -c < "$LOGS/slot-session.log" 2>/dev/null || echo 0) -gt "$SESSION_MAX" ] \
	&& mv -f "$LOGS/slot-session.log" "$LOGS/slot-session.log.1"

log() {
	echo "$(date '+%F %T') $*" >> "$LOGS/slot-session.log" 2>/dev/null
}

# An update at the card's root, unslot-<version>.pak, from the Updates page or copied there, goes
# in only if unslot's release signed it: slot checks the signature and the MinUI.zip inside it
# against the version and checksum it signed, and writes the zip out untouched. That is then
# unpacked and installed as BaseOS installs a MinUI.zip. A pack that does not check out is left
# as .bad, and System is not touched.
SLOT_BIN="${AGS_SLOT:-/lib/ld-linux-aarch64.so.1 $SYS/slot}"
pak="$(ls -1 "$SD"/unslot-*.pak 2>/dev/null | tail -n 1)"
if [ -n "$pak" ]; then
	mkdir -p "$SD/.unslot-update"
	zip="$SD/.unslot-update/pak.zip"
	if version="$($SLOT_BIN --unpack-pak "$pak" "$zip" 2>> "$LOGS/update.log")"; then
		rm -rf "$SD/.tmp_update"
		if unzip -o -q "$zip" '.tmp_update/*' -d "$SD" && sh "$SD/.tmp_update/h700.sh" > /dev/null 2>&1; then
			rm -f "$SD"/unslot-*.pak
		else
			echo "$(date '+%F %T') $version did not install; left as $(basename "$pak").bad" >> "$LOGS/update.log"
			mv -f "$pak" "$pak.bad"
			rm -rf "$SD/.tmp_update"
		fi
	else
		echo "$(date '+%F %T') $(basename "$pak") is not an update unslot signed; left as .bad" >> "$LOGS/update.log"
		mv -f "$pak" "$pak.bad"
	fi
	rm -f "$zip"
	sync
fi

# After an update pack's installer swaps System, the new one has three starts to draw a frame,
# which slot marks by clearing the trial; on the fourth the replaced one goes back, as BaseOS
# does with its own updates.
UPD="$SD/.unslot-update"
if [ -f "$UPD/trial" ] && [ -d "$UPD/System.old" ]; then
	n="$(cat "$UPD/trial" 2>/dev/null)"
	case "$n" in '' | *[!0-9]*) n=0 ;; esac
	n=$((n + 1))
	if [ "$n" -gt "${AGS_UPDATE_TRIES:-3}" ]; then
		echo "$(date '+%F %T') the update never started; putting the old System back" >> "$LOGS/update.log"
		rm -rf "$UPD/System.failed"
		mv "$SYS" "$UPD/System.failed" && mv "$UPD/System.old" "$SYS" && rm -rf "$UPD/System.failed"
		rm -f "$UPD/trial"
	else
		echo "$n" > "$UPD/trial"
	fi
	sync
fi

pid="$(cat "$RUN/slot-led.pid" 2>/dev/null)"
if [ -z "$pid" ] || [ ! -d "/proc/$pid" ]; then
	sh "$SYS/led.sh" once
	sh "$SYS/led.sh" loop > /dev/null 2>&1 &
	echo $! > "$RUN/slot-led.pid"
fi

echo "|/bin/sh $SYS/coresave.sh %p %s %e %t" > /proc/sys/kernel/core_pattern 2>/dev/null

# BaseOS runs the CPU flat out but leaves the Mali on its lowest clock, 420 MHz, which its
# governor never raises; the driver's performance scene holds it at 648 MHz.
echo 1 > /sys/class/devfreq/gpu/device/scenectrl/command 2>/dev/null

[ -f "$LOGS/slot.log" ] && mv -f "$LOGS/slot.log" "$LOGS/slot.log.1"
log "exec $SYS/slot"

exec /lib/ld-linux-aarch64.so.1 "$SYS/slot" > "$LOGS/slot.log" 2>&1
