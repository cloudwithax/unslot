#!/bin/sh
# Standalone PPSSPP for the stock A133P firmware; no firmware GPU libraries are shipped.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
work="$root/target-a133p"
out=${1:-"$work/ppsspp"}
case "$out" in /*) ;; *) out="$PWD/$out" ;; esac
state="$work/ppsspp-build"
src="$work/src/ppsspp-standalone"
cache="$work/src/drastic-downloads"
repo=https://github.com/hrydgard/ppsspp.git
commit=fa50bb1976065c4f8b1b47af227d367fe9771555
sdl_commit=5d7213995373e1efaf2dc59571a561fae7f6be96
sdl_url="https://codeload.github.com/Ninoh-FOX/SDL2_trimui/tar.gz/$sdl_commit"
sdl_sha=87f0c96445328c7fb465afebae952e084379988c048b11b26559adb1fee5a1ad
sdl="$work/src/SDL2_trimui-$sdl_commit"
sdl_build="$sdl/build-ppsspp-a133p"
alsa="$work/src/ppsspp-alsa"
gpu=${A133P_GPU_LIBS:-"$work/gpu"}
cc="$root/tools/a133p/zcc"
cxx="$root/tools/a133p/zcxx"
strip=${STRIP:-aarch64-linux-gnu-strip}
jobs=${JOBS:-2}
mkdir -p "$state" "$cache" "$work/tmp" "$work/khronos" "$alsa"
export TMPDIR="$work/tmp"
for tool in cmake make git curl dpkg-deb "$strip"; do command -v "$tool" >/dev/null; done
if [ ! -d "$gpu" ] && [ "$gpu" = "$work/gpu" ]; then
    mkdir -p "$gpu"
    cp "$root/.worktrees/a133p/target-a133p/gpu/"*.so "$gpu/"
fi
for lib in libGLESv2.so libEGL.so; do
    [ -f "$gpu/$lib" ] || { echo "provide firmware $gpu/$lib (link input only)" >&2; exit 1; }
done
for dir in GLES2 GLES3 EGL KHR; do
    if [ -n "${KHRONOS_HEADERS:-}" ] || [ ! -d "$work/khronos/$dir" ]; then
        cp -Rp "${KHRONOS_HEADERS:-/usr/include}/$dir" "$work/khronos/"
    fi
done
fetch() {
    url=$1 sha=$2 file="$cache/$3"
    if [ ! -f "$file" ]; then
        curl -fL --retry 3 "$url" -o "$file.part"
        echo "$sha  $file.part" | sha256sum -c - >/dev/null
        mv "$file.part" "$file"
    fi
    echo "$sha  $file" | sha256sum -c - >/dev/null
}
fetch "$sdl_url" "$sdl_sha" SDL.tar.gz
fetch https://deb.debian.org/debian/pool/main/a/alsa-lib/libasound2-dev_1.2.4-1.1_arm64.deb 5acd1f3cd1aa11aeeb9dcd2467e8142ced0bdf112b813d134e63baebf342ee29 alsa-dev.deb
fetch https://deb.debian.org/debian/pool/main/a/alsa-lib/libasound2_1.2.4-1.1_arm64.deb 8fabd77256593c42e2ad1578f938b56d0a5ed19aa61c6c72c890fd761fd129a8 alsa.deb
[ -d "$sdl" ] || tar -xzf "$cache/SDL.tar.gz" -C "$work/src"
patch="$root/tools/a133p/drastic-sdl-display.patch"
# Track content, not only reverse-check: a patch with a removed hunk still
# reverse-checks against the old superset. Restore pinned inputs on any change.
sdl_patch_sha=$(sha256sum "$patch" | cut -d' ' -f1)
if [ "$(cat "$state/.sdl-patch-sha" 2>/dev/null || true)" != "$sdl_patch_sha" ] ||
    ! git -C "$sdl" apply --reverse --check "$patch" 2>/dev/null; then
    tar -xzf "$cache/SDL.tar.gz" -C "$work/src" \
        "SDL2_trimui-$sdl_commit/src/video/mali-fbdev/SDL_malivideo.c"
    git -C "$sdl" apply "$patch"
    echo "$sdl_patch_sha" > "$state/.sdl-patch-sha"
fi
if [ ! -f "$alsa/.extracted" ]; then
    dpkg-deb -x "$cache/alsa-dev.deb" "$alsa"
    dpkg-deb -x "$cache/alsa.deb" "$alsa"
    touch "$alsa/.extracted"
fi
mkdir -p "$sdl_build"
# Configure only when the recipe/compiler/patch changes. make retains its dependency graph.
config_key=$(cat "$0" "$cc" "$cxx" "$patch" | sha256sum | cut -d' ' -f1)
if [ ! -f "$sdl_build/.recipe" ] || [ "$(cat "$sdl_build/.recipe")" != "$config_key" ]; then
    (cd "$sdl_build" && CC="$cc" CXX="$cxx" CFLAGS="-O2 -I$alsa/usr/include -I$work/khronos" LDFLAGS="-L$alsa/usr/lib/aarch64-linux-gnu -Wl,--allow-shlib-undefined" \
        ../configure --host=aarch64-linux-gnu --disable-rpath --disable-static \
        --enable-video-mali --disable-video-x11 --without-x --disable-video-wayland --disable-video-kmsdrm \
        --disable-video-vulkan --disable-video-opengl --enable-video-opengles --enable-video-opengles2 \
        --disable-video-vivante --disable-video-dummy --disable-video-offscreen --disable-video-rpi --disable-video-directfb \
        --enable-alsa --disable-pulseaudio --disable-pipewire --disable-jack --disable-sndio --disable-esd \
        --disable-arts --disable-dbus --disable-ime --disable-ibus --disable-fcitx --disable-libudev \
        --disable-hidapi --disable-joystick-virtual)
    echo "$config_key" > "$sdl_build/.recipe"
fi
grep -q '^#define SDL_AUDIO_DRIVER_ALSA 1' "$sdl_build/include/SDL_config.h"
grep -q '^#define SDL_VIDEO_DRIVER_MALI 1' "$sdl_build/include/SDL_config.h"
make -C "$sdl_build" -j"$jobs" build/libSDL2.la
sdl_lib="$sdl_build/build/.libs/libSDL2-2.0.so.0"
inc="$state/sdl2-inc"
mkdir -p "$inc/SDL2"
# Preserve timestamps so staging headers does not force a complete C++ rebuild.
cp -p "$sdl/include/"*.h "$inc/"
cp -p "$sdl_build/include/SDL_config.h" "$inc/"
cp -p "$inc/"*.h "$inc/SDL2/"
[ -d "$src/.git" ] || git init -q "$src"
git -C "$src" cat-file -e "$commit^{commit}" 2>/dev/null || git -C "$src" fetch --depth 1 "$repo" "$commit"
if [ "$(git -C "$src" rev-parse HEAD 2>/dev/null || true)" != "$commit" ]; then
    git -C "$src" checkout -q --detach "$commit"
fi
gles_patch="$root/tools/a133p/ppsspp-gles-proc.patch"
gles_patch_sha=$(sha256sum "$gles_patch" | cut -d' ' -f1)
if [ "$(cat "$state/.gles-patch-sha" 2>/dev/null || true)" != "$gles_patch_sha" ] ||
    ! git -C "$src" apply --reverse --check "$gles_patch" 2>/dev/null; then
    git -C "$src" checkout "$commit" -- Common/GPU/OpenGL/gl3stub.c
    git -C "$src" apply "$gles_patch"
    echo "$gles_patch_sha" > "$state/.gles-patch-sha"
fi
git -C "$src" submodule update --init --recursive --depth 1
build="$state/build"
cmake -S "$src" -B "$build" \
    -DCMAKE_TOOLCHAIN_FILE="$root/tools/a133p/toolchain.cmake" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_C_FLAGS="-I$work/khronos -Wno-error=date-time" -DCMAKE_CXX_FLAGS="-I$work/khronos -Wno-error=date-time" \
    -DOPENGL_LIBRARIES="$gpu/libGLESv2.so;$gpu/libEGL.so" \
    -DLIBRETRO=OFF -DUSING_GLES2=ON -DUSING_EGL=OFF \
    -DUSING_X11_VULKAN=OFF -DUSE_WAYLAND_WSI=OFF -DVULKAN=OFF -DARM_NO_VULKAN=ON \
    -DUSE_DISCORD=OFF -DUSE_MINIUPNPC=OFF -DUSE_SYSTEM_FFMPEG=OFF \
    -DUSE_SYSTEM_LIBZIP=OFF -DUSE_SYSTEM_LIBPNG=OFF -DUSE_SYSTEM_ZSTD=OFF \
    -DUSE_SYSTEM_LIBSDL2=ON -DUSE_CCACHE=OFF -DHEADLESS=OFF -DUNITTEST=OFF -DATLAS_TOOL=OFF \
    -DCMAKE_DISABLE_FIND_PACKAGE_X11=ON -DCMAKE_DISABLE_FIND_PACKAGE_Wayland=ON \
    -DCMAKE_DISABLE_FIND_PACKAGE_SDL2_ttf=ON -DCMAKE_DISABLE_FIND_PACKAGE_Fontconfig=ON \
    -DCMAKE_DISABLE_FIND_PACKAGE_ZLIB=ON \
    -DCMAKE_SKIP_BUILD_RPATH=ON \
    -DSDL2_INCLUDE_DIR="$inc" -DSDL2_LIBRARY="$sdl_lib" \
    -DCMAKE_EXE_LINKER_FLAGS="-Wl,-rpath,\$ORIGIN/libs"
# Upstream's prebuilt FFmpeg imports fcntl64 (glibc 2.28). Rebuild its pinned
# source with the same codec selection instead of aliasing the newer symbol.
cmake --build "$build" --target zlib -j"$jobs"
ffbuild="$state/ffmpeg-build"
# This old FFmpeg tests only the sysctl symbol, which glibc 2.23 still exports,
# not its removed header. Require both, like its other header-dependent probes.
cat > "$state/ffmpeg-sysctl.patch" <<'PATCH'
--- a/configure
+++ b/configure
@@ -5308,3 +5308,3 @@
 check_func  sysconf
-check_func  sysctl
+check_func_headers sys/sysctl.h sysctl
 check_func  usleep
PATCH
git -C "$src/ffmpeg" apply --reverse --check "$state/ffmpeg-sysctl.patch" 2>/dev/null ||
    git -C "$src/ffmpeg" apply "$state/ffmpeg-sysctl.patch"
ffprefix="$state/ffmpeg"
mkdir -p "$ffbuild" "$state/ffmpeg-deps"
ln -sf "$build/lib/libzlib.a" "$state/ffmpeg-deps/libz.a"
ffkey=$(cat "$0" "$src/ffmpeg/linux_arm64.sh" "$cc" | sha256sum | cut -d' ' -f1)
if [ ! -f "$ffbuild/.recipe" ] || [ "$(cat "$ffbuild/.recipe")" != "$ffkey" ]; then
    (cd "$ffbuild" && "$src/ffmpeg/configure" --target-os=linux --arch=aarch64 \
        --enable-cross-compile --cc="$cc" --ar=aarch64-linux-gnu-ar --nm=aarch64-linux-gnu-nm \
        --ranlib=aarch64-linux-gnu-ranlib --strip="$strip" --prefix="$ffprefix" \
        --extra-cflags="-O3 -fasm -fno-short-enums -fno-strict-aliasing -I$src/ext/zlib" \
        --extra-ldflags="-L$state/ffmpeg-deps" \
        --disable-shared --enable-static --enable-zlib --enable-pic --disable-everything \
        --disable-avdevice --disable-filters --disable-programs --disable-network --disable-avfilter \
        --disable-postproc --disable-encoders --disable-protocols --disable-hwaccels --disable-doc \
        --enable-decoder=h264,mpeg4,mpeg2video,mjpeg,mjpegb,aac,aac_latm,atrac3,atrac3p,mp3,pcm_s16le,pcm_s8 \
        --enable-encoder=huffyuv,ffv1,mjpeg,pcm_s16le \
        --enable-demuxer=h264,m4v,mpegvideo,mpegps,mp3,avi,aac,pmp,oma,pcm_s16le,pcm_s8,wav \
        --enable-muxer=avi --enable-parser=h264,mpeg4video,mpegaudio,mpegvideo,aac,aac_latm)
    echo "$ffkey" > "$ffbuild/.recipe"
fi
make -C "$ffbuild" -j"$jobs"
# FFmpeg's install rewrites header timestamps even on a no-op build, forcing
# PPSSPP to recompile. Install only when the archives actually change.
install_ffmpeg=0
for lib in avcodec avformat avutil swresample swscale; do
    cmp -s "$ffbuild/lib$lib/lib$lib.a" "$ffprefix/lib/lib$lib.a" || install_ffmpeg=1
done
if [ "$install_ffmpeg" = 1 ] || [ ! -f "$ffprefix/include/libavutil/avconfig.h" ]; then
    make -C "$ffbuild" install
fi
cmake -S "$src" -B "$build" -U 'FFmpeg_*' -DFFMPEG_DIR="$ffprefix"
cmake --build "$build" --target PPSSPPSDL -j"$jobs"
shim="$state/libunslot-ppsspp.so"
shim_key=$({ cat "$root/tools/ppsspp/volume.c" "$cc" "$inc/SDL_config.h"; printf '%s\n' '-Wl,-z,lazy -DUNSLOT_OPAQUE_SWAP'; } | sha256sum | cut -d' ' -f1)
if [ ! -f "$shim" ] || [ "$(cat "$state/.shim-recipe" 2>/dev/null || true)" != "$shim_key" ]; then
    "$cc" -O2 -Wall -Wextra -shared -fPIC -mcpu=cortex_a53 -Wl,-z,lazy -DUNSLOT_OPAQUE_SWAP -I"$inc" -I"$work/khronos" \
        -o "$shim" "$root/tools/ppsspp/volume.c" -L"$gpu" -ldl -lpthread -lGLESv2
    echo "$shim_key" > "$state/.shim-recipe"
fi
mkdir -p "$out/libs"
rm -rf "$out/assets"
cp "$build/PPSSPPSDL" "$out/PPSSPPSDL"
cp "$sdl_lib" "$out/libs/libSDL2-2.0.so.0"
cp "$shim" "$out/libs/libunslot-ppsspp.so"
cp -a "$build/assets" "$out/assets"
cp "$sdl/LICENSE.txt" "$out/SDL-LICENSE.txt"
cp "$src/LICENSE.TXT" "$out/LICENSE.TXT"
"$strip" --strip-unneeded "$out/PPSSPPSDL" "$out/libs/libSDL2-2.0.so.0" "$out/libs/libunslot-ppsspp.so"
chmod 755 "$out/PPSSPPSDL" "$out/libs/"*.so*
sh "$root/tools/a133p/check-glibc.sh" "$out/PPSSPPSDL" "$out/libs/libSDL2-2.0.so.0" "$out/libs/libunslot-ppsspp.so"
{
    echo "repo=$repo commit=$commit version=1.20.4"
    echo "sdl_repo=https://github.com/Ninoh-FOX/SDL2_trimui commit=$sdl_commit"
    echo "sdl_archive=$sdl_url sha256=$sdl_sha"
    echo "compiler=tools/a133p/zcc,zcxx -target aarch64-linux-gnu.2.23 -mcpu=cortex_a53; static libc++; no Docker fallback"
    "$cxx" --version | head -n 1
    echo "toolchain=tools/a133p/toolchain.cmake"
    echo "backend=standalone SDL2 GLES2; patched EGL/dlsym procedure loader; video-mali PowerVR GE8300 null native EGL window; display-mode patch"
    echo "runtime_gpu=firmware EGL/GLESv2 and transitive PowerVR libraries; link inputs only, not bundled"
    echo "gles_patch=tools/a133p/ppsspp-gles-proc.patch sha256=$(sha256sum "$gles_patch" | cut -d' ' -f1)"
    echo "gles_loader=eglGetProcAddress first, then dlsym on a cached explicit GLES dlopen handle; never RTLD_DEFAULT (lld exports PPSSPP's same-named pointer objects)"
    echo "gles_fix_reason=Brick Pro PowerVR EGL returned NULL for all 63 core GLES procedures while libGLESv2 exported them; extension procedures require EGL; unpatched glGetStringi call caused pc=0 SIGSEGV"
    echo "gles_handle_fix_reason=requester traced prior staged build jumping to its exported glGetStringi OBJECT; RTLD_DEFAULT resolved the pointer variable rather than the GLES function"
    echo "gles_fix_evidence=requester verified equivalent preload fallback boots Bomberman (USA).iso and draws its first frame with the H700 binary; staged A133P device proof below"
    echo "runtime_proof=Brick Pro firmware 1.1.1: boots and plays Bomberman (USA) at 30/30 (100%) on the PowerVR, sound on the codec, volume keys and Menu/Exit via slot's supervisor, second launch reads the pad; Smart Pro and Brick untested"
    echo "patch=tools/a133p/drastic-sdl-display.patch sha256=$(sha256sum "$patch" | cut -d' ' -f1)"
    echo "shim=tools/ppsspp/volume.c sha256=$(sha256sum "$root/tools/ppsspp/volume.c" | cut -d' ' -f1) flags=-O2 -shared -fPIC -mcpu=cortex_a53 -Wl,-z,lazy -DUNSLOT_OPAQUE_SWAP -ldl -lpthread -lGLESv2"
    echo "shim_opaque_swap=-DUNSLOT_OPAQUE_SWAP fills the frame's alpha with 1 before each swap: the A133P display layer blends by per-pixel alpha and PPSSPP leaves it at 0, which showed a black panel while the game played"
    echo "shim_binding=lazy PLT resolution lets preloaded SDL interposition resolve SDL imports after executable dependencies load; override lld's eager-binding default"
    echo "sdl_configure:"
    (cd "$sdl_build" && ./config.status --config)
    echo "cmake_flags:"
    echo "h700_delta=zig glibc 2.23 toolchain; TrimUI SDL and Khronos/firmware GPU link inputs; -Wno-error=date-time (libpng17 pngerror.c uses __DATE__/__TIME__ under -Werror)"
    echo "zig_cpu_spelling=cortex_a53; cortex-a53 fails with unknown CPU cortex; FFmpeg inherits the CPU from zcc rather than emitting its own -mcpu"
    echo "ffmpeg=rebuilt pinned submodule with zig; upstream prebuilt libavutil.a imported fcntl64 unavailable at glibc 2.23; codec selection matches ffmpeg/linux_arm64.sh"
    echo "ffmpeg_delta=configure sysctl probe checks sys/sysctl.h as well as symbol (zig omits obsolete header); omit GCC-only -finline-limit=300 and -lgcc; inherit Cortex-A53 from zcc"
    echo "rpath_delta=CMAKE_SKIP_BUILD_RPATH=ON prevents absolute scratch SDL/GPU paths; explicit executable linker flag retains only \$ORIGIN/libs"
    echo "ffmpeg_configure=$(cat "$ffbuild/ffbuild/config.mak" 2>/dev/null | grep '^FFMPEG_CONFIGURATION=' || grep '^FFMPEG_CONFIGURATION=' "$ffbuild/config.mak")"
    grep -E '^(CMAKE_(BUILD_TYPE|C_FLAGS|CXX_FLAGS|EXE_LINKER_FLAGS|TOOLCHAIN_FILE|DISABLE_FIND_PACKAGE_)|LIBRETRO:|USING_|USE_|VULKAN:|ARM_NO_VULKAN:|HEADLESS:|UNITTEST:|ATLAS_TOOL:|SDL2_|OPENGL_LIBRARIES:)' "$build/CMakeCache.txt"
    echo "submodules:"
    git -C "$src" submodule status --recursive
    echo "sha256sums (metadata excluded):"
    (cd "$out" && find . -type f ! -name ppsspp.meta -print | LC_ALL=C sort | while IFS= read -r file; do sha256sum "$file"; done)
} > "$out/ppsspp.meta"
echo "built $out (warm sources, SDL make graph, CMake objects and shim are reusable)"
