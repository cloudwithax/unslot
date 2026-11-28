#!/bin/sh
# Runs the PSP game $1 in standalone PPSSPPSDL, for slot to drive as one of its cores. Saves and
# PPSSPP settings live under Saves/PSP, and savestates live under States/PSP, which an update
# leaves alone.
here="$(cd "$(dirname "$0")" && pwd)"
SD="${SLOT_ROOT:-/mnt/sdcard}"
home="$SD/Saves/PSP"
states="${SLOT_PPSSPP_STATES:-$SD/States/PSP}"
slot_state="$SD/Config/slot.state"
ra_conf="$SD/Config/retroachievements.conf"

# The libretro core kept its memory stick in Saves/, so its game saves are in Saves/PSP/SAVEDATA.
# They move, once, to where PPSSPPSDL looks for them.
if [ -d "$home/SAVEDATA" ] && [ ! -e "$home/ppsspp/PSP/SAVEDATA" ]; then
	mkdir -p "$home/ppsspp/PSP"
	mv "$home/SAVEDATA" "$home/ppsspp/PSP/SAVEDATA"
fi

sys="$home/ppsspp/PSP/SYSTEM"
[ -d "$sys" ] || mkdir -p "$sys"
[ -d "$home/ppsspp/PSP/SAVEDATA" ] || mkdir -p "$home/ppsspp/PSP/SAVEDATA"
[ -d "$states" ] || mkdir -p "$states"

if [ -d "$home/ppsspp/PSP/PPSSPP_STATE" ] && [ ! -L "$home/ppsspp/PSP/PPSSPP_STATE" ]; then
	rmdir "$home/ppsspp/PSP/PPSSPP_STATE" 2>/dev/null || true
fi
xdg="/tmp/unslot-ppsspp-home"
if [ "$(readlink "$xdg/ppsspp/PSP/SYSTEM" 2>/dev/null)" = "$sys" ] &&
	[ "$(readlink "$xdg/ppsspp/PSP/PPSSPP_STATE" 2>/dev/null)" = "$states" ]; then
	export XDG_CONFIG_HOME="$xdg"
	export XDG_DATA_HOME="$xdg"
elif [ "$(readlink "$home/ppsspp/PSP/PPSSPP_STATE" 2>/dev/null)" = "$states" ] ||
	ln -sfn "$states" "$home/ppsspp/PSP/PPSSPP_STATE" 2>/dev/null; then
	export XDG_CONFIG_HOME="$home"
	export XDG_DATA_HOME="$home"
else
	# FAT32/exFAT cards do not support symlinks; bridge PSP dirs through tmpfs (cached across launches).
	rm -rf "$xdg"
	mkdir -p "$xdg/ppsspp/PSP"
	for sub in SYSTEM SAVEDATA Cheats GAME PLUGINS SCREENSHOT TEXTURES; do
		[ -d "$home/ppsspp/PSP/$sub" ] || mkdir -p "$home/ppsspp/PSP/$sub"
		ln -sfn "$home/ppsspp/PSP/$sub" "$xdg/ppsspp/PSP/$sub"
	done
	ln -sfn "$states" "$xdg/ppsspp/PSP/PPSSPP_STATE"
	export XDG_CONFIG_HOME="$xdg"
	export XDG_DATA_HOME="$xdg"
fi

# PPSSPP notes the graphics backend it is starting in FailedGraphicsBackends.txt and clears it once
# that works, so one start that crashed or was killed there leaves it never trying that backend
# again; this build has only OpenGL, so every PSP game would then refuse to start. And it counts
# its running instances in shared memory, which a killed one never takes itself out of: the next
# start believes it is the second and plays no sound.
rm -f "$sys/FailedGraphicsBackends.txt" /dev/shm/PPSSPP_ID /tmp/shm/PPSSPP_ID

# Seed defaults only on first launch so user adjustments inside PPSSPP persist.
[ -f "$sys/ppsspp.ini" ] || cp "$here/ppsspp.ini" "$sys/ppsspp.ini"
[ -f "$sys/controls.ini" ] || cp "$here/controls.ini" "$sys/controls.ini"

# Sync RetroAchievements account and hardcore state from slot's config (only rewrite on change).
ra_user=""
ra_token=""
if [ -f "$ra_conf" ]; then
	ra_user="$(awk -F= '/^[[:space:]]*user[[:space:]]*=/{sub(/^[^=]*=/,""); gsub(/^[[:space:]]+|[[:space:]\r]+$/,""); print; exit}' "$ra_conf")"
	ra_token="$(awk -F= '/^[[:space:]]*token[[:space:]]*=/{sub(/^[^=]*=/,""); gsub(/^[[:space:]]+|[[:space:]\r]+$/,""); print; exit}' "$ra_conf")"
fi
ra_hardcore=False
grep -q '^hardcore=1$' "$slot_state" 2>/dev/null && ra_hardcore=True
if [ -n "$ra_user" ] && [ -n "$ra_token" ]; then
	ra_enable=True
	[ "$(cat "$sys/ppsspp_retroachievements.dat" 2>/dev/null)" = "$ra_token" ] ||
		printf '%s' "$ra_token" > "$sys/ppsspp_retroachievements.dat"
else
	ra_enable=False
	[ ! -e "$sys/ppsspp_retroachievements.dat" ] || rm -f "$sys/ppsspp_retroachievements.dat"
fi

if ! grep -q "^AchievementsEnable = ${ra_enable}$" "$sys/ppsspp.ini" 2>/dev/null ||
	! grep -q "^AchievementsChallengeMode = ${ra_hardcore}$" "$sys/ppsspp.ini" 2>/dev/null ||
	! grep -q "^AchievementsUserName = ${ra_user}$" "$sys/ppsspp.ini" 2>/dev/null; then
	awk -v en="$ra_enable" -v hc="$ra_hardcore" -v user="$ra_user" '
function flush_ach() {
	if (!seen_en) print "AchievementsEnable = " en
	if (!seen_hc) print "AchievementsChallengeMode = " hc
	if (!seen_user) print "AchievementsUserName = " user
}
/^\[/ {
	if (in_ach) {
		flush_ach()
		in_ach = 0
	}
	if ($0 ~ /^\[Achievements\][[:space:]]*$/) {
		in_ach = 1
		has_ach = 1
	}
	print
	next
}
in_ach && $0 ~ /^[[:space:]]*AchievementsEnable[[:space:]]*=/ {
	print "AchievementsEnable = " en; seen_en = 1; next
}
in_ach && $0 ~ /^[[:space:]]*AchievementsChallengeMode[[:space:]]*=/ {
	print "AchievementsChallengeMode = " hc; seen_hc = 1; next
}
in_ach && $0 ~ /^[[:space:]]*AchievementsUserName[[:space:]]*=/ {
	print "AchievementsUserName = " user; seen_user = 1; next
}
{ print }
END {
	if (in_ach) {
		flush_ach()
	} else if (!has_ach) {
		print ""
		print "[Achievements]"
		flush_ach()
	}
}' "$sys/ppsspp.ini" > "$sys/ppsspp.ini.tmp" && mv "$sys/ppsspp.ini.tmp" "$sys/ppsspp.ini"
fi

# Derive SDL_GAMECONTROLLERCONFIG for ANBERNIC-keys from /sys/class/input without mapping guide
# to hardware Menu (0x138), leaving Menu exclusively to slot.
dpad_stick=0
grep -q '^dpad_stick=1$' "$slot_state" 2>/dev/null && dpad_stick=1

unslot_pad_mapping() {
	sysfs="${1:-/sys/class/input}"
	dev=""
	for d in "$sysfs"/event*; do
		[ -f "$d/device/name" ] || continue
		if [ "$(cat "$d/device/name" 2>/dev/null)" = "ANBERNIC-keys" ]; then
			dev="$d/device"
			break
		fi
	done
	[ -n "$dev" ] || return 0
	key_caps="$(cat "$dev/capabilities/key" 2>/dev/null || true)"
	abs_caps="$(cat "$dev/capabilities/abs" 2>/dev/null || true)"
	awk -v key_line="$key_caps" -v abs_line="$abs_caps" -v dpad_stick="$dpad_stick" '
	function has_bit(words, nw, code,   w, b, idx, word, nib, bit, pos, ch, val) {
		w = int(code / 64)
		b = code % 64
		idx = nw - w
		if (idx < 1) return 0
		word = words[idx]
		nib = int(b / 4)
		bit = b % 4
		pos = length(word) - nib
		if (pos < 1) return 0
		ch = tolower(substr(word, pos, 1))
		val = index("0123456789abcdef", ch) - 1
		if (val < 0) return 0
		return int(val / (2 ^ bit)) % 2 == 1
	}
	BEGIN {
		nk = split(key_line, kw, /[[:space:]]+/)
		na = split(abs_line, aw, /[[:space:]]+/)
		n = 0
		for (c = 288; c <= 767; c++) {
			if (has_bit(kw, nk, c)) btn[c] = n++
		}
		for (c = 256; c < 288; c++) {
			if (has_bit(kw, nk, c)) btn[c] = n++
		}
		mapping = "default,ANBERNIC-keys"
		split("a b x y leftshoulder rightshoulder back start leftstick lefttrigger righttrigger rightstick", names, " ")
		split("304 305 307 306 308 309 310 311 313 314 315 316", codes, " ")
		for (i = 1; i <= 12; i++) {
			c = codes[i] + 0
			if (c in btn) mapping = mapping "," names[i] ":b" btn[c]
		}
		sticks = 0
		for (c = 0; c < 64; c++) {
			if (c >= 16 && c <= 23) continue
			if (has_bit(aw, na, c)) sticks++
		}
		has_hat = has_bit(aw, na, 16) && has_bit(aw, na, 17)
		if (dpad_stick == 1 && sticks < 2) {
			if (has_hat) {
				mapping = mapping ",-lefty:h0.1,+lefty:h0.4,-leftx:h0.8,+leftx:h0.2"
			} else {
				if (544 in btn) mapping = mapping ",-lefty:b" btn[544]
				if (545 in btn) mapping = mapping ",+lefty:b" btn[545]
				if (546 in btn) mapping = mapping ",-leftx:b" btn[546]
				if (547 in btn) mapping = mapping ",+leftx:b" btn[547]
			}
		} else {
			if (has_hat) {
				mapping = mapping ",dpup:h0.1,dpdown:h0.4,dpleft:h0.8,dpright:h0.2"
			} else {
				split("dpup dpdown dpleft dpright", dnames, " ")
				for (i = 0; i < 4; i++) {
					c = 544 + i
					if (c in btn) mapping = mapping "," dnames[i + 1] ":b" btn[c]
				}
			}
		}
		if (sticks >= 2) mapping = mapping ",leftx:a0,lefty:a1"
		if (sticks >= 4) mapping = mapping ",rightx:a2,righty:a3"
		print mapping ",platform:Linux,"
	}'
}

if grep -qs '^ANBERNIC-keys$' /sys/class/input/event*/device/name 2>/dev/null; then
	pad_cfg="$(unslot_pad_mapping)"
	[ -n "$pad_cfg" ] && export SDL_GAMECONTROLLERCONFIG="$pad_cfg"
fi

# slot drives PPSSPP itself (crates/slot/src/standalone.rs): libunslot-ppsspp.so hands it each
# frame, its sound and the pad over UNSLOT_BRIDGE, so slot's volume and menu are its own, and
# presses F2 and F4 for slot's saves and loads, which controls.ini binds alongside any of the
# player's own.
if ! grep -q '^Save State[[:space:]]*=.*1-132' "$sys/controls.ini" 2>/dev/null ||
	! grep -q '^Load State[[:space:]]*=.*1-134' "$sys/controls.ini" 2>/dev/null; then
	awk '
/^Save State[[:space:]]*=/ { s = 1; if ($0 !~ /[= ,]1-132(,|$)/) $0 = $0 ",1-132" }
/^Load State[[:space:]]*=/ { l = 1; if ($0 !~ /[= ,]1-134(,|$)/) $0 = $0 ",1-134" }
{ print }
END {
	if (!s) print "Save State = 1-132"
	if (!l) print "Load State = 1-134"
}' "$sys/controls.ini" > "$sys/controls.ini.tmp" && mv "$sys/controls.ini.tmp" "$sys/controls.ini"
fi
cd "$here"
if [ -n "${SLOT_RESUME_STATE:-}" ] && [ -f "$SLOT_RESUME_STATE" ]; then
	LD_LIBRARY_PATH="$here/libs" LD_PRELOAD="$here/libs/libunslot-ppsspp.so" SDL_LINUX_JOYSTICK_CLASSIC=1 \
		exec "$here/PPSSPPSDL" --fullscreen --state="$SLOT_RESUME_STATE" "$1"
else
	LD_LIBRARY_PATH="$here/libs" LD_PRELOAD="$here/libs/libunslot-ppsspp.so" SDL_LINUX_JOYSTICK_CLASSIC=1 \
		exec "$here/PPSSPPSDL" --fullscreen "$1"
fi
