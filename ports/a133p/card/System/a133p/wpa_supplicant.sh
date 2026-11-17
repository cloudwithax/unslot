#!/bin/sh
# wpa_supplicant as slotwifi.sh starts it, with its debug output kept in Logs/wpa_supplicant.log
# while the A133P's Wi-Fi is brought up. It runs in the background itself in place of -B, so
# what it says reaches the log. Passwords are not in it: the supplicant leaves keys out of its
# debug output unless asked with -K.
LOG="${AGS_WPA_LOG:-/mnt/SDCARD/Logs/wpa_supplicant.log}"
args=""
for a in "$@"; do
	[ "$a" = -B ] || args="$args '$a'"
done
echo "--- $(date '+%F %T') wpa_supplicant$args" >> "$LOG"
eval "set -- $args"
wpa_supplicant "$@" -dd -t >> "$LOG" 2>&1 &
exit 0
