#!/bin/sh
# Installs unslot from an update pack. BaseOS runs this at boot, before it starts a frontend,
# when the pack is at the card's root as MinUI.zip: it unzips the pack's .tmp_update folder and
# runs .tmp_update/h700.sh, the hook NextUI's packs use. Only System/ is replaced, so games,
# saves and settings stay, and a card running slot becomes one running unslot.
#
# The pack is laid out the way a .bosupd is, a manifest saying what it is and a payload it can
# be checked against:
#
#   .tmp_update/unslot/manifest       format=unslot-update/1, target, version, build
#   .tmp_update/unslot/System.sha256  the sha256 of every file in the payload
#   .tmp_update/unslot/System/        the new System folder
#   .tmp_update/h700.sh               this, last in the zip, so the rest is out once it is
#
# The System folder it replaces goes to .unslot-update/System.old, with a trial count that
# System/launch_frontend.sh uses to put it back if the new one never starts. What happened goes
# in Logs/update.log.
set -u

HERE="$(cd "$(dirname "$0")" && pwd)"
SD="$(dirname "$HERE")"
PACK="$HERE/unslot"
UPD="$SD/.unslot-update"

say() {
	echo "unslot update: $*"
	/bin/mkdir -p "$SD/Logs"
	echo "$(date '+%F %T') $*" >> "$SD/Logs/update.log"
}

# Takes the unzipped pack off the card, so BaseOS has nothing to run until another comes.
clear_pack() {
	/bin/rm -rf "$PACK" "$HERE/h700.sh"
	rmdir "$HERE" 2>/dev/null || true
	sync
}

# Power went after the new System was moved in but before the pack was cleared.
if [ ! -d "$PACK/System" ]; then
	say "the pack is already installed; clearing it"
	/bin/rm -f "$SD/MinUI.zip"
	clear_pack
	exit 0
fi

reject() {
	say "$1; leaving the pack as MinUI.zip.bad"
	/bin/mv -f "$SD/MinUI.zip" "$SD/MinUI.zip.bad" 2>/dev/null || true
	clear_pack
	exit 1
}

manifest="$PACK/manifest"
grep -qx 'format=unslot-update/1' "$manifest" 2>/dev/null || reject "this is not an unslot pack"
grep -qx 'target=h700' "$manifest" || reject "this pack is not for the H700"
version="$(sed -n 's/^version=//p' "$manifest" | head -n 1)"
(cd "$PACK" && sha256sum -c System.sha256 > /dev/null 2>&1) ||
	reject "$version did not match its checksums"
[ -f "$PACK/System/slot" ] || reject "$version has no System/slot"

# A System already moved aside by an install that lost power is the one to keep.
if [ -d "$SD/System" ]; then
	/bin/mkdir -p "$UPD"
	/bin/rm -rf "$UPD/System.old" "$UPD/System.failed"
	/bin/mv "$SD/System" "$UPD/System.old" || reject "the old System could not be moved aside"
fi
/bin/mv "$PACK/System" "$SD/System" || {
	/bin/mv "$UPD/System.old" "$SD/System" 2>/dev/null
	reject "the new System could not be moved in"
}
[ -d "$UPD/System.old" ] && echo 0 > "$UPD/trial"
/bin/rm -f "$SD/MinUI.zip"
say "installed $version"
clear_pack
exit 0
