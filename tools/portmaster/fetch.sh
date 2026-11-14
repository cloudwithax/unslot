#!/bin/sh
# Fetches what PortMaster's ports need to run from unslot into OUT (vendor/portmaster), each
# pinned and checked:
#
#   PortMaster/        PortMaster's runtime from its release, the parts a port's script uses:
#                      device_info.txt, funcs.txt and PortMasterDialog.txt, which control.txt
#                      sources; gptokeyb and gptokeyb2, which turn the pad into keys;
#                      libinterpose, which gptokeyb2 preloads; portmsg, which shows a port's
#                      messages; SDL's controller database; PortMaster's version; the tools
#                      ports unpack and patch their games with (7-Zip, innoextract, xdelta3,
#                      astcenc, sdl_resolution, tasksetter, libgl_default.txt and
#                      libtheoradec); utils/, its patcher and splash; and runtimes/love_11.5,
#                      the LÖVE they run on. All for arm64 alone. Not its GUI, which needs
#                      Python, nor the runtimes it downloads, which slot fetches when a port
#                      wants one (crates/slot/src/port_runtime.rs).
#   bin/bash           Ubuntu 22.04's static bash, which every port's script is written for;
#                      BaseOS has only BusyBox's ash
#   bin/zip            Ubuntu 22.04's zip, which some ports repack their game files with
#   PortMaster/libs.aarch64/
#                      Ubuntu 22.04's OpenAL, sndio, mpg123, Vorbis and Ogg, which LÖVE and
#                      many ports load and BaseOS has not got
#   licenses/          PortMaster's MIT licence and those of what ships from it, and Ubuntu's
#                      copyright files for what comes from it
#   portmaster.manifest
#                      where each came from and its sha256
#
# Nothing is compiled: this runs on any host with curl, unzip, ar, tar and zstd.
set -eu

OUT="${1:-vendor/portmaster}"
PM_TAG=2026.11.12-1059
PM_SHA=631adf14de9f08cd9974da23eb50dc6ca9ad836e89d0ebefe5079c0e567f6346
PM_LICENSE_SHA=ad472b77840ebffc06dae38a37ae6f65aea14401920cc9e168782bfd11d4f62b
BASH_VERSION=5.1-6ubuntu1.1
BASH_SHA=1164dc7d8c1d1dffef0d23ac319fd7e1ee50ffa789eca27dc85016cc892994d4
ZIP_VERSION=3.0-12build2
ZIP_SHA=2718626223216b4c00e28c7206ee8bf6c65d29bd4752039ab8a482275c2a84e6
CACHE="${PORTMASTER_CACHE:-$HOME/.cache/unslot-portmaster}"

rm -rf "$OUT"
mkdir -p "$OUT/PortMaster" "$OUT/bin" "$OUT/licenses" "$CACHE"
OUT="$(cd "$OUT" && pwd)"
CACHE="$(cd "$CACHE" && pwd)"
manifest="$OUT/portmaster.manifest.new"
: > "$manifest"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

note() {
	echo "$1 $(sha256sum "$2" | cut -d' ' -f1) $3" >> "$manifest"
}

# fetch URL SHA FILE: FILE, downloaded once into the cache and checked.
fetch() {
	if [ ! -f "$CACHE/$3" ] || [ "$(sha256sum "$CACHE/$3" | cut -d' ' -f1)" != "$2" ]; then
		curl -fsSL -o "$CACHE/$3.part" "$1"
		mv "$CACHE/$3.part" "$CACHE/$3"
	fi
	[ "$(sha256sum "$CACHE/$3" | cut -d' ' -f1)" = "$2" ] || { echo "$3 is not what was pinned" >&2; exit 1; }
}

# PortMaster's runtime, out of its release.
release="https://github.com/PortsMaster/PortMaster-GUI/releases/download/$PM_TAG"
fetch "$release/PortMaster.zip" "$PM_SHA" "PortMaster-$PM_TAG.zip"
unzip -q "$CACHE/PortMaster-$PM_TAG.zip" -d "$tmp"
pm="$tmp/PortMaster"
for f in device_info.txt funcs.txt PortMasterDialog.txt gamecontrollerdb.txt version \
	gptokeyb gptokeyb2 libinterpose.aarch64.so portmsg.aarch64 \
	7zzs.aarch64 innoextract.aarch64 xdelta3 astcenc.aarch64 \
	sdl_resolution.aarch64 tasksetter libgl_default.txt libs.aarch64/libtheoradec.so.1 \
	runtimes/love_11.5/love.txt runtimes/love_11.5/love.aarch64 \
	runtimes/love_11.5/libs.aarch64/liblove-11.5.so runtimes/love_11.5/libs.aarch64/libluajit-5.1.so.2 \
	runtimes/love_11.5/libs.aarch64/libmodplug.so.1 runtimes/love_11.5/libs.aarch64/libogg.so.0; do
	mkdir -p "$(dirname "$OUT/PortMaster/$f")"
	cp "$pm/$f" "$OUT/PortMaster/$f"
	note "PortMaster/$f" "$OUT/PortMaster/$f" "$release/PortMaster.zip"
done
cp -R "$pm/utils" "$OUT/PortMaster/utils"
(cd "$OUT" && find PortMaster/utils -type f | LC_ALL=C sort) | while read -r f; do
	note "$f" "$OUT/$f" "$release/PortMaster.zip"
done
for f in gptokeyb gptokeyb2 portmsg.aarch64 7zzs.aarch64 innoextract.aarch64 xdelta3 \
	astcenc.aarch64 sdl_resolution.aarch64 runtimes/love_11.5/love.aarch64; do
	chmod 755 "$OUT/PortMaster/$f"
	file "$OUT/PortMaster/$f" | grep -q aarch64
done
for f in libinterpose.aarch64.so libs.aarch64/libtheoradec.so.1 runtimes/love_11.5/libs.aarch64/liblove-11.5.so; do
	file "$OUT/PortMaster/$f" | grep -q aarch64
done
# gptokeyb and gptokeyb2 are GPL-2.0, built by PortMaster from these; portmsg is MIT, with its
# font under the OFL.
echo "source PortMaster/gptokeyb https://github.com/PortsMaster/gptokeyb" >> "$manifest"
echo "source PortMaster/gptokeyb2 https://github.com/PortsMaster/gptokeyb2" >> "$manifest"
for f in gptokeyb gptokeyb2 portmsg portmsg-sans 7zzs innoextract xdelta3 astcenc libtheora \
	love luajit libmodplug libogg peaberry-otf aeromatics; do
	cp "$pm/licenses/LICENSE-$f.txt" "$OUT/licenses/portmaster-$f-LICENSE.txt"
done
fetch "https://raw.githubusercontent.com/PortsMaster/PortMaster-GUI/$PM_TAG/LICENSE" "$PM_LICENSE_SHA" "PortMaster-$PM_TAG-LICENSE"
cp "$CACHE/PortMaster-$PM_TAG-LICENSE" "$OUT/licenses/portmaster-MIT.txt"

# deb NAME VERSION SHA POOL: the package's files under $tmp/NAME, from Ubuntu 22.04 for arm64,
# whose glibc is BaseOS's.
deb() {
	file="$1_$2_arm64.deb"
	fetch "http://ports.ubuntu.com/ubuntu-ports/$4/$file" "$3" "$file"
	mkdir -p "$tmp/$1"
	(cd "$tmp/$1" && ar x "$CACHE/$file" data.tar.zst && tar --zstd -xf data.tar.zst)
}

deb bash-static "$BASH_VERSION" "$BASH_SHA" pool/universe/b/bash
cp "$tmp/bash-static/bin/bash-static" "$OUT/bin/bash"
note bin/bash "$OUT/bin/bash" "https://launchpad.net/ubuntu/+source/bash/$BASH_VERSION"
cp "$tmp/bash-static/usr/share/doc/bash-static/copyright" "$OUT/licenses/bash-copyright.txt"

deb zip "$ZIP_VERSION" "$ZIP_SHA" pool/main/z/zip
cp "$tmp/zip/usr/bin/zip" "$OUT/bin/zip"
note bin/zip "$OUT/bin/zip" "https://launchpad.net/ubuntu/+source/zip/$ZIP_VERSION"
cp "$tmp/zip/usr/share/doc/zip/copyright" "$OUT/licenses/zip-copyright.txt"
chmod 755 "$OUT/bin/bash" "$OUT/bin/zip"
file "$OUT/bin/bash" | grep -q 'aarch64.*statically linked'
file "$OUT/bin/zip" | grep -q aarch64

# What BaseOS has not got that LÖVE, and so PortMaster's patcher, and many ports load: OpenAL,
# with the sndio it is built against, mpg123 and Vorbis, from Ubuntu 22.04 like bash, into
# PortMaster/libs.aarch64, which funcs.txt puts after a port's own libraries.
while read -r name version sha pool libs; do
	deb "$name" "$version" "$sha" "$pool"
	for l in $libs; do
		cp -L "$(find "$tmp/$name" -name "$l" | head -n 1)" "$OUT/PortMaster/libs.aarch64/$l"
		file "$OUT/PortMaster/libs.aarch64/$l" | grep -q aarch64
		note "PortMaster/libs.aarch64/$l" "$OUT/PortMaster/libs.aarch64/$l" \
			"https://launchpad.net/ubuntu/+source/$(basename "$pool")/$version"
	done
	cp "$tmp/$name/usr/share/doc/$name/copyright" "$OUT/licenses/$name-copyright.txt"
done <<EOF
libopenal1 1.19.1-2build3 e43d76e5ada49abd01ec2384732bfe6f15a1dd1adef8605c15c3cb0387e2aa75 pool/universe/o/openal-soft libopenal.so.1
libsndio7.0 1.8.1-1.1 b0387bad84af336cf4b447114d204c8b1b6e20d1cd2af6b645dad5bb5e42f786 pool/universe/s/sndio libsndio.so.7
libbsd0 0.11.5-1 19703bfe6fde9a161e42f5107451a46cc182a62dd0621a55d06aec7dc372991d pool/main/libb/libbsd libbsd.so.0
libmd0 1.0.4-1build1 6f87f19f71411a94ae370f7f9bcf4c7dfb5d4393f11de99634ac3137448ad678 pool/main/libm/libmd libmd.so.0
libmpg123-0 1.29.3-1ubuntu0.1 4a9a2ee385035bffe3c8accc974cc5272a402227d00196e8c7a2e9946ae90ae4 pool/main/m/mpg123 libmpg123.so.0
libvorbis0a 1.3.7-1build2 bf1b1c79b8953e077c19fc1015972b2f8e16521e41d4596e2f7e3d91811ea4d0 pool/main/libv/libvorbis libvorbis.so.0
libvorbisfile3 1.3.7-1build2 56cebf5da54aee5e0de0a5b638b9af78bb2bdca97defaff4053bb30326054182 pool/main/libv/libvorbis libvorbisfile.so.3
libogg0 1.3.5-0ubuntu3 6072fff3bdc02037b2cc4dd5ee421bdb5a656eb2f70563e940fb0d967ee70332 pool/main/libo/libogg libogg.so.0
EOF

mv "$manifest" "$OUT/portmaster.manifest"
echo "vendored PortMaster $PM_TAG, bash $BASH_VERSION and zip $ZIP_VERSION into $OUT"
