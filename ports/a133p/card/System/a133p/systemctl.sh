#!/bin/sh
# The systemctl calls slotbt.sh makes, for TrimUI's firmware, which starts bluetoothd with its
# own script rather than systemd.
case "${1:-} ${2:-}" in
"start bluetooth" | "stop bluetooth") exec /etc/bluetooth/bluetoothd "$1" ;;
esac
exit 0
