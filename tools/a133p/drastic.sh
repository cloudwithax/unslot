#!/bin/sh
# Host-only SDL2 build and staging; closed DraStic needs glibc 2.27 (firmware: 2.33).
set -eu
root="$(cd "$(dirname "$0")/../.." && pwd)"
work="$root/target-a133p"
cache="$work/src/drastic-downloads"
sdl_commit=5d7213995373e1efaf2dc59571a561fae7f6be96
drastic_commit=d445a56349cce68f5337d02c9fe890de9d390f3f
mkdir -p "$cache" "$work/tmp" "$work/drastic/libs" "$work/drastic/system"
export TMPDIR="$work/tmp"
mkdir -p "$work/khronos"
for dir in GLES2 GLES3 EGL KHR; do
    cp -R "${KHRONOS_HEADERS:-/usr/include}/$dir" "$work/khronos/"
done
fetch() {
    url="$1" sha="$2" file="$cache/$3"
    if [ ! -f "$file" ]; then
        curl -fL --retry 3 "$url" -o "$file.part"
        mv "$file.part" "$file"
    fi
    echo "$sha  $file" | sha256sum -c - >/dev/null
}
fetch "https://codeload.github.com/Ninoh-FOX/SDL2_trimui/tar.gz/$sdl_commit" 87f0c96445328c7fb465afebae952e084379988c048b11b26559adb1fee5a1ad SDL.tar.gz
src="$work/src/SDL2_trimui-$sdl_commit"
[ -d "$src" ] || tar -xzf "$cache/SDL.tar.gz" -C "$work/src"
patch="$root/tools/a133p/drastic-sdl-display.patch"
git -C "$src" apply --reverse --check "$patch" 2>/dev/null || git -C "$src" apply "$patch"
alsa="$work/src/drastic-alsa"
mkdir -p "$alsa"
fetch https://deb.debian.org/debian/pool/main/a/alsa-lib/libasound2-dev_1.2.4-1.1_arm64.deb 5acd1f3cd1aa11aeeb9dcd2467e8142ced0bdf112b813d134e63baebf342ee29 alsa-dev.deb
fetch https://deb.debian.org/debian/pool/main/a/alsa-lib/libasound2_1.2.4-1.1_arm64.deb 8fabd77256593c42e2ad1578f938b56d0a5ed19aa61c6c72c890fd761fd129a8 alsa.deb
dpkg-deb -x "$cache/alsa-dev.deb" "$alsa"
dpkg-deb -x "$cache/alsa.deb" "$alsa"
build="$src/build-a133p"
mkdir -p "$build"
# ALSA is dlopened on firmware, not bundled. Its Debian probe library needs
# newer libm than the source-build floor; allow its transitive imports only.
(cd "$build" && CC="$root/tools/a133p/zcc" CXX="$root/tools/a133p/zcxx" CFLAGS="-O2 -I$alsa/usr/include -I$work/khronos" LDFLAGS="-L$alsa/usr/lib/aarch64-linux-gnu -Wl,--allow-shlib-undefined" \
    ../configure --host=aarch64-linux-gnu --disable-rpath --disable-static \
    --enable-video-mali --disable-video-x11 --without-x --disable-video-wayland --disable-video-kmsdrm \
    --disable-video-vulkan --disable-video-opengl --enable-video-opengles --enable-video-opengles2 \
    --disable-video-vivante --disable-video-dummy --disable-video-offscreen --disable-video-rpi --disable-video-directfb \
    --enable-alsa --disable-pulseaudio --disable-pipewire --disable-jack --disable-sndio --disable-esd \
    --disable-arts --disable-dbus --disable-ime --disable-ibus --disable-fcitx --disable-libudev \
    --disable-hidapi --disable-joystick-virtual &&
    grep -q '^#define SDL_AUDIO_DRIVER_ALSA 1' include/SDL_config.h &&
    grep -q '^#define SDL_VIDEO_DRIVER_MALI 1' include/SDL_config.h &&
    make -j2 build/libSDL2.la)
out="$work/drastic"
cp "$build/build/.libs/libSDL2-2.0.so.0" "$out/libs/libSDL2-2.0.so.0"
"$root/tools/a133p/zcc" -O2 -Wall -shared -fPIC -I"$src/include" -I"$build/include" \
    -o "$out/libs/libunslot-drastic.so" "$root/tools/drastic/volume.c" -ldl -lpthread
sh "$root/tools/a133p/check-glibc.sh" "$out/libs/libSDL2-2.0.so.0" "$out/libs/libunslot-drastic.so"
raw="https://raw.githubusercontent.com/trngaje/advanced_drastic/$drastic_commit"
while read -r sha from to; do
    fetch "$raw/$from" "$sha" "$(basename "$to")"
    cp "$cache/$(basename "$to")" "$out/$to"
done <<EOF
817308c8b7f64e0b16d0c3ebc245ba37cd93724a64b79c3c6cc52b81da06fc6d drastic_v2520 drastic
6ad5072c3920fd6c8eba2c49d2dd058c296564a11d6013807124a919cf0d66e6 system/drastic_bios_arm7.bin system/drastic_bios_arm7.bin
0a4e88c5dfd20ce669b8df45d009974f23fe7cb9b1f2148dbf30b6445eca9bc5 system/drastic_bios_arm9.bin system/drastic_bios_arm9.bin
799fa38ede70742fa8ca297e295caf3b878d0c7ee34643f1f4499d59b7ee4123 game_database.xml game_database.xml
71a7ed35ac568fa9181ffafdd709d93e42ed39f123d13fca5250482842d36fd4 drastic_readme.txt drastic_readme.txt
EOF
chmod 755 "$out/drastic"
# TrimUI's face-button positions use Xbox codes: B is SDL button 0, A is 1.
# Keep existing Saves/NDS/config untouched; the launcher installs defaults once.
sed -e 's/^controls_b\[CONTROL_INDEX_A\] = 1024$/controls_b[CONTROL_INDEX_A] = 1025/' \
    -e 's/^controls_b\[CONTROL_INDEX_B\] = 1025$/controls_b[CONTROL_INDEX_B] = 1024/' \
    -e 's/^controls_b\[CONTROL_INDEX_UI_SELECT\] = 1024$/controls_b[CONTROL_INDEX_UI_SELECT] = 1025/' \
    -e 's/^controls_b\[CONTROL_INDEX_UI_EXIT\] = 1025$/controls_b[CONTROL_INDEX_UI_EXIT] = 1024/' \
    "$root/card/System/drastic/drastic.cfg" > "$out/drastic.cfg"
cp "$src/LICENSE.txt" "$out/SDL-LICENSE.txt"
{
    echo "drastic_repo=https://github.com/trngaje/advanced_drastic commit=$drastic_commit binary=drastic_v2520"
    echo "drastic_glibc_floor_exception=2.27; closed binary, firmware baseline 2.33"
    echo "sdl_repo=https://github.com/Ninoh-FOX/SDL2_trimui commit=$sdl_commit license=Zlib"
    echo "compiler=tools/a133p/zcc -target aarch64-linux-gnu.2.23 -mcpu=cortex_a53"
    echo "build_flags=build-a133p/config.status --config; jobs=2"
    echo "backend=mali with upstream SDL2_POWERVR_GE8300, null native EGL window; no H700 ION patches"
    echo "patch=tools/a133p/drastic-sdl-display.patch sha256:$(sha256sum "$patch" | cut -d' ' -f1)"
    echo "runtime_gpu=firmware EGL/GLESv2 and transitive PowerVR libraries; not bundled"
    echo "runtime_proof=none; host ELF checks only; controls follow trimui_code_to_btn and SDL classic joystick ordering"
    (cd "$out" && sha256sum drastic libs/*.so* system/* game_database.xml drastic.cfg SDL-LICENSE.txt drastic_readme.txt)
    (cd "$build" && ./config.status --config)
} > "$out/drastic.meta"
