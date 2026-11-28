#!/bin/sh
# Rebuilds the A133P DraStic and PPSSPP bridge shims from the current checkout's
# tools/{drastic,ppsspp}/volume.c and tools/bridge/bridge.h against glibc 2.23, so a release
# always ships the checkout's shims even when a133p-cores is reused.
set -eu

root="$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)"
work="$root/target-a133p"
cache="$work/src/drastic-downloads"
sdl_commit="5d7213995373e1efaf2dc59571a561fae7f6be96"
sdl_url="https://codeload.github.com/Ninoh-FOX/SDL2_trimui/tar.gz/$sdl_commit"
sdl_sha="87f0c96445328c7fb465afebae952e084379988c048b11b26559adb1fee5a1ad"
sdl="$work/src/SDL2_trimui-$sdl_commit"
cc="$root/tools/a133p/zcc"
strip="${STRIP:-aarch64-linux-gnu-strip}"

mkdir -p "$cache" "$work/src" "$work/khronos" "$work/gpu"

if [ ! -d "$sdl/include" ]; then
    tarball="$cache/SDL.tar.gz"
    if [ ! -f "$tarball" ]; then
        curl -fL --retry 3 "$sdl_url" -o "$tarball.part"
        echo "$sdl_sha  $tarball.part" | sha256sum -c - >/dev/null
        mv "$tarball.part" "$tarball"
    fi
    echo "$sdl_sha  $tarball" | sha256sum -c - >/dev/null
    tar -xzf "$tarball" -C "$work/src" "SDL2_trimui-$sdl_commit/include"
fi

if [ ! -f "$work/gpu/libGLESv2.so" ]; then
    gles_deb="$cache/libgles2_1.3.2-1_arm64.deb"
    gles_sha="ae3fda1556b677519ff13d43295d7df678d3a9c39a21042e87d9255de1bc78b6"
    if [ ! -f "$gles_deb" ]; then
        curl -fL --retry 3 \
            "https://deb.debian.org/debian/pool/main/libg/libglvnd/libgles2_1.3.2-1_arm64.deb" \
            -o "$gles_deb.part"
        echo "$gles_sha  $gles_deb.part" | sha256sum -c - >/dev/null
        mv "$gles_deb.part" "$gles_deb"
    fi
    tmp="$work/tmp/shims-gles"
    rm -rf "$tmp" && mkdir -p "$tmp"
    dpkg-deb -x "$gles_deb" "$tmp"
    cp -L "$tmp/usr/lib/aarch64-linux-gnu/libGLESv2.so.2" "$work/gpu/libGLESv2.so"
fi

for dir in GLES2 KHR; do
    if [ ! -d "$work/khronos/$dir" ]; then
        cp -Rp "${KHRONOS_HEADERS:-/usr/include}/$dir" "$work/khronos/"
    fi
done

if [ -d "$work/ppsspp/libs" ]; then
    out="$work/ppsspp/libs/libunslot-ppsspp.so"
    "$cc" -O2 -Wall -Wextra -shared -fPIC -mcpu=cortex_a53 -Wl,-z,lazy \
        -I"$sdl/include" -I"$work/khronos" -I"$root/tools/bridge" \
        -o "$out" "$root/tools/ppsspp/volume.c" -L"$work/gpu" -ldl -lpthread -lGLESv2
    "$strip" --strip-unneeded "$out"
    chmod 755 "$out"
    sh "$root/tools/a133p/check-glibc.sh" "$out"
    echo "built $out"
fi

if [ -d "$work/drastic/libs" ]; then
    out="$work/drastic/libs/libunslot-drastic.so"
    "$cc" -O2 -Wall -Wextra -shared -fPIC -mcpu=cortex_a53 -Wl,-z,lazy \
        -I"$sdl/include" -I"$root/tools/bridge" \
        -o "$out" "$root/tools/drastic/volume.c" -ldl -lpthread
    "$strip" --strip-unneeded "$out"
    chmod 755 "$out"
    sh "$root/tools/a133p/check-glibc.sh" "$out"
    echo "built $out"
fi
