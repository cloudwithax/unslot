#!/bin/sh
# Runs the DS game $1 in DraStic, for slot to drive as one of its cores. DraStic keeps its
# settings, saves and states in the folder it runs from, so it runs from Saves/NDS, which an
# update leaves alone, with its own BIOS and game fixes brought over from here.
here="$(cd "$(dirname "$0")" && pwd)"
home="${SLOT_ROOT:-/mnt/sdcard}/Saves/NDS"
mkdir -p "$home/system" "$home/config"
cd "$home"
for f in system/drastic_bios_arm7.bin system/drastic_bios_arm9.bin game_database.xml; do
	cmp -s "$here/$f" "$f" || cp "$here/$f" "$f"
done
# The controls are set for the H700's pad as SDL numbers it through the kernel's joystick node,
# there being no udev on BaseOS to say the pad is one. DraStic writes its settings back, so the
# defaults go in only once.
[ -f config/drastic.cfg ] || cp "$here/drastic.cfg" config/drastic.cfg
# In kiosk mode DraStic's own menu, which can load any game on the card, is unbound for the
# session and its binding put back after; a session cut short puts it back at the next launch.
keep=config/drastic.cfg.kiosk
[ -f "$keep" ] && mv "$keep" config/drastic.cfg
if grep -q '^active=1$' "${SLOT_ROOT:-/mnt/sdcard}/Config/kiosk.conf" 2>/dev/null; then
	cp config/drastic.cfg "$keep"
	sed -i 's/^\(controls_[ab]\[CONTROL_INDEX_MENU\]\) = .*/\1 = 65535/' config/drastic.cfg
fi
# slot drives DraStic itself (crates/slot/src/standalone.rs): libunslot-drastic.so hands it each
# frame, its sound and the pad over UNSLOT_BRIDGE, so slot's volume and menu are its own.
LD_LIBRARY_PATH="$here/libs" LD_PRELOAD="$here/libs/libunslot-drastic.so" SDL_LINUX_JOYSTICK_CLASSIC=1 \
	"$here/drastic" "$1"
if [ -f "$keep" ]; then mv "$keep" config/drastic.cfg; fi
