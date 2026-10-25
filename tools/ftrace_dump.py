#!/usr/bin/env python3
"""Decode a SLOT_FTRACE trace: slot-ftrace.bin plus its .names and .threads files.

  ftrace_dump.py TRACE                 summary: spans, counters, per-frame intervals
  ftrace_dump.py TRACE --dump [N]      print records (first N) as t_ms thread phase event arg
  ftrace_dump.py TRACE --dump --thread slot-emu --event emu. --from 5 --to 6

Records are 24 bytes little-endian: u64 t_ns (CLOCK_MONOTONIC), u32 tid, u16 event, u8 phase
(0 instant, 1 begin, 2 end, 3 counter), u8 reserved, i64 arg. Several processes may append to
the one file; tid is global and the timeline is shared, so the records are sorted by time.
"""
import argparse
import os
import struct
import sys
from collections import defaultdict

REC = struct.Struct("<QIHBBq")
PHASE = "ibec"  # instant, begin, end, counter


def load(path):
    names = {}
    try:
        for line in open(path + ".names"):
            i, _, n = line.rstrip("\n").partition(" ")
            if n:
                names[int(i)] = n
    except OSError:
        pass
    threads = {}
    try:
        for line in open(path + ".threads"):
            p = line.rstrip("\n").split(" ", 2)
            if len(p) == 3:
                threads[int(p[0])] = (int(p[1]), p[2])
    except OSError:
        pass
    data = open(path, "rb").read()
    n = len(data) // REC.size
    if len(data) % REC.size:
        print(f"note: {len(data) % REC.size} trailing bytes ignored", file=sys.stderr)
    recs = list(REC.iter_unpack(data[: n * REC.size]))
    recs.sort(key=lambda r: r[0])
    return recs, names, threads


def tname(threads, tid):
    p = threads.get(tid)
    if not p:
        return f"tid{tid}"
    comm = p[1]
    return f"{comm}/{tid}"


def pct(xs, q):
    return xs[min(len(xs) - 1, int(len(xs) * q))]


def stats(label, xs, unit=1e6, suffix="ms", out=print):
    if not xs:
        return
    xs = sorted(xs)
    out(
        f"  {label:<28} n={len(xs):<6} mean={sum(xs)/len(xs)/unit:8.3f} p50={pct(xs,.5)/unit:8.3f} "
        f"p95={pct(xs,.95)/unit:8.3f} p99={pct(xs,.99)/unit:8.3f} max={xs[-1]/unit:8.3f} {suffix}"
    )


def dump(recs, names, threads, a):
    t0 = recs[0][0]
    shown = 0
    for t, tid, ev, ph, _, arg in recs:
        ms = (t - t0) / 1e6
        if a.t_from is not None and ms < a.t_from:
            continue
        if a.t_to is not None and ms > a.t_to:
            break
        n = names.get(ev, str(ev))
        tn = tname(threads, tid)
        if a.event and a.event not in n:
            continue
        if a.thread and a.thread not in tn:
            continue
        print(f"{ms:12.3f} {tn:<22} {PHASE[ph]} {n:<24} {arg}")
        shown += 1
        if a.dump and a.dump > 0 and shown >= a.dump:
            break


def pair_spans(recs, names):
    """Per (tid, event) stack of begins; yields (ev, tid, t_begin, t_end, begin_arg, end_arg)."""
    open_ = defaultdict(list)
    for t, tid, ev, ph, _, arg in recs:
        if ph == 1:
            open_[(tid, ev)].append((t, arg))
        elif ph == 2 and open_[(tid, ev)]:
            b, barg = open_[(tid, ev)].pop()
            yield ev, tid, b, t, barg, arg


def summary(recs, names, threads):
    inv = {v: k for k, v in names.items()}
    t0, t1 = recs[0][0], recs[-1][0]
    print(f"{len(recs)} records, {(t1 - t0) / 1e9:.2f} s, {len(set(r[1] for r in recs))} threads")
    per = defaultdict(int)
    for r in recs:
        per[r[1]] += 1
    for tid, n in sorted(per.items(), key=lambda kv: -kv[1]):
        print(f"  {tname(threads, tid):<24} {n:>8} records ({n / ((t1 - t0) / 1e9):.0f}/s)")

    spans = defaultdict(list)
    by_ev_pairs = list(pair_spans(recs, names))
    for ev, tid, b, e, _, _ in by_ev_pairs:
        spans[ev].append(e - b)
    print("\nSpans (duration):")
    for ev in sorted(spans):
        stats(names.get(ev, str(ev)), spans[ev])

    ctr = defaultdict(list)
    for t, tid, ev, ph, _, arg in recs:
        if ph == 3:
            ctr[ev].append(arg)
    print("\nCounters (value):")
    for ev in sorted(ctr):
        xs = sorted(ctr[ev])
        print(
            f"  {names.get(ev, str(ev)):<28} n={len(xs):<6} min={xs[0]} p50={pct(xs,.5)} p95={pct(xs,.95)} max={xs[-1]}"
        )

    inst = defaultdict(int)
    for r in recs:
        if r[3] == 0:
            inst[r[2]] += 1
    print("\nInstants (count):")
    for ev in sorted(inst):
        print(f"  {names.get(ev, str(ev)):<28} {inst[ev]}")

    def times(name, ph):
        e = inv.get(name)
        return [r[0] for r in recs if r[2] == e and r[3] == ph]

    print("\nPer-frame intervals:")
    swap_end = times("present.swap", 2)
    stats("present period (swap end)", [b - a for a, b in zip(swap_end, swap_end[1:])])
    ticks = times("tick", 0)
    stats("tick interval", [b - a for a, b in zip(ticks, ticks[1:])])
    for n in ("emu.run_frame", "emu.run_overlapped", "emu.run_linked"):
        ts = times(n, 1)
        if len(ts) > 1:
            stats(f"{n} start interval", [b - a for a, b in zip(ts, ts[1:])])
    pub = [(r[0], r[5]) for r in recs if r[2] == inv.get("emu.publish") and r[3] == 2]
    stats("publish interval", [b[0] - a[0] for a, b in zip(pub, pub[1:])])
    pick = {r[5]: r[0] for r in recs if r[2] == inv.get("render.frame")}
    lat = [pick[s] - t for t, s in pub if s in pick]
    stats("publish -> render.frame", lat)
    if pub:
        lost = [s for _, s in pub if s not in pick]
        print(f"  published {len(pub)}, picked up {len(pick)}, never shown (overwritten) {len(lost)}")
    rep = times("render.repeat", 0)
    print(f"  render.repeat (picture drawn again) {len(rep)}, of {len(swap_end)} presents")
    fps = [r[5] / 1000 for r in recs if r[2] == inv.get("ui.fps_milli")]
    if fps:
        print(f"  on-screen FPS values: n={len(fps)} min={min(fps):.1f} max={max(fps):.1f} mean={sum(fps)/len(fps):.2f}")
    for n in ("audio.xrun", "ring.underrun", "ring.overrun", "present.missed", "emu.deadline_miss"):
        if inv.get(n) is not None:
            c = sum(1 for r in recs if r[2] == inv[n])
            print(f"  {n}: {c}")

    # Frames whose present period ran long, with what the other threads were doing.
    long_ = [(b, b - a) for a, b in zip(swap_end, swap_end[1:]) if b - a > 20_000_000]
    print(f"\nPresent periods over 20 ms: {len(long_)}")
    for b, d in long_[:10]:
        print(f"  at {(b - t0) / 1e6:10.3f} ms period {d / 1e6:.2f} ms")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("trace")
    ap.add_argument("--dump", nargs="?", const=-1, type=int, metavar="N", help="print records instead of the summary")
    ap.add_argument("--thread", help="substring of the thread name")
    ap.add_argument("--event", help="substring of the event name")
    ap.add_argument("--from", dest="t_from", type=float, help="ms from the first record")
    ap.add_argument("--to", dest="t_to", type=float, help="ms from the first record")
    a = ap.parse_args()
    recs, names, threads = load(a.trace)
    if not recs:
        sys.exit("no records")
    if a.dump is not None:
        try:
            dump(recs, names, threads, a)
        except BrokenPipeError:
            pass
    else:
        summary(recs, names, threads)


if __name__ == "__main__":
    main()
