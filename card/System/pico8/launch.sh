#!/bin/sh
# Runs the PICO-8 cart $1, or Splore for the Splore card, in the owner's PICO-8 from BIOS/PICO-8,
# for slot to drive as one of its cores (crates/slot/src/standalone.rs). `slot --pico8` starts
# PICO-8 with libunslot-pico8.so, or libunslot-pico8-64.so for pico8_64, preloaded, and the shim
# hands over each frame, its sound and the pad.
here="$(cd "$(dirname "$0")" && pwd)"
exec /lib/ld-linux-aarch64.so.1 "$here/../slot" --pico8 "$1"
