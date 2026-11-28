#!/bin/sh
# Runs the DS game $1 in DraStic, for slot to drive as one of its cores. Persistent saves and
# settings live under Saves/NDS (which an update leaves alone), while transient savestates and
# unzip_cache run out of RAM tmpfs when driven by slot so save/load does not stall on FAT32.
here="$(cd "$(dirname "$0")" && pwd)"
SD="${SLOT_ROOT:-/mnt/sdcard}"
home="$SD/Saves/NDS"
states="${SLOT_DRASTIC_STATES:-}"
[ -d "$home/config" ] || mkdir -p "$home/config"
[ -d "$home/backup" ] || mkdir -p "$home/backup"

# The controls are set for the H700's pad as SDL numbers it through the kernel's joystick node,
# there being no udev on BaseOS to say the pad is one. DraStic writes its settings back, so the
# defaults go in only once.
[ -f "$home/config/drastic.cfg" ] || cp "$here/drastic.cfg" "$home/config/drastic.cfg"

if [ -n "$states" ]; then
	run="/tmp/unslot-drastic-home"
	unzip="/tmp/unslot-drastic-unzip"
	[ -d "$states" ] || mkdir -p "$states"
	[ -d "$unzip" ] || mkdir -p "$unzip"
	if [ "$(readlink "$run/config" 2>/dev/null)" != "$home/config" ] ||
		[ "$(readlink "$run/savestates" 2>/dev/null)" != "$states" ] ||
		[ "$(readlink "$run/system" 2>/dev/null)" != "$here/system" ]; then
		rm -rf "$run"
		mkdir -p "$run"
		for sub in backup cheats config input_record microphone profiles scripts slot2; do
			[ -d "$home/$sub" ] || mkdir -p "$home/$sub"
			ln -sfn "$home/$sub" "$run/$sub"
		done
		ln -sfn "$here/system" "$run/system"
		ln -sfn "$here/game_database.xml" "$run/game_database.xml"
		ln -sfn "$states" "$run/savestates"
		ln -sfn "$unzip" "$run/unzip_cache"
	fi
	cd "$run"
else
	[ -d "$home/system" ] || mkdir -p "$home/system"
	cd "$home"
	for f in system/drastic_bios_arm7.bin system/drastic_bios_arm9.bin game_database.xml; do
		cmp -s "$here/$f" "$f" || cp "$here/$f" "$f"
	done
fi

# In kiosk mode DraStic's own menu, which can load any game on the card, is unbound for the
# session and its binding put back after; a session cut short puts it back at the next launch.
keep="$home/config/drastic.cfg.kiosk"
[ -f "$keep" ] && mv "$keep" "$home/config/drastic.cfg"
if grep -q '^active=1$' "$SD/Config/kiosk.conf" 2>/dev/null; then
	cp "$home/config/drastic.cfg" "$keep"
	sed -i 's/^\(controls_[ab]\[CONTROL_INDEX_MENU\]\) = .*/\1 = 65535/' "$home/config/drastic.cfg"
	LD_LIBRARY_PATH="$here/libs" LD_PRELOAD="$here/libs/libunslot-drastic.so" SDL_LINUX_JOYSTICK_CLASSIC=1 \
		"$here/drastic" "$1"
	status=$?
	[ -f "$keep" ] && mv "$keep" "$home/config/drastic.cfg"
	exit "$status"
fi

# slot drives DraStic itself (crates/slot/src/standalone.rs): libunslot-drastic.so hands it each
# frame, its sound and the pad over UNSLOT_BRIDGE, so slot's volume and menu are DraStic's too.
LD_LIBRARY_PATH="$here/libs" LD_PRELOAD="$here/libs/libunslot-drastic.so" SDL_LINUX_JOYSTICK_CLASSIC=1 \
	exec "$here/drastic" "$1"
