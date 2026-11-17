#!/bin/sh
SD="${SLOT_ROOT:-/mnt/sdcard}"
SYS="$SD/System"
RUN="${AGS_RUN:-/run}"
LOGS="$SD/Logs"
SESSION_MAX=65536
export SLOT_ROOT="$SD"
# Where slot marks a cart while it loads, so one that takes it down is not seated again.
export SLOT_RUN="$RUN"

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
#
# UNSLOT_CHIP, which only System/a133p/launch.sh sets, says which chip's packs these are: an
# A133P's are unslot-<version>-a133p.pak and install through .tmp_update/a133p.sh. Only the
# pack's own files leave .tmp_update, as a TrimUI card boots from the updater beside them.
CHIP="${UNSLOT_CHIP:-h700}"
[ "$CHIP" = a133p ] && [ ! -f "$SYS/trace-radio.on" ] && unset AGS_RADIO_TRACE AGS_WPA_LOG
PAKS="unslot-*.pak"
[ "$CHIP" = h700 ] || PAKS="unslot-*-$CHIP.pak"
SLOT_BIN="${AGS_SLOT:-/lib/ld-linux-aarch64.so.1 $SYS/slot}"
# TrimUI's BusyBox sort has no -V; it fails before it reads, so a plain sort still gets the list.
pak="$(ls -1 "$SD"/$PAKS 2>/dev/null | { sort -V 2>/dev/null || sort; } | tail -n 1)"
if [ -n "$pak" ]; then
	mkdir -p "$SD/.unslot-update"
	zip="$SD/.unslot-update/pak.zip"
	if version="$($SLOT_BIN --unpack-pak "$pak" "$zip" 2>> "$LOGS/update.log")"; then
		rm -rf "$SD/.tmp_update/unslot" "$SD/.tmp_update/$CHIP.sh"
		if unzip -o -q "$zip" '.tmp_update/*' -d "$SD" && sh "$SD/.tmp_update/$CHIP.sh" > /dev/null 2>&1; then
			rm -f "$SD"/$PAKS
		else
			echo "$(date '+%F %T') $version did not install; left as $(basename "$pak").bad" >> "$LOGS/update.log"
			mv -f "$pak" "$pak.bad"
			rm -rf "$SD/.tmp_update/unslot" "$SD/.tmp_update/$CHIP.sh"
			rmdir "$SD/.tmp_update" 2>/dev/null
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
		if mv "$SYS" "$UPD/System.failed" && mv "$UPD/System.old" "$SYS"; then
			# The runtimes ports downloaded since are the user's, not the update's.
			for f in "$UPD/System.failed/PortMaster/libs"/*.squashfs; do
				[ -e "$f" ] && [ ! -e "$SYS/PortMaster/libs/${f##*/}" ] || continue
				mkdir -p "$SYS/PortMaster/libs" && mv "$f" "$SYS/PortMaster/libs/"
			done
			rm -rf "$UPD/System.failed"
		fi
		rm -f "$UPD/trial"
	else
		echo "$n" > "$UPD/trial"
	fi
	sync
fi

# The power LED, through BaseOS's AXP2202 driver; TrimUI's firmware does not let it be set.
pid="$(cat "$RUN/slot-led.pid" 2>/dev/null)"
if [ "$CHIP" = h700 ] && { [ -z "$pid" ] || [ ! -d "/proc/$pid" ]; }; then
	sh "$SYS/led.sh" once
	sh "$SYS/led.sh" loop > /dev/null 2>&1 &
	echo $! > "$RUN/slot-led.pid"
fi

echo "|/bin/sh $SYS/coresave.sh %p %s %e %t" > /proc/sys/kernel/core_pattern 2>/dev/null

# BaseOS runs the CPU flat out but leaves the Mali on its lowest clock, 420 MHz, which its
# governor never raises; the driver's performance scene holds it at 648 MHz.
echo 1 > /sys/class/devfreq/gpu/device/scenectrl/command 2>/dev/null

# A port slot quit for, from the Apps page, or DraStic for a DS game: it runs from its own folder
# with slot gone, and slot starts once it ends. The note's second line, when it has one, is what
# to hand it. The note goes first, so a port that hangs the device is not run again.
#
# A port's script from PortMaster looks for PortMaster at /opt/tools/PortMaster and for its own
# files under /roms/ports, and is written for bash, whose >(tee log.txt) needs /dev/fd; BaseOS
# has none of them, so the runtime in System/PortMaster, the card's Ports, bash and /proc's fd
# links are put there, and System/bin, with bash, goes on the PATH. A real folder or file
# already at one of those places is left alone. SLOT_FS moves the root filesystem, for the tests.
#
# Before such a port runs, slot offers the PortMaster runtimes its port.json names and the card
# has not got for download, on screen, and the port is not run without them. While it runs, slot
# reads the volume keys and sets the codec's volume by them, as ports play through ALSA alone.
FS="${SLOT_FS:-}"

# A firmware with no root certificates of its own (TrimUI's stock OS has no /etc/ssl) uses the
# Mozilla CA bundle on the card, so curl, Syncthing, Tailscale and ports can verify HTTPS.
if [ ! -f "$FS/etc/ssl/certs/ca-certificates.crt" ] && [ -f "$SYS/ssl/ca-certificates.crt" ]; then
	export SSL_CERT_FILE="$SYS/ssl/ca-certificates.crt"
	export CURL_CA_BUNDLE="$SSL_CERT_FILE"
fi

# link TARGET AT: AT made a link to TARGET, unless something other than a link is there.
link() {
	[ -e "$2" ] && [ ! -L "$2" ] && return 0
	mkdir -p "$(dirname "$2")" && ln -sfn "$1" "$2"
}

if [ -f "$SD/.unslot-port" ]; then
	port="$(sed -n 1p "$SD/.unslot-port")"
	arg="$(sed -n 2p "$SD/.unslot-port")"
	rm -f "$SD/.unslot-port"
	sync
	if [ -f "$port" ]; then
		case "$port" in
		"$SD/Ports/"*)
			link "$SYS/PortMaster" "$FS/opt/tools/PortMaster"
			link "$SD/Ports" "$FS/roms/ports"
			[ -e "$FS/bin/bash" ] || link "$SYS/bin/bash" "$FS/bin/bash"
			[ -e "$FS/dev/fd" ] || link /proc/self/fd "$FS/dev/fd"
			for i in 0:stdin 1:stdout 2:stderr; do
				[ -e "$FS/dev/${i#*:}" ] || link "/proc/self/fd/${i%%:*}" "$FS/dev/${i#*:}"
			done
			export PATH="$SYS/bin:$PATH"
			portmaster=1
			;;
		esac
		run=sh
		head -n 1 "$port" | grep -q bash && command -v bash > /dev/null && run=bash
		set --
		[ -n "$arg" ] && set -- "$arg"
		log "port $port $*"
		if [ -n "${portmaster:-}" ] && ! $SLOT_BIN --port-runtimes "$port" >> "$LOGS/ports.log" 2>&1; then
			log "port not run: a runtime it needs is not on the card"
		else
			(
				cd "$(dirname "$port")" || exit
				# The foreground supervisor owns the port and restores the mixer before returning.
				if [ -n "${portmaster:-}" ]; then
					$SLOT_BIN --port-volume "$run" "$port" "$@"
				else
					$run "$port" "$@"
				fi
			) >> "$LOGS/ports.log" 2>&1
			log "port ended $?"
		fi
	fi
fi

# Profiling. With System/trace-ftrace.on, slot records its frame trace (SLOT_FTRACE, which its core
# host joins), Nodosumi its own (NODOSUMI_FTRACE) and slot logs its pacing and input latency, all
# for tools/ftrace_dump.py and Nodosumi's tools/ftrace/analyze.py. They go to Logs/ftrace; a start
# after one where a core recorded keeps that one, with its slot.log, in Logs/ftrace.1. With
# System/trace-ftrace-fine.on as well, Nodosumi also times the zones it enters thousands of times
# a frame and counts each frame's SH4 and ARM7 instructions and the SH4's loads and stores that
# miss fastmem, which costs more.
if [ -f "$SYS/trace-ftrace.on" ]; then
	TRACE="$LOGS/ftrace"
	if [ -f "$TRACE/nodo-trace.bin" ]; then
		rm -rf "$TRACE.1"
		mv "$TRACE" "$TRACE.1"
		[ -f "$LOGS/slot.log" ] && cp "$LOGS/slot.log" "$TRACE.1/slot.log"
	fi
	mkdir -p "$TRACE"
	export SLOT_FTRACE="$TRACE/slot-trace.bin" NODOSUMI_FTRACE="$TRACE/nodo-trace.bin" SLOT_TRACE_LATENCY=1
	[ -f "$SYS/trace-ftrace-fine.on" ] && export NODOSUMI_FTRACE_FINE=1 NODOSUMI_COUNT_INSTRS=1
	log "tracing to $TRACE${NODOSUMI_FTRACE_FINE:+, fine}"
fi

[ -f "$LOGS/slot.log" ] && mv -f "$LOGS/slot.log" "$LOGS/slot.log.1"
log "exec $SYS/slot"

exec /lib/ld-linux-aarch64.so.1 "$SYS/slot" > "$LOGS/slot.log" 2>&1
