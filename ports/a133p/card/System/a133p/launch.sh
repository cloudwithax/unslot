#!/bin/sh
# unslot on TrimUI's A133P handhelds, the Smart Pro, the Brick and the Brick Pro, on their stock
# firmware. .tmp_update/updater runs this at boot in place of TrimUI's frontend. It sets the
# handheld up once, as BaseOS does before it starts a frontend, then runs launch_frontend.sh
# again each time slot ends, as BaseOS's init does, so a reload, a port or a DS game comes back
# to slot. The hardware facts here (GPIOs, mixer controls, the model's name) were worked out by
# NextUI's tg5040 platform (LoveRetro/NextUI).
SD=/mnt/SDCARD
SYS="$SD/System"
HERE="$SYS/a133p"
LOGS="$SD/Logs"
mkdir -p "$LOGS"
[ "$(wc -c < "$LOGS/a133p.log" 2> /dev/null || echo 0)" -gt 65536 ] && mv -f "$LOGS/a133p.log" "$LOGS/a133p.log.1"
exec >> "$LOGS/a133p.log" 2>&1
echo "$(date '+%F %T') unslot on $(cat /etc/version 2>/dev/null || echo 'TrimUI firmware')"

# Which of the three this is: TrimUI's frontend carries its model's name.
model="$(strings /usr/trimui/bin/MainUI 2>/dev/null | grep -m 1 '^Trimui')"
case "$model" in
"Trimui Brick Pro") target=brickpro ;;
"Trimui Brick") target=brick ;;
*)
	target=smartpro
	model="Trimui Smart Pro"
	;;
esac
echo "model $model ($target)"

export SLOT_ROOT="$SD"
# slot reads the model from BaseOS's names. No BASEOS_VERSION, so BaseOS updates stay off.
export BASEOS_TARGET="$target"
export BASEOS_MODEL="$model"
# The update packs this card takes, unslot-<version>-a133p.pak.
export UNSLOT_CHIP=a133p
export PATH="$SYS/bin:/usr/trimui/bin:$PATH"
export LD_LIBRARY_PATH="/usr/trimui/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

# slot and its scripts keep their pids and sockets in /run, which is in memory on BaseOS.
grep -q ' /run ' /proc/mounts || { mkdir -p /run && mount -t tmpfs -o mode=0755,nosuid,nodev tmpfs /run; }

# gpio N in|out [VALUE]. A pin the kernel already holds (PD11 on the Brick Pro) is left to it.
gpio() {
	[ -d "/sys/class/gpio/gpio$1" ] || echo "$1" > /sys/class/gpio/export 2> /dev/null
	[ -d "/sys/class/gpio/gpio$1" ] || return 0
	echo "$2" > "/sys/class/gpio/gpio$1/direction"
	[ -z "${3:-}" ] || echo "$3" > "/sys/class/gpio/gpio$1/value"
}
gpio 107 out 1 # PD11: the 5 V rail
gpio 227 out 0 # PH3: the rumble motor, which slot drives
if [ "$target" = smartpro ]; then
	gpio 110 out 1 # PD14 and PD18: the Smart Pro's left and right pads
	gpio 114 out 1
fi

# TrimUI's own light shows stop; the lights stay off.
/etc/init.d/lcservice stop > /dev/null 2>&1
for f in max_scale max_scale_lr max_scale_f1f2 max_scale_rear; do
	echo 0 > "/sys/class/led_anim/$f" 2> /dev/null
done

# The buttons: trimui_inputd reads them and gives them to the kernel as one pad, TRIMUI Player1.
pidof trimui_inputd > /dev/null || trimui_inputd &

# slot plays at its own volume, so the codec stays at its loudest, the speaker on, and the
# channels the right way round.
echo 0 > /sys/class/speaker/mute 2> /dev/null
amixer -q sset 'Headphone' 0
amixer -q sset 'digital volume' 0
amixer -q cset name='DAC volume' 160
amixer -q sset 'DAC Swap' Off

# Flat out, as BaseOS runs the H700.
for p in /sys/devices/system/cpu/cpufreq/policy*; do
	echo performance > "$p/scaling_governor" 2> /dev/null
done

# slot runs Wi-Fi itself, so the firmware's supplicant and lease go: two supplicants on wlan0
# fight over every handshake, one deauthenticating as soon as the other associates. The
# firmware's is a procd service whose boot script is still retrying `ifconfig wlan0 up` when
# this runs, so it starts after any one-off stop: it is watched for, by its config under
# /etc/wifi, for a minute.
echo "radio daemons at boot: $(ps w 2> /dev/null | grep -E 'wpa_supplicant|udhcpc|hciattach|bluetoothd|bluealsa' | grep -v grep | tr -s ' ' | tr '\n' ';')"
killall udhcpc 2> /dev/null
(n=0
while [ "$n" -lt 60 ]; do
	for pid in $(ps w 2> /dev/null | grep '[w]pa_supplicant.*/etc/wifi/' | awk '{print $1}'); do
		echo "$(date '+%F %T') stopping the firmware's wpa_supplicant ($pid)"
		kill "$pid" 2> /dev/null
	done
	sleep 1
	n=$((n + 1))
done) &

# What slotwifi.sh and slotbt.sh call on BaseOS, where TrimUI's firmware has something else.
command -v rfkill > /dev/null 2>&1 || export AGS_RFKILL="/bin/sh $HERE/rfkill.sh"
export AGS_SYSTEMCTL="/bin/sh $HERE/systemctl.sh"
export AGS_SETBT="$HERE/bluetooth.sh"
export AGS_HCIATTACH=hciattach
# The firmware's own hciattach loads the chip's firmware for some seconds after boot.
export AGS_HCI_WAIT_S=20
# The Brick Pro's firmware has no timeout at all; older BusyBoxes want its seconds after -t.
if ! timeout 1 true 2> /dev/null; then
	if timeout -t 1 true 2> /dev/null; then
		export AGS_TIMEOUT="timeout -t"
	else
		export AGS_TIMEOUT="/bin/sh $HERE/timeout.sh"
	fi
fi

# While the port is brought up: the radio scripts' every step, the supplicant's debug output,
# the kernel's log and what the firmware has, all in Logs, a boot's worth at a time.
export AGS_RADIO_TRACE="$LOGS/radio-trace.log"
export AGS_WPA_SUPPLICANT="/bin/sh $HERE/wpa_supplicant.sh"
export AGS_WPA_LOG="$LOGS/wpa_supplicant.log"
for f in radio-trace.log wpa_supplicant.log dmesg.txt; do
	[ -f "$LOGS/$f" ] && mv -f "$LOGS/$f" "$LOGS/$f.1"
done
# The diagnostics and the dmesg every 5 s run at nice 19, so they give way to everything else: the
# dmesg loop runs through games too, and a Crazy Taxi 2 trace showed its emulator off the CPU and
# slot's drawing slowed every 5.3 s. TrimUI's BusyBox has nice but no renice or taskset.
nice -n 19 /bin/sh "$HERE/diag.sh" "$LOGS/a133p-diag.txt" &
nice -n 19 /bin/sh -c 'while :; do
	sleep 5
	dmesg > "$1/dmesg.txt" 2> /dev/null
done' sh "$LOGS" &

while :; do
	/bin/sh "$SYS/launch_frontend.sh"
	# A slot that cannot start at all is not started again and again without a pause.
	sleep 1
done
