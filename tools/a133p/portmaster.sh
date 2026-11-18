#!/bin/sh
# Puts in OUT/System the builds of PortMaster's zip and OpenAL that TrimUI's firmware can load.
# The H700 tree takes them from Ubuntu 22.04, which needs glibc 2.34; the firmware has 2.33, so
# these come from Ubuntu 20.04 instead, with the sndio that build of OpenAL was made against.
#
#   portmaster.sh OUT
set -eu

OUT="$1"
SYS="$OUT/System"
POOL=http://ports.ubuntu.com/ubuntu-ports/pool
[ -d "$SYS" ] || { echo "$OUT has no System" >&2; exit 1; }

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

# deb NAME VERSION SHA256 POOL-DIR: the package, checked, unpacked into $tmp/NAME.
deb() {
	file="$1_$2_arm64.deb"
	curl -fsSL -o "$tmp/$file" "$POOL/$4/$file"
	echo "$3  $tmp/$file" | sha256sum -c - > /dev/null
	mkdir -p "$tmp/$1"
	dpkg-deb -x "$tmp/$file" "$tmp/$1"
}

deb zip 3.0-11build1 de3fb99c7564bc7c3d9f31259756305b914796d5dffb491892bacb085f3e8dbc main/z/zip
deb libopenal1 1.19.1-1 8d3018e6752cecb94cf284f8ce236ae62d591f5f1ccafe967bd29020d3794619 universe/o/openal-soft
deb libsndio7.0 1.5.0-3 19906f9fe8f30e205e7f0554cb0f7aec928eb4411cb78419866aad01e636d6f1 universe/s/sndio

lib=usr/lib/aarch64-linux-gnu
cp "$tmp/zip/usr/bin/zip" "$SYS/bin/zip"
chmod 755 "$SYS/bin/zip"
cp -L "$tmp/libopenal1/$lib/libopenal.so.1" "$SYS/PortMaster/libs.aarch64/libopenal.so.1"
cp "$tmp/libsndio7.0/$lib/libsndio.so.7.0" "$SYS/PortMaster/libs.aarch64/libsndio.so.7.0"

# The newest glibc each one asks for has to be one the firmware has.
for f in "$SYS/bin/zip" "$SYS/PortMaster/libs.aarch64/libopenal.so.1" \
	"$SYS/PortMaster/libs.aarch64/libsndio.so.7.0"; do
	v="$(objdump -T "$f" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -n 1)"
	[ "$(printf '%s\n2.33\n' "$v" | sort -V | tail -n 1)" = 2.33 ] ||
		{ echo "$f needs glibc $v, newer than the firmware's 2.33" >&2; exit 1; }
done

mkdir -p "$SYS/licenses"
{
	echo "bin/zip zip 3.0-11build1 https://launchpad.net/ubuntu/+source/zip/3.0-11build1"
	echo "PortMaster/libs.aarch64/libopenal.so.1 openal-soft 1.19.1-1 https://launchpad.net/ubuntu/+source/openal-soft/1:1.19.1-1"
	echo "PortMaster/libs.aarch64/libsndio.so.7.0 sndio 1.5.0-3 https://launchpad.net/ubuntu/+source/sndio/1.5.0-3"
} > "$SYS/licenses/portmaster-a133p.manifest"
echo "PortMaster's zip and OpenAL in $SYS are the A133P's"
