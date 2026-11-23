#!/bin/sh
# Cross-compile standalone PPSSPPSDL + preload volume/pause shim into vendor/ppsspp.
# Runs cleanly inside the `unslot-gles` Docker image (or natively on arm64).
set -eu

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT=$(CDPATH= cd -- "$HERE/../.." && pwd)

OUT=${1:-"$ROOT/vendor/ppsspp"}
case "$OUT" in
    /*) ;;
    *) OUT="$PWD/$OUT" ;;
esac

PPSSPP_REPO="https://github.com/hrydgard/ppsspp.git"
# v1.20.4 (SDL2 standalone backend; 5f2d4afbdced51ac056af25a05fd182a1175b558 migrated SDL/ to SDL3).
PPSSPP_COMMIT="fa50bb1976065c4f8b1b47af227d367fe9771555"

SDL_COMMIT="eb2e00f8b7459df90e273208f6cc49427c64267f"
SDL_URL="https://github.com/trngaje/SDL_drastic/archive/${SDL_COMMIT}.tar.gz"
SDL_SHA="7c8a9bb462c69c3da8a91b256854da2421ef850e306f34525c8f8404247874fe"
SDL_SRC_DESC="https://github.com/trngaje/SDL_drastic/tree/${SDL_COMMIT} + https://raw.githubusercontent.com/knulli-cfw/knulli-linux/7dabcbbfaa93512530e99a130e40ab1e4166987a/board/allwinner/h700/patches/sdl2_drastic"

SRC="$ROOT/target-device/ppsspp/src"
BUILD="$ROOT/target-device/ppsspp/build"
DR_SDL="$ROOT/vendor/drastic/libs/libSDL2-2.0.so.0"
CACHE="${DRASTIC_CACHE:-$ROOT/target-device/drastic-cache}"

arch=$(uname -m)
case "$arch" in
    aarch64|arm64) cross="" ;;
    *) cross="aarch64-linux-gnu-" ;;
esac

for tool in cmake "${cross}gcc" "${cross}g++" "${cross}strip"; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "missing required tool: $tool (run inside unslot-gles container)" >&2
        exit 1
    }
done

[ -f "$DR_SDL" ] || {
    echo "missing $DR_SDL (run 'task drastic' first to build SDL_drastic)" >&2
    exit 1
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

# Allow git commands inside bind-mounted checkouts when running as root in Docker.
if [ -f /.dockerenv ] && command -v git >/dev/null 2>&1; then
    git config --global --add safe.directory "$SRC" 2>/dev/null || true
    git config --global --add safe.directory '*' 2>/dev/null || true
fi

if [ -d "$SRC/.git" ]; then
    cur=$(git -C "$SRC" rev-parse HEAD 2>/dev/null || true)
    if [ "$cur" != "$PPSSPP_COMMIT" ]; then
        git -C "$SRC" fetch origin "$PPSSPP_COMMIT" || git -C "$SRC" fetch origin
        git -C "$SRC" checkout --force "$PPSSPP_COMMIT"
        git -C "$SRC" submodule update --init --recursive --depth 1
    elif git -C "$SRC" submodule status 2>/dev/null | grep -q '^-'; then
        git -C "$SRC" submodule update --init --recursive --depth 1
    fi
else
    mkdir -p "$(dirname "$SRC")"
    git clone "$PPSSPP_REPO" "$SRC"
    git -C "$SRC" checkout --force "$PPSSPP_COMMIT"
    git -C "$SRC" submodule update --init --recursive --depth 1
fi

sdl_tgz=""
for dir in "$CACHE" "$HOME/.cache/unslot-drastic"; do
    if [ -f "$dir/SDL_drastic-${SDL_COMMIT}.tar.gz" ]; then
        sdl_tgz="$dir/SDL_drastic-${SDL_COMMIT}.tar.gz"
        break
    fi
done
if [ -z "$sdl_tgz" ]; then
    mkdir -p "$CACHE"
    sdl_tgz="$CACHE/SDL_drastic-${SDL_COMMIT}.tar.gz"
    curl -fsSL -o "$sdl_tgz" "$SDL_URL"
fi

got_sha=$(sha256sum "$sdl_tgz" | awk '{print $1}')
if [ "$got_sha" != "$SDL_SHA" ]; then
    echo "SHA-256 mismatch for $sdl_tgz: expected $SDL_SHA, got $got_sha" >&2
    exit 1
fi

mkdir -p "$tmp/sdl-src"
tar -xzf "$sdl_tgz" -C "$tmp/sdl-src"
sdl_src="$tmp/sdl-src/SDL_drastic-${SDL_COMMIT}"
[ -d "$sdl_src/include" ] || sdl_src=$(find "$tmp/sdl-src" -maxdepth 1 -mindepth 1 -type d | head -n 1)

STATE="$ROOT/target-device/ppsspp"
mkdir -p "$STATE"

tc_flag=""
if [ -n "$cross" ]; then
    cat >"$STATE/toolchain.cmake" <<'EOF'
set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR aarch64)
set(CMAKE_C_COMPILER aarch64-linux-gnu-gcc)
set(CMAKE_CXX_COMPILER aarch64-linux-gnu-g++)
set(CMAKE_FIND_ROOT_PATH /usr/aarch64-linux-gnu)
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_C_FLAGS_INIT "-mcpu=cortex-a53")
set(CMAKE_CXX_FLAGS_INIT "-mcpu=cortex-a53")
EOF
    tc_flag="-DCMAKE_TOOLCHAIN_FILE=$STATE/toolchain.cmake"
fi

# Configure SDL_drastic once to generate Release SDL_config.h matching libSDL2-2.0.so.0.
cmake -S "$sdl_src" -B "$tmp/sdl-cfg" \
    $tc_flag \
    -DCMAKE_BUILD_TYPE=Release \
    -DSDL_SHARED=ON \
    -DSDL_STATIC=OFF \
    -DSDL_TEST=OFF \
    -DSDL_X11=OFF \
    -DSDL_WAYLAND=OFF \
    -DSDL_OPENGLES=ON \
    -DSDL_PULSEAUDIO=OFF \
    -DSDL_PIPEWIRE=OFF \
    -DSDL_JACK=OFF \
    -DSDL_SNDIO=OFF \
    -DSDL_ALSA=OFF \
    -DSDL_OSS=OFF \
    -DSDL_ARTS=OFF \
    -DSDL_ESD=OFF \
    -DSDL_NAS=OFF \
    -DSDL_DISKAUDIO=OFF

SDL_INC="$STATE/sdl2-inc"
rm -rf "$SDL_INC"
mkdir -p "$SDL_INC/SDL2"
cp -f "$sdl_src/include/"*.h "$SDL_INC/"
cp -f "$tmp/sdl-cfg/include/SDL2/SDL_revision.h" "$SDL_INC/"
cp -f "$tmp/sdl-cfg/include-config-release/SDL2/SDL_config.h" "$SDL_INC/"
cp -f "$SDL_INC/"*.h "$SDL_INC/SDL2/"

# Clear stale CMake cache if switching toolchain/header locations across runs.
if [ -f "$BUILD/CMakeCache.txt" ] && grep -q '/tmp/tmp\.' "$BUILD/CMakeCache.txt" "$BUILD/CMakeFiles/"*/CMakeSystem.cmake 2>/dev/null; then
    rm -rf "$BUILD/CMakeCache.txt" "$BUILD/CMakeFiles"
fi
mkdir -p "$BUILD"
cmake -S "$SRC" -B "$BUILD" \
    $tc_flag \
    -DCMAKE_BUILD_TYPE=Release \
    -DLIBRETRO=OFF \
    -DUSING_GLES2=ON \
    -DUSING_EGL=OFF \
    -DUSING_X11_VULKAN=OFF \
    -DUSE_WAYLAND_WSI=OFF \
    -DVULKAN=OFF \
    -DARM_NO_VULKAN=ON \
    -DUSE_DISCORD=OFF \
    -DUSE_MINIUPNPC=OFF \
    -DUSE_SYSTEM_FFMPEG=OFF \
    -DUSE_SYSTEM_LIBZIP=OFF \
    -DUSE_SYSTEM_LIBPNG=OFF \
    -DUSE_SYSTEM_ZSTD=OFF \
    -DUSE_SYSTEM_LIBSDL2=ON \
    -DUSE_CCACHE=OFF \
    -DHEADLESS=OFF \
    -DUNITTEST=OFF \
    -DATLAS_TOOL=OFF \
    -DCMAKE_DISABLE_FIND_PACKAGE_X11=ON \
    -DCMAKE_DISABLE_FIND_PACKAGE_Wayland=ON \
    -DCMAKE_DISABLE_FIND_PACKAGE_SDL2_ttf=ON \
    -DCMAKE_DISABLE_FIND_PACKAGE_Fontconfig=ON \
    -DCMAKE_DISABLE_FIND_PACKAGE_ZLIB=ON \
    -DSDL2_INCLUDE_DIR="$SDL_INC" \
    -DSDL2_LIBRARY="$DR_SDL" \
    -DCMAKE_EXE_LINKER_FLAGS="-Wl,-rpath,\$ORIGIN/libs"

jobs=$(nproc 2>/dev/null || echo 4)
cmake --build "$BUILD" --target PPSSPPSDL -j"$jobs"

"${cross}gcc" -O2 -Wall -Wextra -shared -fPIC -mcpu=cortex-a53 \
    -I"$SDL_INC" -I"$HERE/../bridge" \
    -o "$tmp/libunslot-ppsspp.so" \
    "$HERE/volume.c" \
    -ldl -lpthread -lGLESv2

rm -rf "$OUT/assets" "$OUT/libs"
mkdir -p "$OUT/libs"

cp -f "$BUILD/PPSSPPSDL" "$OUT/PPSSPPSDL"
cp -f "$DR_SDL" "$OUT/libs/libSDL2-2.0.so.0"
cp -f "$tmp/libunslot-ppsspp.so" "$OUT/libs/libunslot-ppsspp.so"
cp -a "$BUILD/assets" "$OUT/assets"
cp -f "$sdl_src/LICENSE.txt" "$OUT/SDL-LICENSE.txt"
[ -f "$SRC/LICENSE.TXT" ] && cp -f "$SRC/LICENSE.TXT" "$OUT/LICENSE.TXT" || true

"${cross}strip" --strip-unneeded "$OUT/PPSSPPSDL" "$OUT/libs/libunslot-ppsspp.so"
chmod 755 "$OUT/PPSSPPSDL" "$OUT/libs/libSDL2-2.0.so.0" "$OUT/libs/libunslot-ppsspp.so"

sha() { sha256sum "$1" | awk '{print $1}'; }

{
    printf 'PPSSPPSDL %s %s@%s\n' "$(sha "$OUT/PPSSPPSDL")" "$PPSSPP_REPO" "$PPSSPP_COMMIT"
    printf 'libs/libSDL2-2.0.so.0 %s %s\n' "$(sha "$OUT/libs/libSDL2-2.0.so.0")" "$SDL_SRC_DESC"
    printf 'libs/libunslot-ppsspp.so %s tools/ppsspp/volume.c\n' "$(sha "$OUT/libs/libunslot-ppsspp.so")"
    printf 'SDL-LICENSE.txt %s %s/LICENSE.txt\n' "$(sha "$OUT/SDL-LICENSE.txt")" "$SDL_URL"
    [ -f "$OUT/LICENSE.TXT" ] && printf 'LICENSE.TXT %s %s@%s\n' "$(sha "$OUT/LICENSE.TXT")" "$PPSSPP_REPO" "$PPSSPP_COMMIT" || true
} >"$OUT/ppsspp.manifest"

echo "built $OUT cleanly"
