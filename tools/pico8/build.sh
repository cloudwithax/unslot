#!/bin/sh
# Builds libunslot-pico8.so, the 32-bit ARM shim `slot --pico8` preloads into the owner's PICO-8,
# or with aarch64 the 64-bit one for pico8_64, see sdl.c.
# usage: build.sh OUT [aarch64]
# Uses $CC, else that ARM's gcc, else zig cc.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
out="$1"
if [ "${2:-}" = aarch64 ]; then
	gcc=aarch64-linux-gnu-gcc target=aarch64-linux-gnu.2.28 arm=64-bit
else
	gcc=arm-linux-gnueabihf-gcc target=arm-linux-gnueabihf.2.28 arm=32-bit
fi
mkdir -p "$(dirname "$out")"
if [ -n "${CC:-}" ]; then
	set -- $CC
elif command -v "$gcc" >/dev/null 2>&1; then
	set -- "$gcc"
elif command -v zig >/dev/null 2>&1; then
	set -- zig cc -target "$target"
else
	echo "build.sh: no $arm ARM compiler, install $gcc or zig" >&2
	exit 1
fi
"$@" -shared -fPIC -O2 -s -Wall -I"$here/../bridge" -o "$out" "$here/sdl.c" -ldl -lpthread
echo "built $out"
