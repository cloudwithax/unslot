# unslot on TrimUI's A133P handhelds

> [!WARNING]
> This port is being brought up. It has booted on a Brick Pro, where Wi-Fi and Bluetooth do not
> work yet; the Smart Pro and Brick have not been tried. If you try it,
> [open an issue](https://github.com/cloudwithax/unslot/issues) with the files in `Logs`.

The port lives on the `a133p` branch. It runs unslot on TrimUI's stock firmware, in place of
TrimUI's frontend, with no BaseOS under it.

## Devices

| Model | Screen | Controls |
|---|---|---|
| TrimUI Smart Pro | 1280 by 720 | two sticks |
| TrimUI Brick | 1024 by 768 | no sticks; two front keys |
| TrimUI Brick Pro | 1024 by 768 | two sticks, L3 and R3, F1 and F2, Home |

All three are Allwinner A133P with a PowerVR GE8300 GPU, on TrimUI's stock firmware: Tina Linux on
Linux 4.9, with an XRadio XR829 for Wi-Fi and Bluetooth. TrimUI's 2023 SDK has glibc 2.23; the Brick
Pro's firmware 1.1.1 (July 2026) has 2.33. NextUI calls this platform tg5040. The hardware facts
the port uses, its GPIOs, mixer controls, key codes and ioctls, were taken from
[NextUI](https://github.com/LoveRetro/NextUI). No NextUI code is used: NextUI is under the
PolyForm Noncommercial licence.

## Install

Build the card with `task dist:a133p` (see [Building](#building)); it goes in `dist-a133p/`. Copy
everything in it, the hidden `.tmp_update` folder too, to the root of a FAT32 or exFAT card, and
boot the handheld with it.

On the first boot the stock firmware runs the card's `trimui/app/MainUI` in place of its own
frontend. That script:

1. renames the firmware's `/usr/trimui/bin/runtrimui.sh` to `runtrimui-original.sh`;
2. puts a hook in its place, which on every boot after runs the card's `.tmp_update/updater`, or
   the stock frontend if the card has none;
3. deletes the card's `trimui/` folder;
4. starts unslot.

A firmware a NextUI card has already hooked is left as it is: the same hook runs unslot's updater.

## Going back to stock

Take `.tmp_update/updater` off the card. The hook then starts the stock frontend. Or put a NextUI
card back in, which boots through the same hook.

## How it boots

`.tmp_update/updater` runs `System/a133p/launch.sh`, which sets the handheld up once, as BaseOS
does on the H700:

- reads the model from `strings /usr/trimui/bin/MainUI`, and exports `BASEOS_TARGET` as
  `smartpro`, `brick` or `brickpro`, and `UNSLOT_CHIP=a133p`;
- mounts a tmpfs on `/run`;
- sets the GPIOs: 107 for the 5 V rail, 227 for the rumble motor, and 110 and 114 for the Smart
  Pro's pads;
- turns TrimUI's LEDs off;
- starts `trimui_inputd`;
- sets the codec to its loudest, with the speaker on;
- sets the CPU governor to performance;
- stops the firmware's `wpa_supplicant`.

It then runs `System/launch_frontend.sh` in a loop, as BaseOS's init does, so slot comes back after
a reload, a port or a DS game.

## In slot

- **Buttons** come from the pad `trimui_inputd` makes, TRIMUI Player1, mapped by the letters
  printed on them. L2 and R2 are its analog triggers. The Brick Pro's Home acts as Menu. The
  Brick's front keys act as L3 and R3. The Brick Pro's F1 and F2 are not mapped yet.
- **Sticks** on the Smart Pro and the Brick Pro, with Left Stick as D-Pad available.
- **Screen:** PowerVR EGL with a null window. The Brick and Brick Pro's 4:3 panels are laid out as
  the 640 by 480 Anbernics are; the Smart Pro's 16:9 panel at 1.5×, with bars.
- **Backlight** through the brightness ioctl on `/dev/disp`.
- **Rumble** through GPIO 227 and `/sys/class/motor/voltage`, capped at 2.5 V.
- **Wi-Fi** through slot's own `wpa_supplicant` on `wlan0`.
- **Bluetooth** attaches the XRadio chip with `hciattach -n ttyS1 xradio`, and runs bluetoothd from
  `/etc/bluetooth/bluetoothd`.

## Updates

A133P builds look for `unslot-<version>-a133p.pak`, not the H700's `unslot-<version>.pak`. None is
published yet, so Settings → Online → Updates finds nothing.

`tools/update/pack.sh TREE OUT.zip a133p` makes an A133P pack. The installer,
`card/update/install.sh`, goes in each pack as `<chip>.sh`; it installs only packs for its own chip,
and leaves `.tmp_update/updater` alone.

## After a first boot

Look at these on the card:

| File | What it says |
|---|---|
| `Logs/a133p.log` | What `launch.sh` did: the firmware's version, the model it found, and any errors from setting up. |
| `Logs/slot.log` | slot's own log. |
| `Logs/a133p-diag.txt` | What the firmware has for Wi-Fi, Bluetooth and the screen: its tools, daemons, modules, rfkill switches, init scripts and kernel log, written at each boot. |
| `Logs/wpa_supplicant.log` | The supplicant's debug output, without keys. |
| `Logs/radio-trace.log` | Every step `slotwifi.sh` and `slotbt.sh` took. |
| `Logs/dmesg.txt` | The kernel's log, written every 20 seconds. |

The last four are there while the port is brought up. `wpa_supplicant.log`, `radio-trace.log` and
`dmesg.txt` keep the boot before as `.1`.

## Building

Builds use [Task](https://taskfile.dev), and cross-compile against glibc 2.23 through zig.

| Task | What it makes |
|---|---|
| `task build:a133p` | slot for the A133P, with cargo-zigbuild. It needs `cargo install cargo-zigbuild`, and `pip install ziglang` or zig on the `PATH`. |
| `task core:a133p` | mGBA for glibc 2.23, through zig and `tools/a133p/toolchain.cmake`. |
| `task dist:a133p FROM=<tree>` | The card in `dist-a133p`. |

`dist:a133p` starts from an H700 device tree, `FROM`, which defaults to `dist-device`; an unzipped
release `unslot-<version>-h700.zip` works too. It puts in the A133P's own slot and mGBA, and the
files the firmware boots through. It writes `Logs/a133p-glibc.txt`, listing every file that needs a
newer glibc than 2.23, the oldest firmware's.

## Known limits

- **Run on a Brick Pro, firmware 1.1.1.** It boots to the shelf, the screen is the right way up and
  filled, and the buttons drive the menus. Wi-Fi and Bluetooth are still being brought up, and the
  Smart Pro and Brick have not been tried.
- **Some cores and tools need a newer glibc.** They come from the H700 build. On the Brick Pro's
  glibc 2.33, 14 files will not load: DOSBox Pure, FinalBurn Neo, Hatari, Beetle PC-FX, Neko
  Project II, PCSX ReARMed, PUAE, px68k, Retro8, MAME 2003-Plus, TIC-80, PortMaster's OpenAL and
  `astcenc`, and `zip`. On older firmware with glibc 2.23, about 60 files will not, listed in
  `Logs/a133p-glibc.txt`. The A133P's own slot and mGBA, and gpSP, load on either.
- **Wi-Fi is not done.** The firmware starts its own `wpa_supplicant` at boot, which fought unslot's
  for `wlan0`; it is now stopped for the first minute after boot. The XR829 driver has also logged
  a kernel warning while failing to authenticate to one network, and a join has restarted the
  handheld. Wi-Fi Direct, and so Miracast, is not there: the firmware's supplicant is built without
  P2P.
- **Bluetooth is not done.** The firmware has no `timeout`, which every `bluetoothctl` call ran
  under, so the port brings its own; the firmware's own `hciattach` is left to finish loading the
  chip's firmware.
- **Not done yet:**
  - DraStic, which is the H700's SDL and Mali build;
  - the volume of PortMaster ports, which uses the H700's codec control;
  - headphone detection;
  - TrimUI's RGB LEDs, which stay off;
  - deep sleep;
  - SSH, which on the H700 is BaseOS's dropbear;
  - a release, and a CI job to build one.
- **Pak updates** need `unzip`, which the Brick Pro's firmware does not have. Its BusyBox `sort` has
  no `-V`, so a plain sort picks the pack.
