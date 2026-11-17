#!/usr/bin/env python3
"""One row per ct2-bench.sh run, over the same emulated frames in every run: from the bot's state
load plus SKIP frames, FRAMES frames (default 300 and 7200, two minutes of the drive). Emu work
(wall less render/CHD waits) and CPU per frame, frames over 16.7 ms, the emu thread's involuntary
switches, slot's repeated presents and audio underruns over that stretch, and, with -v, the CPU
clock and temperature and what ran on each CPU (from sample.txt).

  ct2-bench-table.py [-v] [--skip N] [--frames N] RUN_DIR...
"""
import bisect
import csv
import os
import re
import statistics as st
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path

NODO = os.environ.get("NODOSUMI_SRC", os.path.expanduser("~/nodosumi"))
WAITS = ("z_start_render_wait", "z_finish_render_wait", "z_chd_wait", "z_res_lock")


def pct(xs, p):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(p / 100 * len(xs)))]


def frames(d, skip, count):
    path = d / "analysis" / "emu_frames.csv"
    if not path.exists():
        return None
    rows = list(csv.DictReader(open(path)))
    if not rows:
        return None
    f0 = int(rows[0]["arg"])
    lo, hi = f0 + skip, f0 + skip + count
    win = [r for r in rows if lo <= int(r["arg"]) < hi]
    return rows[0], win, int(rows[-1]["arg"]) >= hi - 1


def slot_window(d, start, win):
    """Re-runs analyze.py over the window's wall-clock span for slot's side of it."""
    out = d / "analysis" / "window"
    s = out / "summary.txt"
    if not s.exists():
        out.mkdir(parents=True, exist_ok=True)
        skip = (int(win[0]["t"]) - int(start["t"])) / 1e9
        secs = (int(win[-1]["t"]) + int(win[-1]["dur"]) - int(win[0]["t"])) / 1e9
        with open(s, "w") as fp:
            subprocess.run([sys.executable, f"{NODO}/tools/ftrace/analyze.py", "--core", str(d / "nodo-trace.bin"),
                            "--slot", str(d / "slot-trace.bin"), "--skip-s", f"{skip:.3f}", "--secs", f"{secs:.3f}",
                            "--out", str(out)], stdout=fp, stderr=subprocess.STDOUT)
    text = s.read_text(errors="replace")
    r = {}
    m = re.search(r"presents with a new frame (\d+), repeated (\d+)", text)
    if m:
        r["new"], r["rep"] = int(m[1]), int(m[2])
    m = re.search(r"ring\.underrun on slot-audio: (\d+)", text)
    r["under"] = int(m[1]) if m else 0
    return r


def samples(d, t_lo, t_hi, top=6):
    s = d / "sample.txt"
    if not s.exists():
        return None, None
    prev, cur = {}, {}
    t_prev = None
    temps, freqs = [], []
    per_cpu = defaultdict(Counter)
    span = 0.0

    def close(t):
        nonlocal span
        if t_prev is None or not (t_lo <= t_prev and t <= t_hi):
            return
        span += t - t_prev
        for tid, (comm, cpu, ticks) in cur.items():
            if tid in prev:
                per_cpu[cpu][comm] += ticks - prev[tid][2]

    t = None
    for line in s.read_text(errors="replace").splitlines():
        f = line.split()
        if not f:
            continue
        if f[0] == "T":
            if t is not None:
                close(t)
                prev, t_prev = cur, t
            cur = {}
            t = float(f[1]) * 1e9
            if len(f) >= 6 and t_lo <= t <= t_hi:
                temps.append(int(f[2]) / 1000)
                freqs.append(int(f[4]) / 1000)
            continue
        if len(f) >= 6:
            cur[f[0]] = (f[1], int(f[3]), int(f[4]) + int(f[5]))
    lines = []
    if temps:
        lines.append(f"    cpu temp {min(temps):.1f}..{max(temps):.1f} C, cpu MHz mean {st.mean(freqs):.0f} "
                     f"min {min(freqs):.0f}")
    ticks = span / 1e9 * 100
    for cpu in sorted(per_cpu):
        busy = per_cpu[cpu].most_common()
        total = sum(v for _, v in busy)
        parts = ", ".join(f"{k} {100 * v / ticks:.1f}%" for k, v in busy[:top] if v > 0)
        lines.append(f"    cpu{cpu} busy {100 * total / ticks:5.1f}%: {parts}")
    return (st.mean(freqs) if freqs else None), "\n".join(lines)


def row(d, skip, count, verbose):
    got = frames(d, skip, count)
    if not got or not got[1]:
        return f"{d.name:26} no frames", None
    start, win, full = got
    work = [(int(r["dur"]) - sum(int(r.get(z) or 0) for z in WAITS)) / 1e6 for r in win]
    wall = [int(r["dur"]) / 1e6 for r in win]
    cpu = [int(r["cpu_ns"] or 0) / 1e6 for r in win]
    t_lo, t_hi = int(win[0]["t"]), int(win[-1]["t"]) + int(win[-1]["dur"])
    secs = (t_hi - t_lo) / 1e9
    inv = sum(int(r["nivcsw"] or 0) for r in win) / secs
    cpus = Counter(r["cpu"] for r in win)
    s = slot_window(d, start, win)
    mhz, detail = samples(d, t_lo, t_hi)
    rep = f"{s['rep']}/{s['new'] + s['rep']}" if "rep" in s else "-"
    line = (f"{d.name:26} {st.mean(work):6.2f} {pct(work, 90):6.2f} {pct(work, 99):6.2f} "
            f"{100 * sum(w > 16.7 for w in work) / len(work):5.1f}% {100 * sum(w > 16.7 for w in wall) / len(wall):5.1f}% "
            f"{st.mean(cpu):6.2f} {rep:>9} {s.get('under', 0):6d} {inv:6.0f} {len(win) / secs:5.1f} "
            f"{mhz if mhz else 0:5.0f}{'' if full else '  SHORT'}")
    more = None
    if verbose:
        more = f"    emu cpus {dict(cpus)}" + (("\n" + detail) if detail else "")
    return line, more


def main():
    args = sys.argv[1:]
    verbose = "-v" in args
    skip, count = 300, 7200
    if "--skip" in args:
        skip = int(args[args.index("--skip") + 1])
    if "--frames" in args:
        count = int(args[args.index("--frames") + 1])
    dirs = [Path(a) for i, a in enumerate(args) if not a.startswith("-") and args[i - 1] not in ("--skip", "--frames")]
    print(f"{'run':26} {'work':>6} {'p90':>6} {'p99':>6} {'>16.7':>6} {'wall>':>6} {'cpu':>6} {'rep':>9} "
          f"{'under':>6} {'inv/s':>6} {'fps':>5} {'MHz':>5}")
    for d in dirs:
        line, more = row(d, skip, count, verbose)
        print(line)
        if more:
            print(more)


if __name__ == "__main__":
    main()
