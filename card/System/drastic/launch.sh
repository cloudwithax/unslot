#!/bin/sh
# Runs the DS game $1 in DraStic, for launch_frontend.sh once slot has quit for it; slot starts
# again when the game is left from slot's menu or DraStic's. DraStic keeps its settings, saves and states in the
# folder it runs from, so it runs from Saves/NDS, which an update leaves alone, with its own BIOS
# and game fixes brought over from here.
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
# DraStic has no volume of its own, so slot reads the volume keys while it runs and leaves the
# level in UNSLOT_VOLUME, which libunslot-drastic.so scales DraStic's sound by and shows. slot
# reads Menu too, and opens its own menu over the game; DraStic's is on Select + X.
export UNSLOT_VOLUME=/run/unslot-drastic-volume
/lib/ld-linux-aarch64.so.1 "$here/../slot" --drastic &
volume=$!
LD_LIBRARY_PATH="$here/libs" LD_PRELOAD="$here/libs/libunslot-drastic.so" SDL_LINUX_JOYSTICK_CLASSIC=1 \
	"$here/drastic" "$1"
kill "$volume" 2>/dev/null
rm -f "$UNSLOT_VOLUME"
