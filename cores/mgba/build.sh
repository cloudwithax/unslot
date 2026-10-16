#!/bin/sh
set -eu

here="$(cd "$(dirname "$0")" && pwd)"

device_cflags="-flto=auto"

usage() {
	echo "usage: $0 stamp COMMIT | build COMMIT WORKDIR OUT" >&2
	exit 2
}

sha256() {
	if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1"; else shasum -a 256 "$1"; fi |
		cut -d' ' -f1
}

stamp() {
	echo "commit=$1"
	echo "source=https://github.com/libretro/mgba/tree/$1"
	echo "device_cflags=$device_cflags"
	for p in "$here"/*.patch; do
		echo "patch=$(basename "$p") sha256:$(sha256 "$p")"
	done
}

build() {
	commit="$1" work="$2" out="$3"
	src="$work/mgba"
	obj="$work/build"

	if ! command -v cmake >/dev/null 2>&1; then
		if command -v apt-get >/dev/null 2>&1; then
			sed -i '/bullseye-security/d' /etc/apt/sources.list
			apt-get -o Acquire::Check-Valid-Until=false update -qq
			# Under QEMU, as on an x86 host building for arm64, libc-bin's ldconfig trigger
			# segfaults after cmake itself is in; that is no reason to stop.
			apt-get install -y -qq --no-install-recommends cmake >/dev/null ||
				command -v cmake >/dev/null 2>&1 || exit 1
		else
			echo "building mGBA needs cmake: brew install cmake" >&2
			exit 1
		fi
	fi

	mkdir -p "$src"
	[ -d "$src/.git" ] || git init -q "$src"
	git -C "$src" cat-file -e "$commit^{commit}" 2>/dev/null ||
		git -C "$src" fetch -q --depth 1 https://github.com/libretro/mgba "$commit"
	git -C "$src" checkout -q --force --detach "$commit"
	git -C "$src" clean -q -fdx
	for p in "$here"/*.patch; do
		git -C "$src" apply "$p"
	done

	cflags=""
	if [ "$(uname -s)-$(uname -m)" = "Linux-aarch64" ]; then
		cflags="$device_cflags"
	fi
	cmake -S "$src" -B "$obj" -DLIBMGBA_ONLY=ON -DBUILD_LIBRETRO=ON -DCMAKE_BUILD_TYPE=Release \
		-DCMAKE_C_FLAGS="$cflags" >/dev/null
	cmake --build "$obj" --target mgba_libretro --parallel "$(getconf _NPROCESSORS_ONLN)" >/dev/null

	for ext in dylib so; do
		if [ -f "$obj/mgba_libretro.$ext" ]; then
			mkdir -p "$(dirname "$out")"
			cp "$obj/mgba_libretro.$ext" "$out"
			stamp "$commit" >"$out.meta"
			return
		fi
	done
	echo "cmake finished without producing mgba_libretro" >&2
	exit 1
}

case "${1:-}" in
stamp)
	[ $# -eq 2 ] && [ -n "$2" ] || usage
	stamp "$2"
	;;
build)
	[ $# -eq 4 ] && [ -n "$2" ] && [ -n "$3" ] && [ -n "$4" ] || usage
	build "$2" "$3" "$4"
	;;
*)
	usage
	;;
esac
