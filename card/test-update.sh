#!/bin/sh
# The update pack, end to end off the device: tools/update/pack.sh builds one, it is unzipped
# the way BaseOS's frontend-session does at boot, and its installer runs. Then slotupdate.sh,
# which the updater fetches packs with, against a pretend curl.
set -eu

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(dirname "$HERE")"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT HUP INT TERM
FAILS=0
# BaseOS's unzip is busybox's; use it when there is one, so its pattern matching is what is tested.
UNZIP="unzip"
if command -v busybox > /dev/null 2>&1; then UNZIP="busybox unzip"; fi

fail() {
	echo "FAIL: $*"
	FAILS=$((FAILS + 1))
}

tree="$TMP/tree"
mkdir -p "$tree/System/licenses"
echo "new slot" > "$tree/System/slot"
echo "v9.9.9" > "$tree/System/version"
echo "licence" > "$tree/System/licenses/a licence.txt"
sh "$REPO/tools/update/pack.sh" "$tree" "$TMP/MinUI.zip" > /dev/null || fail "pack.sh failed"
[ "$(unzip -Z1 "$TMP/MinUI.zip" | tail -n 1)" = ".tmp_update/h700.sh" ] ||
	fail "the installer is not the last thing in the zip"
unzip -p "$TMP/MinUI.zip" .tmp_update/unslot/manifest | grep -qx 'version=v9.9.9' ||
	fail "the manifest does not say which version it is"

CARD="$TMP/card"

# A card running slot, with a save on it and the pack at its root.
card() {
	rm -rf "$CARD"
	mkdir -p "$CARD/System" "$CARD/Saves/GBA" "$CARD/Logs"
	echo "old slot" > "$CARD/System/slot"
	echo "save" > "$CARD/Saves/GBA/Emerald.sav"
	cp "$TMP/MinUI.zip" "$CARD/MinUI.zip"
}

# What BaseOS's frontend-session does before it picks a frontend.
unpack() {
	if [ ! -f "$CARD/.tmp_update/h700.sh" ] && [ -f "$CARD/MinUI.zip" ]; then
		$UNZIP -o "$CARD/MinUI.zip" ".tmp_update/*" -d "$CARD" > /dev/null 2>&1
	fi
}

install() {
	sh "$CARD/.tmp_update/h700.sh" > /dev/null 2>&1
}

boot() {
	unpack
	[ -f "$CARD/.tmp_update/h700.sh" ] && install
}

card
boot && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "the install exited $rc"
[ "$(cat "$CARD/System/slot")" = "new slot" ] || fail "System/ was not replaced"
[ "$(cat "$CARD/System/licenses/a licence.txt")" = "licence" ] || fail "a file with a space was lost"
[ "$(cat "$CARD/.unslot-update/System.old/slot")" = "old slot" ] || fail "the old System was not kept"
[ "$(cat "$CARD/.unslot-update/trial")" = 0 ] || fail "the new System is not on trial"
[ "$(cat "$CARD/Saves/GBA/Emerald.sav")" = "save" ] || fail "a save was touched"
[ ! -e "$CARD/MinUI.zip" ] || fail "the pack was left to install again"
[ ! -e "$CARD/.tmp_update" ] || fail "the unzipped pack was left on the card"
grep -q "installed v9.9.9" "$CARD/Logs/update.log" || fail "the install was not logged"
boot || true
[ ! -e "$CARD/.unslot-update/System.old/version" ] || fail "a second boot installed again"

# Power lost with the old System moved aside but the new one not yet in.
card
unpack
mkdir -p "$CARD/.unslot-update"
mv "$CARD/System" "$CARD/.unslot-update/System.old"
install || fail "the install after a lost move failed"
[ "$(cat "$CARD/System/slot")" = "new slot" ] || fail "the new System did not go in after a lost move"
[ "$(cat "$CARD/.unslot-update/System.old/slot")" = "old slot" ] || fail "the old System was lost"

# Power lost with the new System in but the pack not yet cleared.
card
unpack
mkdir -p "$CARD/.unslot-update"
mv "$CARD/System" "$CARD/.unslot-update/System.old"
mv "$CARD/.tmp_update/unslot/System" "$CARD/System"
install || fail "finishing an install exited non-zero"
[ "$(cat "$CARD/System/slot")" = "new slot" ] || fail "a finished install was undone"
[ ! -e "$CARD/MinUI.zip" ] || fail "the pack of a finished install was left"
[ ! -e "$CARD/.tmp_update" ] || fail "the unzipped pack of a finished install was left"

# A payload that does not match its checksums is put aside, and nothing changes.
card
unpack
echo "tampered" > "$CARD/.tmp_update/unslot/System/slot"
install && fail "a tampered pack installed"
[ "$(cat "$CARD/System/slot")" = "old slot" ] || fail "a tampered pack replaced System"
[ -f "$CARD/MinUI.zip.bad" ] || fail "a bad pack was not set aside"
[ ! -e "$CARD/MinUI.zip" ] || fail "a bad pack would be tried again at every boot"
[ ! -e "$CARD/.tmp_update" ] || fail "a bad pack's files were left"
grep -q "did not match its checksums" "$CARD/Logs/update.log" || fail "a bad pack was not logged"

# Another frontend's pack is not installed.
card
unpack
sed -i 's/^format=.*/format=nextui/' "$CARD/.tmp_update/unslot/manifest"
install && fail "a pack that is not unslot's installed"
[ "$(cat "$CARD/System/slot")" = "old slot" ] || fail "a foreign pack replaced System"

# A card with no frontend yet gets one, with nothing to roll back to.
card
rm -rf "$CARD/System"
boot || fail "installing onto an empty card failed"
[ "$(cat "$CARD/System/slot")" = "new slot" ] || fail "an empty card got no System"
[ ! -e "$CARD/.unslot-update/trial" ] || fail "a first install was put on trial with nothing to go back to"

# unslot's own packs: the launcher installs unslot-<version>.pak from the card's root when slot
# says it is signed, and leaves one that is not as .bad. A pretend slot stands in for the check:
# a pack holding the word "signed" passes and gives up the zip built above.
cat > "$TMP/fake-slot" <<EOF
#!/bin/sh
[ "\$1" = --unpack-pak ] || exit 2
grep -q signed "\$2" || { echo "not signed by unslot" >&2; exit 1; }
cp "$TMP/MinUI.zip" "\$3" && echo v9.9.9
EOF
launch() {
	SLOT_ROOT="$CARD" AGS_RUN="$TMP/run" AGS_SLOT="sh $TMP/fake-slot" 		sh "$HERE/System/launch_frontend.sh" > /dev/null 2>&1 || true
}
mkdir -p "$TMP/run"
card
rm -f "$CARD/MinUI.zip"
echo signed > "$CARD/unslot-v9.9.9.pak"
: > "$CARD/System/led.sh"
launch
[ "$(cat "$CARD/System/slot")" = "new slot" ] || fail "a signed .pak was not installed"
[ ! -e "$CARD/unslot-v9.9.9.pak" ] || fail "an installed .pak was left to install again"
[ ! -e "$CARD/.tmp_update" ] || fail "the .pak's unpacked files were left"
[ ! -e "$CARD/.unslot-update/pak.zip" ] || fail "the .pak's zip was left"
[ "$(cat "$CARD/.unslot-update/trial")" = 1 ] || fail "the start after a .pak install was not counted"
[ "$(cat "$CARD/Saves/GBA/Emerald.sav")" = "save" ] || fail "a .pak install touched a save"

card
rm -f "$CARD/MinUI.zip"
echo "made up" > "$CARD/unslot-v9.9.9.pak"
: > "$CARD/System/led.sh"
launch
[ "$(cat "$CARD/System/slot")" = "old slot" ] || fail "an unsigned .pak replaced System"
[ -f "$CARD/unslot-v9.9.9.pak.bad" ] || fail "an unsigned .pak was not set aside"
[ ! -e "$CARD/unslot-v9.9.9.pak" ] || fail "an unsigned .pak would be tried at every start"
grep -q "not an update unslot signed" "$CARD/Logs/update.log" || fail "an unsigned .pak was not logged"

# slotupdate.sh fetches through curl and checks what it got.
mkdir -p "$TMP/bin"
cat > "$TMP/bin/curl" <<'EOF'
#!/bin/sh
out=""
while [ $# -gt 0 ]; do
	[ "$1" = -o ] && out="$2" && shift
	shift
done
[ -n "${FAKE_CURL_FAIL:-}" ] && exit 22
if [ -n "$out" ]; then printf 'payload' > "$out"; else echo '{"tag_name": "v1.0.0"}'; fi
EOF
chmod 755 "$TMP/bin/curl"
fetch() {
	AGS_CURL="$TMP/bin/curl" sh "$HERE/System/slotupdate.sh" "$@"
}
good="$(printf 'payload' | sha256sum | cut -d' ' -f1)"
[ "$(fetch get https://api.github.com/x)" = '{"tag_name": "v1.0.0"}' ] || fail "get did not print the listing"
fetch fetch https://example/MinUI.zip "$good" "$TMP/got.zip" || fail "a good download failed"
[ "$(cat "$TMP/got.zip")" = payload ] || fail "a good download was not put in place"
[ ! -e "$TMP/got.zip.part" ] || fail "the partial file was left"
fetch fetch https://example/MinUI.zip 0000 "$TMP/bad.zip" 2> "$TMP/err" && fail "a mismatched download was kept"
[ ! -e "$TMP/bad.zip" ] && [ ! -e "$TMP/bad.zip.part" ] || fail "a mismatched download was left on the card"
grep -q checksum "$TMP/err" || fail "a mismatch did not say so"
FAKE_CURL_FAIL=1 fetch fetch https://example/MinUI.zip "$good" "$TMP/none.zip" 2> /dev/null &&
	fail "a failed download succeeded"
FAKE_CURL_FAIL=1 fetch get https://api.github.com/x 2> "$TMP/err" && fail "a failed listing succeeded"
grep -q "no release yet" "$TMP/err" || fail "a 404 did not say there was no release"
fetch nonsense 2> /dev/null && rc=0 || rc=$?
[ "$rc" = 2 ] || fail "an unknown verb exited $rc, want 2"

if [ "$FAILS" -gt 0 ]; then
	echo "$FAILS failure(s)"
	exit 1
fi
echo "update: ok"
