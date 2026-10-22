#!/bin/sh
# Builds a card image to flash to TF1: a BaseOS release for one model, with unslot already on its
# BASEOS partition (p7), so the device boots straight into unslot.
#
#   build.sh TARGET TREE OUT.img.zip      TARGET as BaseOS names models (rg34xxsp), TREE from
#                                         task dist:device
#
# BaseOS ships p7 small and, on first boot, grows it to fill the card and formats it FAT32 afresh,
# which would wipe anything an image put there. An image's p7 is ext4 instead, so a reset mid-write
# cannot corrupt it, and one script in the image's rootfs changes: expand-storage grows that ext4
# in place with resize2fs, which BaseOS lacks, so the image brings e2fsprogs' own, built for the
# device. BaseOS mounts p7 as ext4 by its own fallback once FAT and exFAT do not take. Everything
# else is BaseOS's release as published, and its own gptgrow grows p7 here, so its partition
# layout, which its A/B updates rely on, is what it wrote. Updates and slot cards are not
# touched: only a card flashed from an image is ext4. Needs curl, unzip, zip, cc, make,
# aarch64-linux-gnu-gcc, debugfs and mke2fs (e2fsprogs) and fakeroot; no root.
set -eu
# debugfs and mke2fs live in sbin, which Debian leaves off a user's PATH.
PATH="$PATH:/usr/sbin:/sbin"

TARGET="$1"
TREE="$2"
OUT="$3"
BASEOS_VERSION="${BASEOS_VERSION:-1.4.0}"
CACHE="${IMAGE_CACHE:-$HOME/.cache/unslot-image}"
RELEASE="https://github.com/pvaibhav/BaseOS/releases/download/v$BASEOS_VERSION"
SECTOR=512

[ -f "$TREE/System/slot" ] || { echo "$TREE has no System/slot" >&2; exit 1; }
# FAT made every file executable; ext4 keeps the tree's modes, and Services runs these directly.
for bin in syncthing tailscaled wireguard-go samba/bin/smbd samba/bin/smbpasswd; do
	f="$TREE/System/services/$bin"
	[ ! -e "$f" ] || [ -x "$f" ] || { echo "$f is not executable" >&2; exit 1; }
done
# wpa_cli runs this itself as the link comes and goes, to start DHCP.
[ -x "$TREE/System/slotwifi.sh" ] || { echo "$TREE/System/slotwifi.sh is not executable" >&2; exit 1; }
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

# resize2fs for the device: e2fsprogs at the version of BaseOS's own e2fsck, static for aarch64.
E2FS_VERSION=1.47.2
E2FS_SHA256=08242e64ca0e8194d9c1caad49762b19209a06318199b63ce74ae4ef2d74e63c
resize2fs="$CACHE/resize2fs-$E2FS_VERSION"
if [ ! -f "$resize2fs" ]; then
	curl -fsSL -o "$work/e2fsprogs.tar.xz" \
		"https://mirrors.edge.kernel.org/pub/linux/kernel/people/tytso/e2fsprogs/v$E2FS_VERSION/e2fsprogs-$E2FS_VERSION.tar.xz"
	echo "$E2FS_SHA256  $work/e2fsprogs.tar.xz" | sha256sum -c - > /dev/null
	tar -xf "$work/e2fsprogs.tar.xz" -C "$work"
	(cd "$work/e2fsprogs-$E2FS_VERSION" &&
		./configure -q --host=aarch64-linux-gnu --disable-nls --disable-fuse2fs \
			--disable-e2initrd-helper --disable-uuidd --disable-defrag --disable-imager \
			LDFLAGS=-static > /dev/null &&
		make -s -j"$(nproc)" libs > /dev/null &&
		make -s -C resize resize2fs > /dev/null)
	aarch64-linux-gnu-strip -o "$resize2fs.new" "$work/e2fsprogs-$E2FS_VERSION/resize/resize2fs"
	mv "$resize2fs.new" "$resize2fs"
fi

# The rootfs: expand-storage grows p7's ext4 where BaseOS would format it.
dd if="$img" of="$work/rootfs" bs=$SECTOR skip="$ROOT_START" count=$((ROOT_END - ROOT_START + 1)) status=none
debugfs -R "dump /usr/sbin/expand-storage $work/expand-storage" "$work/rootfs" 2> /dev/null
grep -q '^/usr/sbin/gptgrow "\$DISK"' "$work/expand-storage" &&
	grep -q '^# Fresh empty FAT32 across' "$work/expand-storage" ||
	{ echo "BaseOS $BASEOS_VERSION's expand-storage has changed; check the patch" >&2; exit 1; }
cat > "$work/expand-ext4" <<'EOF'
# unslot: this card came with unslot on an ext4 p7, so grow that to fill the grown partition
# instead of formatting it. resize2fs wants a checked filesystem first.
/usr/sbin/e2fsck -fp "$CARD" >> "$LOG" 2>&1
if [ $? -ge 4 ]; then
	log "expand: e2fsck could not clean p7; leaving it at its first size"
	exit 1
fi
if ! /usr/sbin/resize2fs "$CARD" >> "$LOG" 2>&1; then
	log "expand: resize2fs failed; leaving p7 at its first size"
	exit 1
fi
log "storage expanded to fill the card"
EOF
awk -v tail="$work/expand-ext4" '
/^# Fresh empty FAT32 across/ { while ((getline l < tail) > 0) print l; exit }
{ print }
' "$work/expand-storage" > "$work/expand-storage.new"
debugfs -w -f - "$work/rootfs" > /dev/null 2>&1 <<EOF
rm /usr/sbin/expand-storage
write $work/expand-storage.new /usr/sbin/expand-storage
set_inode_field /usr/sbin/expand-storage mode 0100755
set_inode_field /usr/sbin/expand-storage uid 0
set_inode_field /usr/sbin/expand-storage gid 0
write $resize2fs /usr/sbin/resize2fs
set_inode_field /usr/sbin/resize2fs mode 0100755
set_inode_field /usr/sbin/resize2fs uid 0
set_inode_field /usr/sbin/resize2fs gid 0
EOF
debugfs -R "dump /usr/sbin/expand-storage $work/check" "$work/rootfs" 2> /dev/null
cmp -s "$work/check" "$work/expand-storage.new" || { echo "the rootfs did not take the patch" >&2; exit 1; }
debugfs -R "stat /usr/sbin/expand-storage" "$work/rootfs" 2> /dev/null | grep -q 'Mode:  0755' ||
	{ echo "expand-storage lost its mode" >&2; exit 1; }
debugfs -R "dump /usr/sbin/resize2fs $work/check" "$work/rootfs" 2> /dev/null
cmp -s "$work/check" "$resize2fs" || { echo "the rootfs did not take resize2fs" >&2; exit 1; }
dd if="$work/rootfs" of="$img" bs=$SECTOR seek="$ROOT_START" conv=notrunc status=none
debugfs -R "dump /usr/share/baseos/card-readme.txt $work/README.txt" "$work/rootfs" 2> /dev/null
debugfs -R "dump /usr/share/baseos/baseos.conf $work/baseos.conf" "$work/rootfs" 2> /dev/null

# p7 big enough for the tree with room to spare, grown by BaseOS's own gptgrow.
tree_mib=$(du -sm "$TREE" | cut -f1)
data_mib=$(((tree_mib + 256 + 255) / 256 * 256))
[ "$data_mib" -ge 512 ] || data_mib=512
curl -fsSL -o "$work/gptgrow.c" "https://raw.githubusercontent.com/pvaibhav/BaseOS/v$BASEOS_VERSION/tools/gptgrow.c"
cc -O2 -o "$work/gptgrow" "$work/gptgrow.c"
truncate -s $(((DATA_START + data_mib * 2048 + 4) * SECTOR)) "$img"
"$work/gptgrow" "$img" > /dev/null 2>&1 || true
set -- $(part 7)
[ "$1" = "$DATA_START" ] || { echo "p7 moved" >&2; exit 1; }
data_sectors=$(($2 - $1 + 1))
[ "$data_sectors" -ge $((data_mib * 2048)) ] || { echo "gptgrow did not grow p7" >&2; exit 1; }

# An ext4 named as BaseOS names p7, with the features BaseOS's kernel and e2fsck know, holding
# unslot and BaseOS's own notes, all root's as they would be on FAT.
mkdir "$work/p7"
cp -R "$TREE/." "$work/p7/"
cp "$work/README.txt" "$work/baseos.conf" "$work/p7/"
truncate -s $((data_sectors * SECTOR)) "$work/data"
fakeroot mke2fs -q -F -t ext4 -b 4096 -m 0 -L BASEOS \
	-O ^metadata_csum,^metadata_csum_seed,^64bit,^orphan_file -d "$work/p7" "$work/data"
e2fsck -fn "$work/data" > /dev/null 2>&1 || { echo "p7 does not check clean" >&2; exit 1; }
debugfs -R "stat /System/slot" "$work/data" 2> /dev/null | grep -q 'User:     0   Group:     0' ||
	{ echo "p7's files are not root's" >&2; exit 1; }
dd if="$work/data" of="$img" bs=$SECTOR seek="$DATA_START" conv=notrunc status=none

# BaseOS's gptgrow sees a laid-out card, as it will on a card the image's size.
if "$work/gptgrow" "$img" > /dev/null 2>&1; then
	echo "gptgrow would grow the finished image again" >&2
	exit 1
fi

mkdir -p "$(dirname "$OUT")"
# Where OUT is from here, before zip runs in $work, which a relative OUT would land in and the
# trap would then delete.
out="$(cd "$(dirname "$OUT")" && pwd)/$(basename "$OUT")"
name="$(basename "$OUT" .zip)"
mv "$img" "$work/$name"
rm -f "$out"
(cd "$work" && zip -q -9 "$out" "$name")
echo "$OUT: BaseOS $BASEOS_VERSION for $TARGET with unslot $(cat "$TREE/System/version") on a ${data_mib} MiB BASEOS partition"
