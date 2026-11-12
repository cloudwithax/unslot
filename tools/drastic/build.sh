#!/bin/sh
# Builds and fetches what slot runs DS games with into OUT (vendor/drastic):
#
#   drastic            Exophase's DraStic r2.5.2.0 for aarch64, as trngaje's advanced_drastic
#                      keeps it, pinned and checked
#   system/            DraStic's own replacements for the DS BIOS, from the same commit; never
#                      Nintendo's
#   game_database.xml  DraStic's per-game fixes, from the same commit
#   libs/libSDL2-2.0.so.0
#                      SDL 2.30 as trngaje's SDL_drastic has it, with Knulli's H700 patches for
#                      the Mali's fbdev, built against nothing but glibc so it runs on BaseOS
#   libs/libunslot-drastic.so
#                      tools/drastic/volume.c, preloaded into DraStic for slot's volume keys
#   drastic_readme.txt DraStic's readme, for System/licenses
#   SDL-LICENSE.txt    SDL's zlib licence, for System/licenses
#   drastic.manifest   where each came from and its sha256
#
# SDL is compiled here, so this runs on an arm64 Linux box with a C compiler, curl, ar, xz and
# patch: the one device:sh hands it, whose glibc is older than BaseOS's. Its ALSA headers come from Debian bullseye, pinned.
set -eu

OUT="${1:-vendor/drastic}"
here="$(cd "$(dirname "$0")" && pwd)"
DRASTIC_COMMIT=d445a56349cce68f5337d02c9fe890de9d390f3f
SDL_COMMIT=eb2e00f8b7459df90e273208f6cc49427c64267f
SDL_SHA=7c8a9bb462c69c3da8a91b256854da2421ef850e306f34525c8f8404247874fe
KNULLI_COMMIT=7dabcbbfaa93512530e99a130e40ab1e4166987a
ALSA_VERSION=1.2.4-1.1
ALSA_SHA=8fabd77256593c42e2ad1578f938b56d0a5ed19aa61c6c72c890fd761fd129a8
ALSA_DEV_SHA=5acd1f3cd1aa11aeeb9dcd2467e8142ced0bdf112b813d134e63baebf342ee29
CACHE="${DRASTIC_CACHE:-$HOME/.cache/unslot-drastic}"

mkdir -p "$OUT/system" "$OUT/libs" "$CACHE"
OUT="$(cd "$OUT" && pwd)"
CACHE="$(cd "$CACHE" && pwd)"
manifest="$OUT/drastic.manifest.new"
: > "$manifest"

note() {
	echo "$1 $(sha256sum "$2" | cut -d' ' -f1) $3" >> "$manifest"
}

# fetch URL SHA FILE: FILE, downloaded once into the cache and checked.
fetch() {
	if [ ! -f "$CACHE/$3" ]; then
		curl -fsSL --retry 3 -o "$CACHE/$3.part" "$1"
		mv "$CACHE/$3.part" "$CACHE/$3"
	fi
	echo "$2  $CACHE/$3" | sha256sum -c - >/dev/null
}

# DraStic and its files.
raw="https://raw.githubusercontent.com/trngaje/advanced_drastic/$DRASTIC_COMMIT"
while read -r sha from to; do
	fetch "$raw/$from" "$sha" "$(basename "$to")"
	cp "$CACHE/$(basename "$to")" "$OUT/$to"
	note "$to" "$OUT/$to" "$raw/$from"
done <<EOF
817308c8b7f64e0b16d0c3ebc245ba37cd93724a64b79c3c6cc52b81da06fc6d drastic_v2520 drastic
6ad5072c3920fd6c8eba2c49d2dd058c296564a11d6013807124a919cf0d66e6 system/drastic_bios_arm7.bin system/drastic_bios_arm7.bin
0a4e88c5dfd20ce669b8df45d009974f23fe7cb9b1f2148dbf30b6445eca9bc5 system/drastic_bios_arm9.bin system/drastic_bios_arm9.bin
799fa38ede70742fa8ca297e295caf3b878d0c7ee34643f1f4499d59b7ee4123 game_database.xml game_database.xml
71a7ed35ac568fa9181ffafdd709d93e42ed39f123d13fca5250482842d36fd4 drastic_readme.txt drastic_readme.txt
EOF
chmod 755 "$OUT/drastic"

# SDL, with Knulli's patches in order.
url="https://github.com/trngaje/SDL_drastic/archive/$SDL_COMMIT.tar.gz"
fetch "$url" "$SDL_SHA" "SDL_drastic-$SDL_COMMIT.tar.gz"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
tar -xzf "$CACHE/SDL_drastic-$SDL_COMMIT.tar.gz" -C "$tmp"
src="$tmp/SDL_drastic-$SDL_COMMIT"
patches="https://raw.githubusercontent.com/knulli-cfw/knulli-linux/$KNULLI_COMMIT/board/allwinner/h700/patches/sdl2_drastic"
while read -r sha p; do
	fetch "$patches/$p" "$sha" "$p"
	patch -d "$src" -p1 -s < "$CACHE/$p"
done <<EOF
2bbe4e77e781fd7aa1b204aa3ee4b05c0b9f82b8ed269a26a0197b256673f9df 0001-add-video-malifb-driver.patch
f70445c2e1831ccb197dcf0aee0b8f4d7d4c434e46a374d237181cef12f2867b 0002-enable_no_signal_handlers.patch
063dea9c7a2a0278c9634d137301cfac090e9773e5288c1b7801d26d78b2d50e 0002-mali-fbdev-Implemented-MALI-Zero-Copy-fbdev-rotation.patch
d5b642457e850e9fd7b9668f15db41678f01fff66ec74a826b766f9f8dd7601f 0003-No-need-to-set-glClearColor-at-all.patch
cb6157c177faa5364a4193c6dfec735bc4d209fe82cdcf009d31bc7550ec5471 0004-Rework-blitter-thread-sync.patch
068e14e17a0ea489cb04a33f1aafb175b8c3effb051f71d4c37860eb742dc732 0005-additional-mali-fixes.patch
767bf6d2fcc5ba8cbdc9fd2e130483ac458cd2697af3eda97375db21621a9d1c 0006-refactoring.patch
29fd944705bdc6789ede773fc20a7387af2afdf03ffa2b83a7fa444b98ce3e89 0007-no-MSAA.patch
075f694ff4d356d54ce40f16dcae3b389909a535855964cd252e30126968cd3f 0008-add-sRGB.patch
f2b501c92146a5d51b727489622d6e4b2e42d91b6035d63e9b0ef6703675a978 0009-memleak-fix.patch
fab5eef5d01b3212e6518f7c9e946925e5e7b3110aa4208f7d78dd32a942497f 0010-blitter-only-with-rotation.patch
EOF

# ALSA's headers, and the library configure links its check against; SDL itself opens
# BaseOS's libasound when it starts.
alsa="$tmp/alsa"
mkdir -p "$alsa"
pool=http://deb.debian.org/debian/pool/main/a/alsa-lib
for deb in "libasound2_${ALSA_VERSION}_arm64.deb $ALSA_SHA" "libasound2-dev_${ALSA_VERSION}_arm64.deb $ALSA_DEV_SHA"; do
	set -- $deb
	fetch "$pool/$1" "$2" "$1"
	(cd "$tmp" && ar x "$CACHE/$1" data.tar.xz && xz -dc data.tar.xz | tar -x -C "$alsa")
done
ln -sf libasound.so.2 "$alsa/usr/lib/aarch64-linux-gnu/libasound.so"

# Only what DraStic draws, plays and reads its pad with: the Mali's fbdev, ALSA and the
# kernel's joystick nodes.
(cd "$src" && CFLAGS="-O2 -I$alsa/usr/include" LDFLAGS="-L$alsa/usr/lib/aarch64-linux-gnu" \
	./configure -q --disable-rpath --disable-static \
	--enable-video-mali --disable-video-x11 --without-x --disable-video-wayland --disable-video-kmsdrm \
	--disable-video-vulkan --disable-video-opengl --enable-video-opengles --enable-video-opengles1 \
	--enable-video-opengles2 --disable-video-vivante --disable-video-dummy --disable-video-offscreen \
	--disable-video-rpi --disable-video-directfb \
	--enable-alsa --disable-pulseaudio --disable-pipewire --disable-jack --disable-sndio --disable-esd \
	--disable-arts --disable-dbus --disable-ime --disable-ibus --disable-fcitx --disable-libudev \
	--disable-hidapi --disable-joystick-virtual &&
	make -s -j"$(nproc)" >/dev/null)
# A configure that cannot link ALSA leaves it out without a word, and DraStic then crashes.
grep -q '^#define SDL_AUDIO_DRIVER_ALSA 1' "$src/include/SDL_config.h" || { echo "SDL was built without ALSA" >&2; exit 1; }
cp "$src/build/.libs/libSDL2-2.0.so.0" "$OUT/libs/libSDL2-2.0.so.0"
strip "$OUT/libs/libSDL2-2.0.so.0"
note libs/libSDL2-2.0.so.0 "$OUT/libs/libSDL2-2.0.so.0" "https://github.com/trngaje/SDL_drastic/tree/$SDL_COMMIT + $patches"
cp "$src/LICENSE.txt" "$OUT/SDL-LICENSE.txt"

# slot's volume, against the same SDL's headers.
cc -O2 -Wall -shared -fPIC -I"$src/include" -o "$OUT/libs/libunslot-drastic.so" "$here/volume.c" -ldl -lpthread
strip "$OUT/libs/libunslot-drastic.so"
note libs/libunslot-drastic.so "$OUT/libs/libunslot-drastic.so" tools/drastic/volume.c

mv "$manifest" "$OUT/drastic.manifest"
ls -la "$OUT" "$OUT/libs" "$OUT/system"
