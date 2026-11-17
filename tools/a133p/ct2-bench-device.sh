#!/bin/sh
# The handheld's half of ct2-bench.sh, copied to /mnt/SDCARD/prof/bench.sh. BusyBox 1.27 sh.
#   setup    back up what a run changes and put in a launcher that sources prof/bench.env
#   stop     hold launch.sh's loop and end slot
#   cool N   wait for the CPU to cool to N degrees
#   start    seat Crazy Taxi 2 on its backed-up resume state and VMUs, and let the loop start slot
#   sample   while slot runs: each thread's CPU and CPU time, about once a second, to stdout
#   restore  put everything setup backed up back and let the loop start slot as normal
# A run's traces and samples go to /tmp/bench-out, in memory, so writing them keeps off the card.
SD=/mnt/SDCARD
SYS="$SD/System"
P="$SD/prof"
B="$P/backup"
OUT=/tmp/bench-out
CT2="Crazy Taxi 2 (USA)"
RES="$SD/States/DC/nodosumi/$CT2/resume.state"
VMU="$SD/BIOS/dc/vmu/$CT2"
STATE="$SD/Config/slot.state"
# A process's command line, nothing for one that has gone.
cmd() {
	{ tr '\0' ' ' < "$1/cmdline"; } 2> /dev/null
}

slot_pids() {
	for d in /proc/[0-9]*; do
		case "$(cmd "$d")" in
		*System/slot*) echo "${d#/proc/}" ;;
		esac
	done
}

# launch.sh's loop: the parent of slot's main process.
loop_pid() {
	for p in $(slot_pids); do
		case "$(cmd "/proc/$p")" in
		*--core-host*) ;;
		*)
			q="$(cut -d ' ' -f 4 "/proc/$p/stat")"
			case "$(cmd "/proc/$q")" in
			*a133p/launch.sh*) echo "$q" && return ;;
			esac
			;;
		esac
	done
	cat "$P/loop.pid" 2> /dev/null
}

stop() {
	for d in /proc/[0-9]*; do
		case "$(cmd "$d")" in
		*"bench.sh sample"*) kill "${d#/proc/}" 2> /dev/null ;;
		esac
	done
	l="$(loop_pid)"
	[ -n "$l" ] || { echo "no launch.sh loop found" >&2; exit 1; }
	echo "$l" > "$P/loop.pid"
	kill -STOP "$l"
	p="$(slot_pids)"
	if [ -n "$p" ]; then
		kill $p 2> /dev/null
		n=0
		while [ -n "$(slot_pids)" ] && [ "$n" -lt 50 ]; do
			sleep 0.2
			n=$((n + 1))
		done
		p="$(slot_pids)"
		[ -z "$p" ] || kill -9 $p 2> /dev/null
		sleep 0.5
	fi
	reset
}

# What a run's pre=/post= may have changed, back as the firmware has it.
reset() {
	for f in /proc/irq/*/smp_affinity_list; do
		echo 0-3 > "$f"
	done 2> /dev/null
	echo f > /sys/devices/virtual/workqueue/cpumask 2> /dev/null
	if [ -f /tmp/bench-iso.txt ]; then
		while read -r tid cpus; do
			"$P/pin" "$cpus" -p "$tid" 2> /dev/null
		done < /tmp/bench-iso.txt
		rm -f /tmp/bench-iso.txt
	fi
	t=/sys/class/thermal/thermal_zone0
	[ -f "$B/trip_point_1_temp" ] || cat $t/trip_point_1_temp > "$B/trip_point_1_temp"
	cat "$B/trip_point_1_temp" > $t/trip_point_1_temp
}

# Keeps CPUs 2 and 3 to slot's core host: every other thread that may move, the unbound
# workqueues and the interrupts go to 0-1. Each thread's CPUs are kept for reset.
iso() {
	for d in /proc/[0-9]*; do
		case "$(cmd "$d")" in
		*System/slot*) continue ;;
		esac
		for t in "$d"/task/*; do
			c="$(awk '/^Cpus_allowed_list/ { print $2 }' "$t/status" 2> /dev/null)"
			[ -n "$c" ] && [ "$c" != 0-1 ] || continue
			"$P/pin" 0-1 -p "${t##*/}" 2> /dev/null && echo "${t##*/} $c" >> /tmp/bench-iso.txt
		done
	done
	echo 3 > /sys/devices/virtual/workqueue/cpumask
	for f in /proc/irq/*/smp_affinity_list; do
		echo 0-1 > "$f"
	done 2> /dev/null
	echo "moved $(wc -l < /tmp/bench-iso.txt) threads"
}

# Waits, up to ten minutes, for the CPU to cool to $1 degrees, so runs start alike.
cool() {
	n=0
	while [ "$(($(cat /sys/class/thermal/thermal_zone0/temp) / 1000))" -gt "$1" ] && [ "$n" -lt 600 ]; do
		sleep 5
		n=$((n + 5))
	done
	echo "cooled to $(cat /sys/class/thermal/thermal_zone0/temp) after ${n}s"
}

case "$1" in
setup)
	mkdir -p "$B/vmu"
	[ -f "$B/slot.state" ] || cp "$STATE" "$B/slot.state"
	[ -f "$B/resume.state" ] || cp "$RES" "$B/resume.state"
	[ -f "$B/vmu/vmu0.bin" ] || cp "$VMU"/* "$B/vmu/"
	[ -f "$B/launch_frontend.sh" ] || cp "$SYS/launch_frontend.sh" "$B/launch_frontend.sh"
	[ -f "$B/nodosumi_libretro.so" ] || cp "$SYS/nodosumi_libretro.so" "$B/nodosumi_libretro.so"
	for f in trace-ftrace.on trace-ftrace-fine.on; do
		[ -f "$SYS/$f" ] && mv "$SYS/$f" "$B/$f"
	done
	awk '/^\[ -f "\$LOGS\/slot.log" \] && mv/ { print "[ -f \"$SD/prof/bench.env\" ] && . \"$SD/prof/bench.env\"" } { print }' \
		"$B/launch_frontend.sh" > "$SYS/launch_frontend.sh"
	grep -c bench.env "$SYS/launch_frontend.sh"
	sync
	;;
stop) stop ;;
cool) cool "$2" ;;
iso) iso ;;
start)
	l="$(cat "$P/loop.pid")"
	[ -z "$(slot_pids)" ] || { echo "slot is still running" >&2; exit 1; }
	sed -i "s/^cart=.*/cart=$CT2/; s/^cart_platform=.*/cart_platform=dc/" "$STATE"
	cp "$B/resume.state" "$RES"
	cp "$B/vmu"/* "$VMU/"
	rm -f /run/slot-loading
	rm -rf "$OUT"
	mkdir -p "$OUT"
	cp /proc/interrupts "$OUT/interrupts.0"
	sync
	kill -CONT "$l"
	;;
sample)
	# "T uptime cpu-temp gpu-temp cpu0-kHz cooling-state", then per thread:
	# tid comm state cpu utime stime (ticks) priority nice
	z=/sys/class/thermal
	while [ -d "$OUT" ]; do
		echo "T $(cut -d ' ' -f 1 /proc/uptime) $(cat $z/thermal_zone0/temp $z/thermal_zone1/temp \
			/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq $z/cooling_device0/cur_state | tr '\n' ' ')"
		cat /proc/[0-9]*/task/[0-9]*/stat 2> /dev/null | awk '{
			o = index($0, "("); c = index($0, ") ");
			comm = substr($0, o + 1, c - o - 1); gsub(/ /, "_", comm);
			n = split(substr($0, c + 2), f, " ");
			print $1, comm, f[1], f[37], f[12], f[13], f[16], f[17]
		}'
		sleep 1
	done
	;;
restore)
	stop
	cp "$B/launch_frontend.sh" "$SYS/launch_frontend.sh"
	cp "$B/nodosumi_libretro.so" "$SYS/nodosumi_libretro.so"
	cp "$B/slot.state" "$STATE"
	cp "$B/resume.state" "$RES"
	cp "$B/vmu"/* "$VMU/"
	rm -f "$P/bench.env" /run/slot-loading
	sync
	kill -CONT "$(cat "$P/loop.pid")"
	;;
*)
	echo "usage: bench.sh setup|stop|start|sample|restore" >&2
	exit 2
	;;
esac
