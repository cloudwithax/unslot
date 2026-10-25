#!/bin/sh
# Copies the cores unslot downloads rather than ships (cores.list) from libretro's buildbot into
# OUT, each as the bare library the device loads, and lists them in OUT/cores.txt by checksum and
# size for the sign_cores example to sign. The cores release is OUT's files.
#
#   mirror.sh OUT
set -eu

OUT="$1"
BUILDBOT="${BUILDBOT:-https://buildbot.libretro.com/nightly/linux/aarch64/latest}"
here="$(cd "$(dirname "$0")" && pwd)"

mkdir -p "$OUT"
list="$OUT/cores.txt"
echo "format=unslot-cores/1" > "$list.new"
grep -v '^#' "$here/cores.list" | while read -r core; do
	[ -n "$core" ] || continue
	so="${core}_libretro.so"
	curl -fsSL --retry 3 -o "$OUT/$so.zip" "$BUILDBOT/$so.zip"
	unzip -q -o "$OUT/$so.zip" "$so" -d "$OUT"
	rm "$OUT/$so.zip"
	# An ELF for aarch64: its machine, at byte 18, is 0xb7.
	[ "$(od -An -tx1 -j18 -N2 "$OUT/$so" | tr -d ' \n')" = "b700" ] ||
		{ echo "$so is not for aarch64" >&2; exit 1; }
	echo "$core $(sha256sum "$OUT/$so" | cut -d' ' -f1) $(wc -c < "$OUT/$so" | tr -d ' ')" >> "$list.new"
done
mv "$list.new" "$list"
echo "$list: $(($(wc -l < "$list") - 1)) cores"
