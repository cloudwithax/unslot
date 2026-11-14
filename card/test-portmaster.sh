#!/bin/sh
# The pad mapping control.txt gives ports, against the capabilities H700 models report, and the
# umount System/bin gives them.
set -eu

HERE="$(cd "$(dirname "$0")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT HUP INT TERM
BASH="${BASH_BIN:-bash}"

fail() {
	echo "portmaster: $1" >&2
	exit 1
}

# pad NAME KEYS ABS: an input device as sysfs shows it.
pad() {
	dev="$TMP/sys/event$(ls "$TMP/sys" 2>/dev/null | wc -l)/device"
	mkdir -p "$dev/capabilities"
	echo "$1" > "$dev/name"
	echo "$2" > "$dev/capabilities/key"
	echo "$3" > "$dev/capabilities/abs"
}

mapping() {
	"$BASH" -c 'eval "$(sed -n "/^unslot_pad_mapping()/,/^}/p" "$1")"; unslot_pad_mapping "$2"' \
		_ "$HERE/System/PortMaster/control.txt" "$TMP/sys"
}

# The RG CubeXX: two sticks and their clicks, on Z, RX, RY and RZ, and a hat, as measured with
# the SDL ports run on: the printed A, B, X and Y on buttons 0, 1, 3 and 2, L2 and R2 on 10 and 11.
pad axp2202-pek "10000000000000 0" 0
pad ANBERNIC-keys "400000000 1fff000000000000 0 0 c000000000000 2" 3003c
want="default,ANBERNIC-keys,a:b0,b:b1,x:b3,y:b2,leftshoulder:b4,rightshoulder:b5,back:b6,start:b7,guide:b8,leftstick:b9,lefttrigger:b10,righttrigger:b11,rightstick:b12,dpup:h0.1,dpdown:h0.4,dpleft:h0.8,dpright:h0.2,leftx:a0,lefty:a1,rightx:a2,righty:a3,platform:Linux,"
[ "$(mapping)" = "$want" ] || fail "the CubeXX's mapping is wrong: $(mapping)"

# A model without sticks or their clicks: L2 and R2 move down, and nothing is mapped to an axis.
rm -rf "$TMP/sys"
pad ANBERNIC-keys "0 dff000000000000 0 0 c000000000000 2" 30000
want="default,ANBERNIC-keys,a:b0,b:b1,x:b3,y:b2,leftshoulder:b4,rightshoulder:b5,back:b6,start:b7,guide:b8,lefttrigger:b9,righttrigger:b10,dpup:h0.1,dpdown:h0.4,dpleft:h0.8,dpright:h0.2,platform:Linux,"
[ "$(mapping)" = "$want" ] || fail "a stickless model's mapping is wrong: $(mapping)"

# No handheld pad, nothing to map.
rm -rf "$TMP/sys"
pad "Xbox Wireless Controller" "7fff000000000000 0 0 0 0" 3003f
[ -z "$(mapping)" ] || fail "another pad was mapped as the handheld's"

# A runtime's squashfs file handed to umount is unmounted wherever it is mounted, twice over here
# as BusyBox's left it; a place goes to BusyBox as it is. BusyBox stands in as a script noting what
# it was asked.
fs="$TMP/fs"
mkdir -p "$fs/sys/block/loop1/loop" "$fs/sys/block/loop2/loop" "$fs/proc" "$TMP/bin" "$TMP/libs"
: > "$TMP/libs/frt_3.5.2.squashfs"
: > "$TMP/libs/mono.squashfs"
echo "$TMP/libs/frt_3.5.2.squashfs" > "$fs/sys/block/loop1/loop/backing_file"
echo "$TMP/libs/mono.squashfs" > "$fs/sys/block/loop2/loop/backing_file"
printf '%s\n' "/dev/loop1 /godot squashfs ro 0 0" "/dev/loop2 /root/mono squashfs ro 0 0" \
	"/dev/loop1 /godot squashfs ro 0 0" > "$fs/proc/mounts"
printf '#!/bin/sh\necho "$*" >> "%s"\n' "$TMP/asked" > "$TMP/bin/busybox"
chmod +x "$TMP/bin/busybox"
umount_() { PATH="$TMP/bin:$PATH" SLOT_FS="$fs" sh "$HERE/System/bin/umount" "$@"; }
umount_ "$TMP/libs/frt_3.5.2.squashfs" || fail "a mounted runtime's file did not unmount"
[ "$(cat "$TMP/asked")" = "$(printf 'umount /godot\numount /godot')" ] ||
	fail "a runtime's file unmounted the wrong places: $(cat "$TMP/asked")"
rm "$TMP/asked"
umount_ /godot
[ "$(cat "$TMP/asked")" = "umount /godot" ] || fail "a place was not handed to BusyBox"
rm "$TMP/asked"
: > "$TMP/libs/other.squashfs"
umount_ "$TMP/libs/other.squashfs" 2>/dev/null && fail "a file mounted nowhere said it unmounted"
[ ! -e "$TMP/asked" ] || fail "a file mounted nowhere went to BusyBox"

echo "portmaster: all passed"
