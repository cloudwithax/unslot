#!/bin/sh
# Builds libslotcast.so, what unslot casts to a TV with: x264 and Opus to encode, mbedTLS to talk
# to the TV, each from its pinned commit, static, under slotcast.c's few calls.
#
#   build.sh X264_COMMIT OPUS_COMMIT MBEDTLS_COMMIT WORK OUT
#
# Runs on an aarch64 Debian bullseye box, as the release does, or cross-compiles from an amd64
# one in cores/gles/Dockerfile, as the GPU cores do. On an amd64 box with HOST=1 it builds for
# that box instead, for casting from slot on a desktop.
set -eu

x264_commit="$1" opus_commit="$2" mbedtls_commit="$3" out="$5"
mkdir -p "$4"
work="$(cd "$4" && pwd)"
here="$(cd "$(dirname "$0")" && pwd)"
jobs="$(nproc)"
case "$(uname -m)-${HOST:-0}" in
aarch64-*) cross="" cpu="-mcpu=cortex-a53" ;;
*-1) cross="" cpu="" ;;
*) cross="aarch64-linux-gnu-" cpu="-mcpu=cortex-a53" ;;
esac

fetch() {
	rm -rf "$work/$1"
	git clone --quiet --filter=blob:none "$2" "$work/$1"
	git -C "$work/$1" checkout --quiet "$3"
}

fetch x264 https://code.videolan.org/videolan/x264.git "$x264_commit"
host=""
[ -n "$cross" ] && host="--host=aarch64-linux --cross-prefix=$cross"
# shellcheck disable=SC2086
(cd "$work/x264" && ./configure --prefix="$work/prefix" --enable-static --enable-pic \
	--disable-cli --disable-opencl --bit-depth=8 --chroma-format=420 $host \
	--extra-cflags="$cpu")
make -C "$work/x264" -j"$jobs"
make -C "$work/x264" install-lib-static

fetch opus https://github.com/xiph/opus.git "$opus_commit"
toolchain=""
if [ -n "$cross" ]; then
	toolchain="$work/toolchain.cmake"
	cat >"$toolchain" <<EOF
set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR aarch64)
set(CMAKE_C_COMPILER aarch64-linux-gnu-gcc)
set(CMAKE_FIND_ROOT_PATH /usr/aarch64-linux-gnu)
EOF
	toolchain="-DCMAKE_TOOLCHAIN_FILE=$toolchain"
fi
# shellcheck disable=SC2086
cmake -S "$work/opus" -B "$work/opus/build" $toolchain -DCMAKE_BUILD_TYPE=Release \
	-DCMAKE_C_FLAGS="$cpu" -DCMAKE_POSITION_INDEPENDENT_CODE=ON -DOPUS_BUILD_SHARED_LIBRARY=OFF \
	-DOPUS_BUILD_TESTING=OFF -DOPUS_BUILD_PROGRAMS=OFF -DCMAKE_INSTALL_PREFIX="$work/prefix"
cmake --build "$work/opus/build" -j"$jobs"
cmake --install "$work/opus/build" >/dev/null

fetch mbedtls https://github.com/Mbed-TLS/mbedtls.git "$mbedtls_commit"
git -C "$work/mbedtls" submodule update --quiet --init --depth 1 framework
# shellcheck disable=SC2086
cmake -S "$work/mbedtls" -B "$work/mbedtls/build" $toolchain -DCMAKE_BUILD_TYPE=Release \
	-DCMAKE_C_FLAGS="$cpu" -DCMAKE_POSITION_INDEPENDENT_CODE=ON -DENABLE_PROGRAMS=OFF \
	-DENABLE_TESTING=OFF -DCMAKE_INSTALL_PREFIX="$work/prefix"
cmake --build "$work/mbedtls/build" -j"$jobs"
cmake --install "$work/mbedtls/build" >/dev/null

# Only slotcast.c's calls are seen from outside.
"${cross}gcc" -shared -fPIC -O2 $cpu -fvisibility=hidden -o "$work/libslotcast.so" \
	"$here/slotcast.c" -I"$work/prefix/include" -I"$work/prefix/include/opus" \
	"$work/prefix/lib/libx264.a" "$work/prefix/lib/libopus.a" "$work/prefix/lib/libmbedtls.a" \
	"$work/prefix/lib/libmbedx509.a" "$work/prefix/lib/libmbedcrypto.a" \
	-Wl,--exclude-libs,ALL -Wl,-z,defs -lm -lpthread -ldl
"${cross}strip" --strip-unneeded "$work/libslotcast.so"
mkdir -p "$(dirname "$out")"
cp "$work/libslotcast.so" "$out"
{
	echo "x264=$x264_commit"
	echo "x264_source=https://code.videolan.org/videolan/x264/-/tree/$x264_commit"
	echo "opus=$opus_commit"
	echo "opus_source=https://github.com/xiph/opus/tree/$opus_commit"
	echo "mbedtls=$mbedtls_commit"
	echo "mbedtls_source=https://github.com/Mbed-TLS/mbedtls/tree/$mbedtls_commit"
} >"$out.meta"
