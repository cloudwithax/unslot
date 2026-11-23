#!/bin/sh
set -eu

HERE="$(cd "$(dirname "$0")" && pwd)"
SCRIPT="$HERE/System/launch_frontend.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT HUP INT TERM

setup() {
	rm -rf "$TMP/card" "$TMP/run" "$TMP/fs" "$TMP/asked"
	mkdir -p "$TMP/card/System" "$TMP/run" "$TMP/fs"
	: > "$TMP/card/System/led.sh"
}

# slot stands in as a script that notes what it was asked, and refuses a port's runtimes when
# REFUSE is there.
launch() {
	SLOT_ROOT="$TMP/card" AGS_RUN="$TMP/run" SLOT_FS="$TMP/fs" AGS_SLOT="sh $TMP/slot" \
		sh "$SCRIPT" 2>/dev/null || true
}
cat > "$TMP/slot" <<EOF
#!/bin/sh
echo "\$*" >> "$TMP/asked"
[ "\$1" = --port-runtimes ] && [ -e "$TMP/REFUSE" ] && exit 1
[ "\$1" = --port-volume ] && { shift; exec "\$@"; }
exit 0
EOF

fail() {
	echo "launch: $*" >&2
	exit 1
}

setup
echo "old run" > "$TMP/card/slot.log"
echo "older run" > "$TMP/card/slot.log.1"
echo "12:00:00 exec" > "$TMP/card/slot-session.log"
echo "alarm" > "$TMP/card/slot-card.log"
echo "boot" > "$TMP/card/ags-shutdown-trace.log"
echo "keys" > "$TMP/card/input-trace.log"
mkdir -p "$TMP/card/crash"
echo "pid=1" > "$TMP/card/crash/cores.log"
launch
for f in slot.log slot.log.1 slot-session.log slot-card.log ags-shutdown-trace.log input-trace.log crash; do
	[ ! -e "$TMP/card/$f" ] || fail "$f was left at the root of the card"
done
[ "$(cat "$TMP/card/Logs/slot.log.1")" = "old run" ] || fail "the last run's log was not kept"
[ "$(cat "$TMP/card/Logs/slot-card.log")" = "alarm" ] || fail "slot-card.log did not move"
[ "$(cat "$TMP/card/Logs/ags-shutdown-trace.log")" = "boot" ] || fail "the shutdown trace did not move"
[ "$(cat "$TMP/card/Logs/input-trace.log")" = "keys" ] || fail "the input trace did not move"
[ "$(cat "$TMP/card/Logs/crash/cores.log")" = "pid=1" ] || fail "crash/ did not move"
[ "$(head -1 "$TMP/card/Logs/slot-session.log")" = "12:00:00 exec" ] || fail "the session log lost its history"
tail -1 "$TMP/card/Logs/slot-session.log" | grep -Eq '^[0-9]{4}-[0-9]{2}-[0-9]{2} [0-9:]{8} exec ' \
	|| fail "the session line has no date: $(tail -1 "$TMP/card/Logs/slot-session.log")"

setup
mkdir -p "$TMP/card/Logs"
echo "this run" > "$TMP/card/Logs/slot.log"
launch
[ "$(cat "$TMP/card/Logs/slot.log.1")" = "this run" ] || fail "slot.log did not rotate"

setup
mkdir -p "$TMP/card/Logs"
head -c 70000 /dev/zero | tr '\0' 'x' > "$TMP/card/Logs/slot-session.log"
launch
[ "$(wc -c < "$TMP/card/Logs/slot-session.log.1")" -eq 70000 ] || fail "a full session log did not rotate"
[ "$(grep -c . "$TMP/card/Logs/slot-session.log")" = "1" ] || fail "the new session log did not start fresh"

setup
mkdir -p "$TMP/card/Logs"
head -c 1000 /dev/zero | tr '\0' 'x' > "$TMP/card/Logs/slot-session.log"
launch
[ ! -e "$TMP/card/Logs/slot-session.log.1" ] || fail "a small session log rotated"

# An update on trial counts its starts, and the System it replaced goes back after three.
setup
mkdir -p "$TMP/card/.unslot-update/System.old"
echo old > "$TMP/card/.unslot-update/System.old/slot"
echo new > "$TMP/card/System/slot"
echo 0 > "$TMP/card/.unslot-update/trial"
launch
[ "$(cat "$TMP/card/.unslot-update/trial")" = 1 ] || fail "a start on trial was not counted"
launch
launch
[ "$(cat "$TMP/card/System/slot")" = new ] || fail "the update was put back before its third start"
launch
[ "$(cat "$TMP/card/System/slot")" = old ] || fail "an update that never started was kept"
[ ! -e "$TMP/card/.unslot-update/trial" ] || fail "the trial outlived the rollback"
[ ! -e "$TMP/card/.unslot-update/System.failed" ] || fail "the failed System was left on the card"
grep -q "putting the old System back" "$TMP/card/Logs/update.log" || fail "the rollback was not logged"

# Runtimes ports downloaded while the update was on trial go back with the old System.
setup
mkdir -p "$TMP/card/.unslot-update/System.old" "$TMP/card/System/PortMaster/libs"
echo old > "$TMP/card/.unslot-update/System.old/slot"
echo godot > "$TMP/card/System/PortMaster/libs/frt_3.5.2.squashfs"
echo 3 > "$TMP/card/.unslot-update/trial"
launch
[ "$(cat "$TMP/card/System/slot")" = old ] || fail "an update that never started was kept"
[ "$(cat "$TMP/card/System/PortMaster/libs/frt_3.5.2.squashfs" 2>/dev/null)" = godot ] ||
	fail "a runtime downloaded on trial was lost in the rollback"

# A confirmed update, with no trial left, is started as it is.
setup
mkdir -p "$TMP/card/.unslot-update"
echo new > "$TMP/card/System/slot"
launch
launch
launch
launch
[ "$(cat "$TMP/card/System/slot")" = new ] || fail "a System with no trial was rolled back"

# A port slot quit for runs once, from its own folder, before slot starts again.
setup
mkdir -p "$TMP/card/Ports/game"
printf '#!/bin/bash\npwd > "$1ran"; echo said\n' > "$TMP/card/Ports/Game.sh"
printf '%s' "$TMP/card/Ports/Game.sh" > "$TMP/card/.unslot-port"
launch
[ "$(cat "$TMP/card/Ports/ran")" = "$TMP/card/Ports" ] || fail "the port did not run from its folder"
[ ! -e "$TMP/card/.unslot-port" ] || fail "the port was left to run again"
grep -q said "$TMP/card/Logs/ports.log" || fail "the port's output was not logged"
rm "$TMP/card/Ports/ran"
launch
[ ! -e "$TMP/card/Ports/ran" ] || fail "the port ran a second time"

# A PortMaster port finds PortMaster, its own files under /roms/ports and bash where it looks,
# and what is in System/bin on its PATH.
setup
mkdir -p "$TMP/card/Ports/game" "$TMP/card/System/PortMaster" "$TMP/card/System/bin"
printf '#!/bin/sh\necho tool\n' > "$TMP/card/System/bin/unslot-tool"
chmod +x "$TMP/card/System/bin/unslot-tool"
printf '#!/bin/bash\nunslot-tool > game/ran\n' > "$TMP/card/Ports/Game.sh"
printf '%s' "$TMP/card/Ports/Game.sh" > "$TMP/card/.unslot-port"
launch
[ "$(readlink "$TMP/fs/opt/tools/PortMaster")" = "$TMP/card/System/PortMaster" ] || fail "PortMaster was not linked where ports look"
[ "$(readlink "$TMP/fs/roms/ports")" = "$TMP/card/Ports" ] || fail "Ports was not linked at /roms/ports"
[ "$(readlink "$TMP/fs/bin/bash")" = "$TMP/card/System/bin/bash" ] || fail "bash was not linked"
[ "$(readlink "$TMP/fs/dev/fd")" = /proc/self/fd ] || fail "/dev/fd was not linked for bash's >(...)"
[ "$(readlink "$TMP/fs/dev/stderr")" = /proc/self/fd/2 ] || fail "/dev/stderr was not linked"
[ "$(cat "$TMP/card/Ports/game/ran")" = tool ] || fail "System/bin was not on the port's PATH"
grep -qx -- "--port-runtimes $TMP/card/Ports/Game.sh" "$TMP/asked" || fail "the port's runtimes were not looked for"

# A port whose runtimes are not on the card, and were not downloaded, is not run.
setup
mkdir -p "$TMP/card/Ports"
printf '#!/bin/bash\necho ran > ran\n' > "$TMP/card/Ports/Game.sh"
printf '%s' "$TMP/card/Ports/Game.sh" > "$TMP/card/.unslot-port"
: > "$TMP/REFUSE"
launch
rm "$TMP/REFUSE"
[ ! -e "$TMP/card/Ports/ran" ] || fail "a port without its runtimes ran"
grep -q "port not run" "$TMP/card/Logs/slot-session.log" || fail "the port not run was not logged"

# What is really at those places is left alone.
setup
mkdir -p "$TMP/card/Ports" "$TMP/fs/roms/ports" "$TMP/fs/bin"
echo mine > "$TMP/fs/bin/bash"
printf '#!/bin/sh\n' > "$TMP/card/Ports/Game.sh"
printf '%s' "$TMP/card/Ports/Game.sh" > "$TMP/card/.unslot-port"
launch
[ -d "$TMP/fs/roms/ports" ] && [ ! -L "$TMP/fs/roms/ports" ] || fail "a real /roms/ports was replaced"
[ "$(cat "$TMP/fs/bin/bash")" = mine ] || fail "a bash already there was replaced"

# A firmware without /etc/ssl/certs/ca-certificates.crt gets the card's CA bundle; one with its own keeps it.
setup
mkdir -p "$TMP/card/System/ssl" "$TMP/card/Ports"
echo bundle > "$TMP/card/System/ssl/ca-certificates.crt"
printf '#!/bin/sh\necho "${SSL_CERT_FILE:-}:${CURL_CA_BUNDLE:-}" > certs\n' > "$TMP/card/Ports/Certs.sh"
printf '%s' "$TMP/card/Ports/Certs.sh" > "$TMP/card/.unslot-port"
(unset SSL_CERT_FILE CURL_CA_BUNDLE; launch)
[ "$(cat "$TMP/card/Ports/certs")" = "$TMP/card/System/ssl/ca-certificates.crt:$TMP/card/System/ssl/ca-certificates.crt" ] ||
	fail "the card's CA bundle was not exported when the firmware had none: $(cat "$TMP/card/Ports/certs")"

mkdir -p "$TMP/fs/etc/ssl/certs"
echo system > "$TMP/fs/etc/ssl/certs/ca-certificates.crt"
printf '%s' "$TMP/card/Ports/Certs.sh" > "$TMP/card/.unslot-port"
(unset SSL_CERT_FILE CURL_CA_BUNDLE; launch)
[ "$(cat "$TMP/card/Ports/certs")" = ":" ] ||
	fail "the card's CA bundle overrode the firmware's own: $(cat "$TMP/card/Ports/certs")"

# Cores that name desktop GL are refused by the loader on a firmware without it, so slot gets the
# card's shim on its loader path, first, with what was there after it; a port does not.
setup
mkdir -p "$TMP/card/System/lib" "$TMP/card/Ports"
: > "$TMP/card/System/lib/libGL.so.1"
printf '#!/bin/sh\necho "${LD_LIBRARY_PATH:-}" > %s/loader\n' "$TMP" > "$TMP/slot"
printf '#!/bin/sh\necho "${LD_LIBRARY_PATH:-}" > loader\n' > "$TMP/card/Ports/Loader.sh"
printf '%s' "$TMP/card/Ports/Loader.sh" > "$TMP/card/.unslot-port"
(unset LD_LIBRARY_PATH; launch)
[ "$(cat "$TMP/loader" 2>/dev/null)" = "$TMP/card/System/lib" ] ||
	fail "slot was not given the card's libGL shim: $(cat "$TMP/loader" 2>/dev/null)"
[ -z "$(cat "$TMP/card/Ports/loader" 2>/dev/null)" ] || fail "a port was given the libGL shim"

echo "launch: all passed"
