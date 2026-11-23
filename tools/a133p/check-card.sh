#!/bin/sh
# Checks an A133P card tree from `task dist:a133p` before it ships: check-card.sh OUT VERSION.
# Run by both the release and the release-a133p workflows.
set -eu
out="$1"
version="$2"
here="$(dirname "$0")"

file "$out/System/slot" | grep -q aarch64
test "$(cat "$out/System/version")" = "$version"
sh "$here/check-glibc.sh" "$out/System/slot"
# Everything a game runs on has to load on the firmware's glibc 2.33 and the libstdc++
# of GCC 10 it has.
newest() { objdump -T "$1" | grep -o "$2_[0-9.]*" | sed "s/$2_//" | sort -uV | tail -n 1; }
fits() { [ -z "$1" ] || [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -n 1)" = "$2" ]; }
for f in "$out"/System/*_libretro.so "$out"/System/bin/zip "$out"/System/drastic/drastic \
  "$out"/System/drastic/libs/*.so* "$out"/System/lib/*.so* \
  "$out"/System/ppsspp/PPSSPPSDL "$out"/System/ppsspp/libs/*.so*; do
  test -f "$f"
  fits "$(newest "$f" GLIBC)" 2.33 || { echo "$f needs a newer glibc than 2.33" >&2; exit 1; }
  fits "$(newest "$f" GLIBCXX)" 3.4.28 || { echo "$f needs a newer libstdc++" >&2; exit 1; }
done
test "$(ls "$out"/System/*_libretro.so | wc -l)" -ge 47
test -x "$out/.tmp_update/updater"
test -f "$out/System/a133p/launch.sh"
test -s "$out/System/licenses/drastic.meta"
test -s "$out/System/licenses/portmaster-a133p.manifest"
# PSP runs in the A133P's own PPSSPPSDL, beside its launcher and its own PowerVR SDL2.
for f in PPSSPPSDL libs/libunslot-ppsspp.so libs/libSDL2-2.0.so.0; do
  file "$out/System/ppsspp/$f" | grep -q aarch64
done
test -f "$out/System/ppsspp/launch.sh"
test -f "$out/System/ppsspp/assets/ppge_atlas.zim"
test -s "$out/System/licenses/ppsspp.meta"
# slot drives DraStic and PPSSPP over tools/bridge/bridge.h; shims from an a133p-cores
# built before it leave each emulator drawing over slot's last frame, out of its reach.
for f in drastic/libs/libunslot-drastic.so ppsspp/libs/libunslot-ppsspp.so; do
  grep -q UNSLOT_BRIDGE "$out/System/$f" ||
    { echo "$f has no bridge; run the a133p-cores workflow" >&2; exit 1; }
done
