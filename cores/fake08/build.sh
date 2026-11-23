#!/bin/sh
set -eu

# fake-08, the libretro PICO-8 core that plays PICO-8 carts inside slot. Its Lua (z8lua) is a
# git submodule, which GitHub's source archives leave empty, so it comes as a second archive.
#
# CROSS names a toolchain prefix (CROSS=aarch64-linux-gnu-) to cross-compile from x86_64;
# left empty, the build uses the box's own c++, as in the arm64 device box.

here="$(cd "$(dirname "$0")" && pwd)"

device_cflags="-mcpu=cortex-a53"
link_flags="-static-libstdc++ -static-libgcc"

usage() {
	echo "usage: $0 stamp COMMIT Z8LUA_COMMIT | build COMMIT Z8LUA_COMMIT TARBALL Z8LUA_TARBALL WORKDIR OUT" >&2
	exit 2
}

sha256() {
	if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1"; else shasum -a 256 "$1"; fi |
		cut -d' ' -f1
}

stamp() {
	echo "commit=$1"
	echo "source=https://github.com/jtothebell/fake-08/archive/$1.tar.gz"
	echo "z8lua=https://github.com/jtothebell/z8lua/archive/$2.tar.gz"
	echo "recipe=make -C platform/libretro platform=unix"
	echo "device_cflags=$device_cflags"
	echo "link_flags=$link_flags"
	for p in "$here"/*.patch; do
		echo "patch=$(basename "$p") sha256:$(sha256 "$p")"
	done
}

build() {
	commit="$1" z8lua="$2" tarball="$3" z8lua_tarball="$4" work="$5" out="$6"
	src="$work/fake-08-$commit"

	rm -rf "$work"
	mkdir -p "$work"
	tar -xzf "$tarball" -C "$work"
	if [ ! -f "$src/platform/libretro/Makefile" ]; then
		echo "$tarball does not hold fake-08-$commit/platform/libretro/Makefile" >&2
		exit 1
	fi
	tar -xzf "$z8lua_tarball" -C "$work"
	if [ ! -f "$work/z8lua-$z8lua/eris.c" ]; then
		echo "$z8lua_tarball does not hold z8lua-$z8lua/eris.c" >&2
		exit 1
	fi
	rm -rf "$src/libs/z8lua"
	mv "$work/z8lua-$z8lua" "$src/libs/z8lua"

	for p in "$here"/*.patch; do
		patch -p1 --batch --forward -d "$src" <"$p"
	done

	# fake-08 is C++: its runtime goes inside the core, so it needs no libstdc++ of the device's.
	cross="${CROSS:-}"
	CFLAGS="$device_cflags" CXXFLAGS="$device_cflags" LDFLAGS="$link_flags" \
		make -C "$src/platform/libretro" platform=unix \
		CXX="${cross}g++" AR="${cross}ar" \
		-j"$(getconf _NPROCESSORS_ONLN)"
	"${cross}strip" --strip-unneeded "$src/platform/libretro/fake08_libretro.so"

	mkdir -p "$(dirname "$out")"
	cp "$src/platform/libretro/fake08_libretro.so" "$out"
	stamp "$commit" "$z8lua" >"$out.meta"
}

case "${1:-}" in
stamp)
	[ $# -eq 3 ] && [ -n "$2" ] && [ -n "$3" ] || usage
	stamp "$2" "$3"
	;;
build)
	[ $# -eq 7 ] && [ -n "$2" ] && [ -n "$3" ] && [ -n "$4" ] && [ -n "$5" ] && [ -n "$6" ] && [ -n "$7" ] || usage
	build "$2" "$3" "$4" "$5" "$6" "$7"
	;;
*)
	usage
	;;
esac
