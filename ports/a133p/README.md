# unslot on TrimUI's A133P handhelds

> [!WARNING]
> This port is being brought up. The Brick Pro boots with the correct screen orientation,
> an uncropped menu layout, working Wi-Fi and Bluetooth; the Smart Pro and Brick have not been tried. If you try it,
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
Linux 4.9. The tested Brick Pro uses an XRadio XR829 for Wi-Fi and Bluetooth. TrimUI's 2023 SDK
has glibc 2.23; the Brick Pro's firmware 1.1.1 (July 2026) has 2.33. NextUI calls this platform tg5040. The hardware facts
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
- **Screen:** PowerVR EGL with a null window. The Brick and Brick Pro's 4:3 panels use the whole
  720 by 540 menu canvas, scaled to the panel; the Smart Pro's 16:9 panel uses 1.5×, with bars.
- **Backlight** through the brightness ioctl on `/dev/disp`.
- **Rumble** through GPIO 227 and `/sys/class/motor/voltage`, capped at 2.5 V.
- **Wi-Fi** through slot's own `wpa_supplicant` on `wlan0`.
- **Bluetooth** uses the firmware's XRadio attachment, waiting for firmware loading to finish,
  and runs bluetoothd from `/etc/bluetooth/bluetoothd`.

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
| `task core:a133p:extra -- pcsx_rearmed fbneo` | Selected extra cores from pinned upstream sources, using zig. Without arguments, attempts all 47 shipped cores other than mGBA. |
| `task drastic:a133p` | Host-staged DraStic with a source-built PowerVR SDL2 and TrimUI control defaults. The proprietary runner requires glibc 2.27. |
| `task dist:a133p FROM=<tree>` | The card in `dist-a133p`. |

`dist:a133p` starts from an H700 device tree, `FROM`, which defaults to `dist-device`; an unzipped
release `unslot-<version>-h700.zip` works too. It puts in the A133P's own slot, every core built in
`target-a133p/cores/`, and the files the firmware boots through. Run `core:a133p:extra` first;
extra cores are not built automatically by `dist:a133p`.
The builder records each core's source commit and flags in a `.meta` beside its `.so`.
`dist:a133p` checks each replacement's glibc imports before copying it, strips the copy with
`aarch64-linux-gnu-strip` (or `STRIP`), and keeps its metadata in
`System/licenses/`. It writes `Logs/a133p-glibc.txt`, listing files above the SDK's glibc 2.23
baseline. The tested Brick Pro's firmware has glibc 2.33; this report is not a runtime loader test.
GPU recipes need Khronos headers (`GLES2`, `GLES3`, `EGL`, `KHR`) under `/usr/include`, or the
directory set by `KHRONOS_HEADERS`. Place firmware `libGLESv2.so` and `libEGL.so` copies in
`target-a133p/gpu/`, or set `A133P_GPU_LIBS` to their directory. These proprietary libraries are
link inputs only: they are not bundled into cores or copied to the card.

### DraStic host staging

`drastic:a133p` stages the pinned, unmodified proprietary runner and its BIOS replacements in
`target-a133p/drastic/`. Its SDL2 is built from Zlib-licensed
[SDL2_trimui](https://github.com/Ninoh-FOX/SDL2_trimui/tree/5d7213995373e1efaf2dc59571a561fae7f6be96),
using PowerVR's null native EGL window and the framebuffer's reported size, not H700 Mali pixmaps.
`dist:a133p` overlays this staging directory when present and keeps `drastic.meta` in
`System/licenses/`. Existing `Saves/NDS/config` is not overwritten.

The closed runner imports GLIBC 2.27 and GLIBCXX 3.4: unlike rebuilt libraries, it cannot meet
the SDK's glibc 2.23 baseline. It fits the tested firmware's glibc 2.33, but older firmware is not
covered. Firmware must supply zlib, ALSA, libstdc++, libgcc and the complete PowerVR runtime.
SDL and the volume preload import at most GLIBC 2.17. Their ELF checks and SDL symbol closure
pass on the host; display, sound, controls and the runner itself have not been tested on-device
in this continuation.

### Nodosumi host verification

The pinned `984de9dfdbffdd5fd38296f5ed71356a5778c934` build applies
`tools/a133p/nodosumi-gles.patch` and records its hash. It uses the GLES loader and the GLES 3.0
`glFramebufferTexture2D` attachment, not the desktop loader and an ES 3.2-only call.
Unbuffered startup diagnostics report the loader result and GPU vendor, renderer and version.

Mesa GLES 3.0 reproduced a null-function crash before the patch; the patched real renderer
passes an 8×8 red upload/blit check. Native libretro initialization, load, context reset/destroy,
unload and deinitialization also pass without executing game frames.

A later device trace confirms a **separate frontend launch failure**:
`/lib/ld-2.33.so: unrecognized option --core-host`, before core initialization. Launching slot
through `/lib/ld-linux-aarch64.so.1` makes `current_exe()` resolve to `ld-2.33.so`; the old
`ld-linux`-only recognizer omitted slot's ELF operand and passed `--core-host` to the loader.
The shared self-execution helper now retains the resolved loader and places the original
`argv[0]` ELF before mode flags for GPU-core and DraStic menu children. A real host
versioned-loader subprocess regression and the A133P frontend build pass.

The GLES defect and launcher failure are independently evidenced. Both fixes have since been put
on the Brick Pro; PowerVR rendering, A64 JIT execution and gameplay remain unverified.

For the renderer check, set `SOURCE` to a patched native checkout with its native core built in
`build-host/`; Mesa EGL development libraries are required:

```sh
cc -I "$SOURCE/deps/glad/include" -I "$SOURCE/src" \
  tools/a133p/nodosumi-gles-smoke.c -o /tmp/nodosumi-gles-smoke -lEGL -ldl
LIBGL_ALWAYS_SOFTWARE=1 MESA_GLES_VERSION_OVERRIDE=3.0 \
  /tmp/nodosumi-gles-smoke "$SOURCE/build-host/nodosumi_libretro.so"
```

### Profiling on the device

`A133P_PROFILE=1 sh tools/a133p/gpu-cores.sh nodosumi` builds the release recipe with
`-g -fno-omit-frame-pointer` into `target-a133p/cores-profile/`, with its own `.meta`. zig's clang
already emits both for AArch64, so its code, read-only data and data sections match the release
core byte for byte; only the debug info's build paths differ.

An empty `System/trace-ftrace.on` makes `launch_frontend.sh` record at each slot start:

| File | What it holds |
|---|---|
| `Logs/ftrace/slot-trace.bin` (`.names`, `.threads`) | slot's and its core host's frame path: `SLOT_FTRACE`. |
| `Logs/ftrace/nodo-trace.bin` (`.names`) | Nodosumi's emu, video and worker threads: `NODOSUMI_FTRACE`. Each game start truncates it. |
| `Logs/slot.log` | `slot: pace:` every 600 presents and `slot: latency:` lines: `SLOT_TRACE_LATENCY`. |

A start after one that ran a core keeps the earlier traces and their `slot.log` in
`Logs/ftrace.1`. `System/trace-ftrace-fine.on` adds `NODOSUMI_FTRACE_FINE`, which also times SH4
interpreter runs and TA polygon writes. Decode slot's trace with `tools/ftrace_dump.py`, and both
together with Nodosumi's `tools/ftrace/analyze.py --core nodo-trace.bin --slot slot-trace.bin`.
Remove the flag files and start slot again to stop tracing.

## Known limits

- **Run on a Brick Pro, firmware 1.1.1.** Screen orientation is correct, menu cropping is fixed,
  and the buttons drive the menus. The user confirmed Wi-Fi connectivity and working Bluetooth.
  SSH and SFTP use the stock firmware's server. Smart Pro and Brick remain untested.
- **Core compatibility is not gameplay verification.** An earlier bring-up phase deployed fifteen
  replacement cores: PCSX ReARMed, FBNeo, Snes9x, NeoCD, MAME 2003-Plus, DOSBox Pure, PUAE 2021,
  HatariB, Beetle PC-FX, NP2kai, PX68k, Retro8, TIC-80, Beetle VB and PPSSPP. Those passed the
  Brick Pro's runtime loader check, not gameplay testing. Further rebuilding is host-only:
  compilation, ELF imports and software-core API smoke checks do not establish handheld
  loading, rendering, audio, controls or performance. C++ source builds use zig's static libc++;
  Beetle VB is software-only, and PPSSPP uses GLES with bundled PNG/zlib.
  Unreplaced H700 tools can still need unavailable libraries; consult `Logs/a133p-glibc.txt`.
- **Wi-Fi startup:** the firmware's late-starting `wpa_supplicant` fought unslot's for `wlan0`,
  causing authentication failures and a reported reboot. It is stopped for the first minute
  after boot. Wi-Fi Direct, and so Miracast, is unavailable: the stock supplicant has no P2P.
- **Bluetooth startup:** the firmware has no `timeout`, so the port brings its own watchdog
  for `bluetoothctl`; the firmware's `hciattach` is left to finish loading the chip's firmware.
- **Not done yet:**
  - DraStic runtime verification; its PowerVR SDL and runner are host-staged only;
  - the volume of PortMaster ports, which uses the H700's codec control;
  - headphone detection;
  - TrimUI's RGB LEDs, which stay off;
  - deep sleep;
  - a release, and a CI job to build one.
- **Pak updates** need `unzip`, which the Brick Pro's firmware does not have. Its BusyBox `sort` has
  no `-V`, so a plain sort picks the pack.
