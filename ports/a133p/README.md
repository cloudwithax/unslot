# unslot on TrimUI's A133P handhelds

> [!WARNING]
> This port has not been run on a TrimUI handheld yet. Everything below is what it is built to do,
> not what has been seen to work. If you try it, [open an issue](https://github.com/cloudwithax/unslot/issues)
> with `Logs/a133p.log` and `Logs/slot.log`.

The port lives on the `a133p` branch. It runs unslot on TrimUI's stock firmware, in place of
TrimUI's frontend, with no BaseOS under it.

## Devices

| Model | Screen | Controls |
|---|---|---|
| TrimUI Smart Pro | 1280 by 720 | two sticks |
| TrimUI Brick | 1024 by 768 | no sticks; two front keys |
| TrimUI Brick Pro | 1024 by 768 | two sticks, L3 and R3, F1 and F2, Home |

All three are Allwinner A133P with a PowerVR GE8300 GPU, on TrimUI's stock firmware (glibc 2.23,
Linux 4.9). NextUI calls this platform tg5040. The hardware facts the port uses, its GPIOs, mixer
controls, key codes and ioctls, were taken from [NextUI](https://github.com/LoveRetro/NextUI). No
NextUI code is used: NextUI is under the PolyForm Noncommercial licence.

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
newer glibc than 2.23.

## Known limits

- **Untested on hardware.** Nothing here has run on a TrimUI handheld yet.
- **Most cores and tools will not load yet.** They come from the H700 build and need glibc 2.29 to
  2.38: about 60 of 97 files, among them the buildbot cores such as FCEUmm, Snes9x, PCSX ReARMed and
  FinalBurn Neo, Flycast, PPSSPP, DraStic's SDL, PortMaster's libraries and `zip`. They will not
  load on the stock firmware until they are rebuilt. The A133P's own slot and mGBA, and gpSP, are
  built for 2.23. `Logs/a133p-glibc.txt` lists the rest.
- **Not done yet:**
  - DraStic, which is the H700's SDL and Mali build;
  - the volume of PortMaster ports, which uses the H700's codec control;
  - headphone detection;
  - TrimUI's RGB LEDs, which stay off;
  - deep sleep;
  - SSH, which on the H700 is BaseOS's dropbear;
  - a release, and a CI job to build one.
- **Pak updates** need `unzip` and `sort -V` on the firmware, which has not been checked.
