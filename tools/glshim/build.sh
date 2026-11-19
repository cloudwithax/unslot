#!/bin/sh
# Builds libGL.so.1 for the handhelds from tools/glshim/libgl.c into the directory given, or
# vendor/glshim. The firmware has GLES 2 only, and cores that link desktop GL are refused by the
# loader without this file.
set -eu
out="${1:-vendor/glshim}"
# Linked against glibc 2.23, as the A133P cards' firmware has 2.33 and dlopen is in libc only from
# 2.34: with zig that is a target; without it the machine's own compiler on arm64, else a cross
# compiler, whose glibc is what the shim needs.
if [ -n "${CC:-}" ]; then
	cc="$CC"
elif command -v zig > /dev/null 2>&1; then
	cc="zig cc -target aarch64-linux-gnu.2.23"
elif python3 -m ziglang version > /dev/null 2>&1; then
	cc="python3 -m ziglang cc -target aarch64-linux-gnu.2.23"
elif [ "$(uname -m)" = aarch64 ] || [ "$(uname -m)" = arm64 ]; then
	cc=cc
else
	cc=aarch64-linux-gnu-gcc
fi
mkdir -p "$out"
$cc -O2 -shared -fPIC -Wl,-soname,libGL.so.1 -o "$out/libGL.so.1" "$(dirname "$0")/libgl.c" -ldl
file "$out/libGL.so.1" | grep -q aarch64
echo "built $out/libGL.so.1"
