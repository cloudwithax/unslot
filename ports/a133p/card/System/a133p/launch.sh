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

# gpio N in|out [VALUE]
gpio() {
	[ -d "/sys/class/gpio/gpio$1" ] || echo "$1" > /sys/class/gpio/export
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

# slot runs Wi-Fi itself, so the firmware's supplicant and lease go.
/etc/init.d/wpa_supplicant stop > /dev/null 2>&1
killall udhcpc 2> /dev/null

# What slotwifi.sh and slotbt.sh call on BaseOS, where TrimUI's firmware has something else.
command -v rfkill > /dev/null 2>&1 || export AGS_RFKILL="/bin/sh $HERE/rfkill.sh"
export AGS_SYSTEMCTL="/bin/sh $HERE/systemctl.sh"
export AGS_SETBT="$HERE/bluetooth.sh"
export AGS_HCIATTACH=hciattach

while :; do
	/bin/sh "$SYS/launch_frontend.sh"
	# A slot that cannot start at all is not started again and again without a pause.
	sleep 1
done
