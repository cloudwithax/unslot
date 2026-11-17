#!/usr/bin/env python3
"""Summarise pcsample.bin from tools/a133p/pcsample.c.

    pcsample.py RUNDIR [--so LOCAL_NODOSUMI.so] [--top N] [--blocks N]

Each sample's pc is put in a mapping from pcsample.bin.maps. Samples in nodosumi_libretro.so are
named by function with the local copy's symbols; samples in JIT code by the perf-PID.map Nodosumi
writes with NODOSUMI_PERF=1 (block per guest address). For samples in a C function called from JIT
code, lr names the calling block, so helper time is charged to blocks too."""
import argparse
import bisect
import collections
import glob
import os
import struct
import subprocess


def load_maps(path):
    maps = []
    for line in open(path):
        f = line.split()
        lo, hi = (int(x, 16) for x in f[0].split("-"))
        off = int(f[2], 16)
        name = f[5] if len(f) > 5 else ""
        maps.append((lo, hi, off, name))
    maps.sort()
    return maps


def find_map(maps, starts, pc):
    i = bisect.bisect_right(starts, pc) - 1
    if i >= 0 and maps[i][0] <= pc < maps[i][1]:
        return maps[i]
    return None


def load_syms(so):
    out = subprocess.run(["aarch64-linux-gnu-nm", "-C", "--defined-only", "-S", so],
                         capture_output=True, text=True).stdout
    syms = []
    for line in out.splitlines():
        f = line.split(None, 3)
        if len(f) == 4 and f[2] in "tTwW":
            syms.append((int(f[0], 16), int(f[1], 16), f[3]))
    syms.sort()
    return syms


def load_perf(paths):
    blocks = []
    for p in paths:
        for line in open(p):
            f = line.split()
            if len(f) == 3:
                blocks.append((int(f[0], 16), int(f[1], 16), f[2]))
    blocks.sort()
    return blocks


def lookup(table, starts, addr):
    i = bisect.bisect_right(starts, addr) - 1
    if i >= 0 and table[i][0] <= addr < table[i][0] + max(table[i][1], 1):
        return table[i][2]
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("run")
    ap.add_argument("--so", default="target-a133p/cores/nodosumi_libretro.so")
    ap.add_argument("--top", type=int, default=40)
    ap.add_argument("--blocks", type=int, default=40)
    a = ap.parse_args()

    raw = open(os.path.join(a.run, "pcsample.bin"), "rb").read()
    n = len(raw) // 16
    samples = struct.unpack("<%dQ" % (2 * n), raw[: n * 16])
    maps = load_maps(os.path.join(a.run, "pcsample.bin.maps"))
    mstarts = [m[0] for m in maps]
    syms = load_syms(a.so)
    sstarts = [s[0] for s in syms]
    blocks = load_perf(glob.glob(os.path.join(a.run, "perf-*.map")))
    bstarts = [b[0] for b in blocks]

    def name(pc):
        m = find_map(maps, mstarts, pc)
        if m and os.path.basename(m[3]) == os.path.basename(a.so):
            # A shared object's first segment is mapped at file offset 0; an executable's symbols
            # are already absolute.
            base = min(x[0] - x[2] for x in maps if x[3] == m[3]) if a.so.endswith(".so") else 0
            return "C", lookup(syms, sstarts, pc - base) or "nodosumi+%x" % (pc - base)
        b = lookup(blocks, bstarts, pc)
        if b:
            return "JIT", b
        if m:
            return "lib", os.path.basename(m[3]) or "anon"
        return "?", "?"

    by_func = collections.Counter()
    by_kind = collections.Counter()
    by_block = collections.Counter()
    helper_callers = collections.defaultdict(collections.Counter)
    for i in range(n):
        pc, lr = samples[2 * i], samples[2 * i + 1]
        kind, nm = name(pc)
        by_kind[kind] += 1
        by_func[(kind, nm)] += 1
        if kind == "JIT":
            by_block[nm] += 1
        else:
            lk, ln = name(lr)
            if lk == "JIT":
                by_block[ln] += 1
                helper_callers[nm][ln] += 1

    print("%d samples" % n)
    for k, c in by_kind.most_common():
        print("  %-4s %6.2f%%" % (k, 100.0 * c / n))
    print("\n== top functions / code")
    for (k, nm), c in by_func.most_common(a.top):
        print("  %6.2f%%  %-4s %s" % (100.0 * c / n, k, nm))
    print("\n== guest blocks, own time plus C helpers they called (via lr)")
    for b, c in by_block.most_common(a.blocks):
        print("  %6.2f%%  %s" % (100.0 * c / n, b))
    print("\n== callers of the top C helpers")
    for (k, nm), c in by_func.most_common(a.top):
        if k == "JIT" or not helper_callers[nm]:
            continue
        tot = sum(helper_callers[nm].values())
        print("  %s: %.0f%% of its samples called from JIT" % (nm, 100.0 * tot / c))
        for b, cc in helper_callers[nm].most_common(5):
            print("      %6.2f%%  %s" % (100.0 * cc / n, b))


if __name__ == "__main__":
    main()
