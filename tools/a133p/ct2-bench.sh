#!/bin/sh
# Crazy Taxi 2 on the Brick Pro, the same drive every time, for A/B runs: slot restarts into the
# game on the resume state backed up at setup, Nodosumi's bot loads prof/ct2-drive.state and drives
# ct2-bench.bot's loop, and the traces, a per-thread CPU sample and slot.log come back to
# $CT2_BENCH_OUT/NAME (default /tmp/ct2/bench) with analyze.py's summary.
#
#   sh tools/a133p/ct2-bench.sh setup
#   sh tools/a133p/ct2-bench.sh run NAME SECS [core=LOCAL.so] [trace=coarse|fine|off] [sample=0]
#                                   [bot=FILE] [cool=DEGREES] [pre=CMD] [post=CMD] [VAR=value...]
#   sh tools/a133p/ct2-bench.sh table DIR...
#   sh tools/a133p/ct2-bench.sh restore
#
# VAR=value lines go into prof/bench.env, which the launcher setup puts in sources before it starts
# slot (SLOT_CPUS=off, ...). core= copies that .so in for the run, otherwise the one setup found
# runs; restore puts that one back. Each run first waits for the CPU to cool to cool= (default
# 50 °C). pre= runs on the handheld before slot starts, post= once the bot has loaded its state
# (`sh /mnt/SDCARD/prof/bench.sh iso` moves everything else off CPUs 2-3); stopping puts IRQs,
# thread CPUs and the thermal trip back. The drive state is made once, from a resume state on the
# mode select menu, with `run mkstate 40 trace=off bot=tools/a133p/ct2-mkstate.bot`. Runs need the
# core built with nodosumi-bot-frames.patch to replay the same frames.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
H="${CT2_BENCH_HOST:-root@192.168.1.218}"
O="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR"
OUT="${CT2_BENCH_OUT:-/tmp/ct2/bench}"
P=/mnt/SDCARD/prof
T=/tmp/bench-out
NODO="${NODOSUMI_SRC:-$HOME/nodosumi}"
dev() { sshpass -p tina ssh $O "$H" "$@"; }
put() { sshpass -p tina scp $O "$1" "$H:$2"; }
get() { sshpass -p tina scp $O "$H:$1" "$2"; }

shot() {
	dev "dd if=/dev/fb0 bs=4096 count=768 of=/tmp/bench-fb.raw 2> /dev/null" &&
		get /tmp/bench-fb.raw "$1.raw" &&
		python3 -c "
import sys
from PIL import Image
d = open(sys.argv[1], 'rb').read()[:1024 * 768 * 4]
Image.frombytes('RGBA', (1024, 768), d, 'raw', 'BGRA').convert('RGB').save(sys.argv[2])
" "$1.raw" "$1.png" && rm -f "$1.raw"
}

case "${1:-}" in
setup)
	dev "mkdir -p $P"
	put "$here/ct2-bench-device.sh" "$P/bench.sh"
	mkdir -p "$OUT"
	"$here/zcc" -O2 -o "$OUT/pin" "$here/pin.c"
	put "$OUT/pin" "$P/pin"
	dev "chmod +x $P/pin && sh $P/bench.sh setup"
	;;
run)
	name="$2" secs="$3"
	shift 3
	core='' trace=coarse sample=1 bot="$here/ct2-bench.bot" env='' pre='' post='' cool=50
	for a do
		case "$a" in
		core=*) core="${a#core=}" ;;
		trace=*) trace="${a#trace=}" ;;
		sample=*) sample="${a#sample=}" ;;
		bot=*) bot="${a#bot=}" ;;
		pre=*) pre="${a#pre=}" ;;
		post=*) post="${a#post=}" ;;
		cool=*) cool="${a#cool=}" ;;
		*=*) env="$env
export $a" ;;
		*) echo "not VAR=value: $a" >&2; exit 2 ;;
		esac
	done
	case "$trace" in
	off | coarse | fine) ;;
	*) echo "trace=coarse|fine|off" >&2; exit 2 ;;
	esac
	o="$OUT/$name"
	rm -rf "$o"
	mkdir -p "$o"
	dev "sh $P/bench.sh stop"
	put "$bot" "$P/bot.txt"
	if [ -n "$core" ]; then
		put "$core" /mnt/SDCARD/System/nodosumi_libretro.so
	else
		dev "cp $P/backup/nodosumi_libretro.so /mnt/SDCARD/System/nodosumi_libretro.so"
	fi
	{
		echo "export NODOSUMI_BOT=$P/bot.txt SLOT_TRACE_LATENCY=1"
		if [ "$trace" != off ]; then
			echo "export SLOT_FTRACE=$T/slot-trace.bin NODOSUMI_FTRACE=$T/nodo-trace.bin"
		fi
		if [ "$trace" = fine ]; then
			echo "export NODOSUMI_FTRACE_FINE=1 NODOSUMI_COUNT_INSTRS=1"
		fi
		echo "${env#?}"
	} > "$o/bench.env"
	put "$o/bench.env" "$P/bench.env"
	dev "sh $P/bench.sh cool $cool" | tee "$o/cool.txt"
	[ -z "$pre" ] || dev "$pre"
	dev "sh $P/bench.sh start"
	t=0
	until dev "grep -aq 'nodosumi: bot \(load\|save\)' /mnt/SDCARD/Logs/slot.log 2> /dev/null"; do
		t=$((t + 2))
		[ "$t" -lt 150 ] || { echo "$name: the bot never loaded its state" >&2; dev "sh $P/bench.sh stop"; exit 1; }
		sleep 2
	done
	[ -z "$post" ] || dev "$post"
	[ "$sample" = 0 ] || dev "cd $P && (./pin 0 -n 19 sh bench.sh sample > $T/sample.txt 2> /dev/null < /dev/null &)"
	for s in 1 2 3; do
		sleep $((secs / 4))
		shot "$o/shot$s" || true
	done
	sleep $((secs - 3 * (secs / 4)))
	dev 'for p in /proc/[0-9]*; do case "$(tr "\0" " " < $p/cmdline 2> /dev/null)" in *--core-host*) for t in $p/task/*; do echo "${t##*/} $(cat $t/comm) $(grep -E "^(voluntary|nonvoluntary)" $t/status | tr -s "\t\n" "  ")"; done;; esac; done' > "$o/switches.txt" || true
	dev "cp /proc/interrupts $T/interrupts.1; sh $P/bench.sh stop; mv /tmp/perf-*.map $T/ 2> /dev/null; true"
	for f in $(dev "ls $T"); do
		get "$T/$f" "$o/$f"
	done
	dev "rm -rf $T"
	get /mnt/SDCARD/Logs/slot.log "$o/slot.log"
	grep -a 'nodosumi: bot \|slot: cpus' "$o/slot.log" > "$o/start.txt" || true
	if [ "$trace" != off ]; then
		mkdir -p "$o/analysis"
		python3 "$NODO/tools/ftrace/analyze.py" --core "$o/nodo-trace.bin" --slot "$o/slot-trace.bin" \
			--skip-s 0 --out "$o/analysis" > "$o/analysis/summary.txt" 2>&1 || true
	fi
	python3 "$here/ct2-bench-table.py" "$o"
	;;
table)
	shift
	python3 "$here/ct2-bench-table.py" "$@"
	;;
restore)
	dev "sh $P/bench.sh restore"
	;;
*)
	sed -n '2,17p' "$0" >&2
	exit 2
	;;
esac
