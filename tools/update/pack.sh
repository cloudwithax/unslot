#!/bin/sh
# Builds an update pack from a device tree (task dist:device): a zip that BaseOS installs at
# boot when it is at the card's root as MinUI.zip. It replaces System/ and nothing else, so it
# updates unslot, or turns a card running slot into one running unslot.
#
#   pack.sh TREE OUT.zip
set -eu

TREE="$1"
OUT="$2"
REPO="$(cd "$(dirname "$0")/../.." && pwd)"

[ -f "$TREE/System/slot" ] || { echo "$TREE has no System/slot" >&2; exit 1; }
[ -s "$TREE/System/version" ] || { echo "$TREE has no System/version" >&2; exit 1; }
version="$(cat "$TREE/System/version")"

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT HUP INT TERM
pack="$stage/.tmp_update/unslot"
mkdir -p "$pack"
cp -R "$TREE/System" "$pack/System"
{
	echo "format=unslot-update/1"
	echo "target=h700"
	echo "version=$version"
	echo "build=$(git -C "$REPO" rev-parse --short HEAD 2>/dev/null || echo unknown)"
} > "$pack/manifest"
(cd "$pack" && find System -type f | LC_ALL=C sort | while IFS= read -r f; do sha256sum "$f"; done) \
	> "$pack/System.sha256"
cp "$REPO/card/update/h700.sh" "$stage/.tmp_update/h700.sh"

mkdir -p "$(dirname "$OUT")"
out="$(cd "$(dirname "$OUT")" && pwd)/$(basename "$OUT")"
rm -f "$out"
# The installer goes in last: BaseOS unzips in order, so once it is on the card the rest is.
(cd "$stage" && zip -qrX "$out" .tmp_update/unslot && zip -qX "$out" .tmp_update/h700.sh)
echo "$out: unslot $version"
