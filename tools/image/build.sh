#!/bin/sh
# Builds a card image to flash to TF1: a BaseOS release for one model, with unslot already on its
# BASEOS partition (p7), so the device boots straight into unslot.
#
#   build.sh TARGET TREE OUT.img.zip      TARGET as BaseOS names models (rg34xxsp), TREE from
#                                         task dist:device
#
# BaseOS ships p7 small and, on first boot, grows it to fill the card and formats it afresh,
# which would wipe anything an image put there. So one script in the image's rootfs changes:
# expand-storage holds p7's files in RAM across that format and puts them back. Everything else
# is BaseOS's release as published, and its own gptgrow grows p7 here, so its partition layout,
# which its A/B updates rely on, is what it wrote. Needs curl, unzip, zip, cc, debugfs (e2fsprogs),
# mkfs.vfat (dosfstools) and mcopy (mtools); no root.
set -eu

TARGET="$1"
TREE="$2"
OUT="$3"
BASEOS_VERSION="${BASEOS_VERSION:-1.4.0}"
CACHE="${IMAGE_CACHE:-$HOME/.cache/unslot-image}"
RELEASE="https://github.com/pvaibhav/BaseOS/releases/download/v$BASEOS_VERSION"
SECTOR=512

[ -f "$TREE/System/slot" ] || { echo "$TREE has no System/slot" >&2; exit 1; }
mkdir -p "$CACHE"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT HUP INT TERM

# BaseOS's release, checked against its own checksums.
zip="baseos-$TARGET-$BASEOS_VERSION.img.zip"
curl -fsSL -o "$CACHE/SHA256SUMS-$BASEOS_VERSION" "$RELEASE/SHA256SUMS"
[ -f "$CACHE/$zip" ] || curl -fsSL -o "$CACHE/$zip" "$RELEASE/$zip"
(cd "$CACHE" && grep " $zip\$" "SHA256SUMS-$BASEOS_VERSION" | sha256sum -c - > /dev/null)
unzip -q -o "$CACHE/$zip" -d "$work"
img="$work/$(cd "$work" && ls ./*.img | head -n 1)"

# Where each partition is, from the GPT: start and end sectors by number.
part() {
	dd if="$img" bs=$SECTOR skip=2 count=2 status=none | od -An -v -tu8 -w128 |
		awk -v n="$1" 'NR == n { print $5, $6 }'
}
set -- $(part 5)
ROOT_START="$1" ROOT_END="$2"
set -- $(part 7)
DATA_START="$1"

# The rootfs: expand-storage keeps p7's files across its format.
dd if="$img" of="$work/rootfs" bs=$SECTOR skip="$ROOT_START" count=$((ROOT_END - ROOT_START + 1)) status=none
debugfs -R "dump /usr/sbin/expand-storage $work/expand-storage" "$work/rootfs" 2> /dev/null
for anchor in '^# Fresh empty FAT32 across' '^if mount -t vfat -o rw "\$CARD" "\$dst"' '^log "storage expanded'; do
	grep -q "$anchor" "$work/expand-storage" ||
		{ echo "BaseOS $BASEOS_VERSION's expand-storage has changed; check the patch" >&2; exit 1; }
done
awk '
/^# Fresh empty FAT32 across/ {
	print "# unslot: this card came with unslot on p7. Hold its files in RAM across the format and"
	print "# put them back after; if they will not all fit, leave p7 at its first size instead."
	print "keep=/tmp/.cardkeep"
	print "rm -rf \"$keep\" && mkdir -p \"$keep\" /tmp/.cardold"
	print "if mount -t vfat -o ro,utf8,shortname=mixed \"$CARD\" /tmp/.cardold 2>/dev/null; then"
	print "\tif ! cp -R /tmp/.cardold/. \"$keep\"/ >> \"$LOG\" 2>&1; then"
	print "\t\tumount /tmp/.cardold"
	print "\t\trm -rf \"$keep\""
	print "\t\tlog \"expand: the card'"'"'s files would not fit in RAM; leaving p7 at its first size\""
	print "\t\texit 0"
	print "\tfi"
	print "\tumount /tmp/.cardold"
	print "fi"
	print ""
}
{ print }
/^if mount -t vfat -o rw "\$CARD" "\$dst"/ {
	print "\tcp -R \"$keep\"/. \"$dst\"/ >> \"$LOG\" 2>&1 || log \"expand: putting the card'"'"'s files back failed\""
}
/^log "storage expanded/ { print "rm -rf \"$keep\"" }
' "$work/expand-storage" > "$work/expand-storage.new"
debugfs -w -f - "$work/rootfs" > /dev/null 2>&1 <<EOF
rm /usr/sbin/expand-storage
write $work/expand-storage.new /usr/sbin/expand-storage
set_inode_field /usr/sbin/expand-storage mode 0100755
set_inode_field /usr/sbin/expand-storage uid 0
set_inode_field /usr/sbin/expand-storage gid 0
EOF
debugfs -R "dump /usr/sbin/expand-storage $work/check" "$work/rootfs" 2> /dev/null
cmp -s "$work/check" "$work/expand-storage.new" || { echo "the rootfs did not take the patch" >&2; exit 1; }
debugfs -R "stat /usr/sbin/expand-storage" "$work/rootfs" 2> /dev/null | grep -q 'Mode:  0755' ||
	{ echo "expand-storage lost its mode" >&2; exit 1; }
dd if="$work/rootfs" of="$img" bs=$SECTOR seek="$ROOT_START" conv=notrunc status=none
debugfs -R "dump /usr/share/baseos/card-readme.txt $work/README.txt" "$work/rootfs" 2> /dev/null
debugfs -R "dump /usr/share/baseos/baseos.conf $work/baseos.conf" "$work/rootfs" 2> /dev/null

# p7 big enough for the tree with room to spare, grown by BaseOS's own gptgrow.
tree_mib=$(du -sm "$TREE" | cut -f1)
data_mib=$(((tree_mib + 128 + 255) / 256 * 256))
[ "$data_mib" -ge 512 ] || data_mib=512
curl -fsSL -o "$work/gptgrow.c" "https://raw.githubusercontent.com/pvaibhav/BaseOS/v$BASEOS_VERSION/tools/gptgrow.c"
cc -O2 -o "$work/gptgrow" "$work/gptgrow.c"
truncate -s $(((DATA_START + data_mib * 2048 + 4) * SECTOR)) "$img"
"$work/gptgrow" "$img" > /dev/null 2>&1 || true
set -- $(part 7)
[ "$1" = "$DATA_START" ] || { echo "p7 moved" >&2; exit 1; }
data_sectors=$(($2 - $1 + 1))
[ "$data_sectors" -ge $((data_mib * 2048)) ] || { echo "gptgrow did not grow p7" >&2; exit 1; }

# A fresh FAT32 named as BaseOS names it, with unslot and BaseOS's own notes on it.
mkfs.vfat -F 32 -n BASEOS -C "$work/data" $((data_sectors / 2)) > /dev/null
(cd "$TREE" && for f in * .[!.]*; do
	[ -e "$f" ] || continue
	mcopy -s -Q -i "$work/data" "$f" ::/
done)
mcopy -Q -i "$work/data" "$work/README.txt" "$work/baseos.conf" ::/
dd if="$work/data" of="$img" bs=$SECTOR seek="$DATA_START" conv=notrunc status=none

# BaseOS's gptgrow sees a laid-out card, as it will on a card the image's size.
if "$work/gptgrow" "$img" > /dev/null 2>&1; then
	echo "gptgrow would grow the finished image again" >&2
	exit 1
fi

mkdir -p "$(dirname "$OUT")"
name="$(basename "$OUT" .zip)"
mv "$img" "$work/$name"
rm -f "$OUT"
(cd "$work" && zip -q -9 "$(cd "$(dirname "$OUT")" && pwd)/$(basename "$OUT")" "$name")
echo "$OUT: BaseOS $BASEOS_VERSION for $TARGET with unslot $(cat "$TREE/System/version") on a ${data_mib} MiB BASEOS partition"
