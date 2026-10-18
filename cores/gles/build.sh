#!/bin/sh
# Builds the cores that draw with the GPU for the H700's Mali, against OpenGL ES. The buildbot's
# aarch64 builds of these are made for desktop GL, which BaseOS does not have.
#
#   build.sh CORE COMMIT WORK OUT        CORE is parallel_n64, flycast, flycast_xtreme, ppsspp or vecx
#
# Runs on an aarch64 Debian bullseye box, as the release does, or cross-compiles from an amd64 one
# (cores/gles/Dockerfile); bullseye's glibc is older than the device's, so what it builds loads
# there. Each core's source is fetched at COMMIT, with its submodules, into WORK.
set -eu

core="$1" commit="$2" out="$4"
# Absolute, as CMake reads the toolchain file beside it from its own build folder.
mkdir -p "$(dirname "$3")"
work="$(cd "$(dirname "$3")" && pwd)/$(basename "$3")"
jobs="$(nproc)"
cpu="-mcpu=cortex-a53"
case "$(uname -m)" in
aarch64) cross="" ;;
*) cross="aarch64-linux-gnu-" ;;
esac

fetch() {
	repo="$1"
	source="https://github.com/$repo/tree/$commit"
	rm -rf "$work"
	git clone --quiet --filter=blob:none "https://github.com/$repo" "$work"
	git -C "$work" checkout --quiet "$commit"
	git -C "$work" submodule --quiet update --init --recursive --depth 1
}

toolchain="$work.toolchain.cmake"
cmake_cross() {
	if [ -z "$cross" ]; then
		echo "-DCMAKE_BUILD_TYPE=Release -DCMAKE_C_FLAGS=$cpu -DCMAKE_CXX_FLAGS=$cpu"
		return
	fi
	cat >"$toolchain" <<EOF
set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR aarch64)
set(CMAKE_C_COMPILER aarch64-linux-gnu-gcc)
set(CMAKE_CXX_COMPILER aarch64-linux-gnu-g++)
set(CMAKE_FIND_ROOT_PATH /usr/aarch64-linux-gnu)
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_C_FLAGS_INIT "$cpu")
set(CMAKE_CXX_FLAGS_INIT "$cpu")
EOF
	echo "-DCMAKE_TOOLCHAIN_FILE=$toolchain -DCMAKE_BUILD_TYPE=Release"
}

case "$core" in
parallel_n64)
	fetch libretro/parallel-n64
	make -C "$work" -j"$jobs" platform=unix-gles ARCH=aarch64 WITH_DYNAREC=aarch64 \
		CC=${cross}gcc CXX=${cross}g++ GL_LIB=-lGLESv2 \
		CPUFLAGS="-DARM -DDONT_WANT_ARM_OPTIMIZATIONS -DARM64 $cpu"
	built="$work/parallel_n64_libretro.so"
	;;
flycast)
	fetch flyinghead/flycast
	# shellcheck disable=SC2046
	cmake -S "$work" -B "$work/build" $(cmake_cross) -DLIBRETRO=ON -DUSE_GLES=ON \
		-DUSE_VULKAN=OFF -DUSE_OPENMP=OFF
	cmake --build "$work/build" -j"$jobs"
	built="$work/build/flycast_libretro.so"
	;;
flycast_xtreme)
	# KMFDManic's Flycast Xtreme, which muOS ships: libretro's Flycast of 2021 tuned for ARM.
	fetch KMFDManic/morpheuscast_xtreme
	# Its GLdouble is a float, which bullseye's gl2ext.h has already declared a double.
	sed -i 's/^typedef GLfloat GLdouble;/typedef double GLdouble;/' \
		"$work/core/libretro-common/include/glsm/glsm.h"
	# CPUFLAGS from the environment, which the arm64 platform's own add to.
	CPUFLAGS="$cpu" make -C "$work" -j"$jobs" platform=arm64 ARCH=arm64 HAVE_OPENMP=0 \
		CC=${cross}gcc CXX=${cross}g++ CC_AS=${cross}gcc
	built="$work/km_flycast_xtreme_libretro.so"
	;;
ppsspp)
	fetch hrydgard/ppsspp
	# shellcheck disable=SC2046
	cmake -S "$work" -B "$work/build" $(cmake_cross) -DLIBRETRO=ON -DUSING_GLES2=ON \
		-DUSING_EGL=OFF -DUSING_X11_VULKAN=OFF -DVULKAN=OFF -DUSE_DISCORD=OFF \
		-DUSE_MINIUPNPC=OFF -DUSE_SYSTEM_FFMPEG=OFF -DUSE_SYSTEM_LIBZIP=OFF
	cmake --build "$work/build" -j"$jobs" --target ppsspp_libretro
	built="$(find "$work/build" -name ppsspp_libretro.so | head -n 1)"
	# Its fonts, atlas and shaders, which it reads from the BIOS folder's PPSSPP.
	rm -rf "$(dirname "$out")/PPSSPP"
	mkdir -p "$(dirname "$out")"
	cp -r "$work/assets" "$(dirname "$out")/PPSSPP"
	;;
vecx)
	# Not a GPU core here: its buildbot build links desktop GL, so it is built to draw in
	# software, which a Vectrex's lines need no GPU for. Its Makefile sets its own flags.
	fetch libretro/libretro-vecx
	make -C "$work" -j"$jobs" HAS_GPU=0 CC=${cross}gcc CXX=${cross}g++
	built="$work/vecx_libretro.so"
	;;
*)
	echo "build.sh: no recipe for $core" >&2
	exit 2
	;;
esac

"${cross}strip" --strip-unneeded "$built"
mkdir -p "$(dirname "$out")"
cp "$built" "$out"
{
	echo "core=$core"
	echo "commit=$commit"
	echo "source=$source"
	echo "gles=yes"
} >"$out.meta"
echo "built $out from $core @ $commit"
