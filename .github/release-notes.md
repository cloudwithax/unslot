## Changes

- **Dreamcast (Nodosumi v1.4.0)**: ships Nodosumi 1.4.0 for H700 (as well as A133P), adding A64 JIT first-target indirect branch prediction, 1 ms ARM7 sound CPU batching, and GLES 3.0 entry-point/FBO fixes.
- **Audio pacing & rate control**: output thread paces writes on deep ALSA dmix rings so the queue does not drain and trigger sharp pitch/warble, and the rate controller drains a nearly full queue faster (`Level::FULL_MIN`) without holding up the next frame.
- **4:3 screen layout**: 640×480 and 1024×768 4:3 panels keep the full 720-wide shelf and menu layout with room above and below instead of cropping the sides.
- **Zero-copy GPU frames & TrimUI A133P port**: imports shared ION/DMA-BUF GPU frames directly as EGL images when supported (falling back cleanly to copy on H700 Mali), plus experimental TrimUI A133P (Brick Pro / Brick / Smart Pro) support (`task dist:a133p`), bundled Mozilla CA certificates (`System/ssl/ca-certificates.crt`) for firmwares without `/etc/ssl/certs`, and paused GPU swaps while asleep (`Doze`).

Verified on TrimUI Brick Pro (Nodosumi v1.4.0 / Crazy Taxi 2); H700 targets inherit Nodosumi v1.4.0, the 4:3 layout fix, and the audio pacing/rate-control updates and are covered by CI checks and host unit tests.

## New card: flash the image

The `.img.zip` files are images of a whole card: BaseOS, the small Linux these handhelds start from, with unslot already set up on it. **Flashing one erases everything on the card.**

1. **Pick the image for your handheld.** It is at the bottom of this page under Assets; you need a few minutes after the release for them all to appear.

   | Handheld | Image |
   |---|---|
   | RG28XX | `unslot-$TAG-rg28xx.img.zip` |
   | RG34XX | `unslot-$TAG-rg34xx.img.zip` |
   | RG34XX SP | `unslot-$TAG-rg34xxsp.img.zip` |
   | RG35XX Plus, RG35XX 2024 | `unslot-$TAG-rg35xxplus.img.zip` |
   | RG35XX H | `unslot-$TAG-rg35xxh.img.zip` |
   | RG35XX Pro | `unslot-$TAG-rg35xxpro.img.zip` |
   | RG35XX SP | `unslot-$TAG-rg35xxsp.img.zip` |
   | RG40XX H | `unslot-$TAG-rg40xxh.img.zip` |
   | RG40XX V | `unslot-$TAG-rg40xxv.img.zip` |
   | RG CubeXX | `unslot-$TAG-rgcubexx.img.zip` |
   | RG SP | `unslot-$TAG-rgsp.img.zip` |

2. **Flash it to a microSD card** with [balenaEtcher](https://etcher.balena.io) or [Raspberry Pi Imager](https://www.raspberrypi.com/software/) (choose *Use custom* and pick the file). Both take the `.img.zip` as it is. Any other flasher works too once you unzip it to the `.img` inside. Any size of card will do: the first start grows unslot's space to fill it.
3. **Put the card in TF1**, the slot your handheld starts from, and switch it on. The first start takes a minute or two while the card is set up, then unslot comes up.
4. **Add your games over Wi-Fi.** The image's unslot space is ext4, not FAT, so a reset in the middle of a copy cannot corrupt it. Windows and macOS cannot open it, and Windows may offer to format it: say no. Instead, in Settings → Network → Services, turn on File sharing and open `\\<handheld's address>` (Windows) or `smb://<handheld's address>` (macOS), or turn on Syncthing and share folders with it. Each system goes in its own folder (`Games/GBA`, `Games/SNES`, `Games/PSP` and so on, all listed in the [README](https://github.com/cloudwithax/unslot#systems)), and BIOS files in `BIOS`. Names must match in case: `Games/GBA`, not `games/gba`.

## Already running unslot

Settings → Online → Updates downloads `unslot-$TAG.pak` and installs it when you restart. Or copy `unslot-$TAG.pak` to the root of the card yourself and restart with the power connected. unslot installs it only if it is a signed unslot release; your games, saves and settings stay.

## Already running BaseOS, or slot

- **slot:** copy `MinUI.zip` to the root of the card and restart with the power connected. BaseOS swaps slot for unslot before it starts, and your games, saves and settings stay.
- **BaseOS with another frontend:** copy the contents of the `unslot-$TAG` folder in `unslot-$TAG-h700.zip` to the root of its `BASEOS` drive, or of a card in TF2.

---

unslot is a fork of [slot](https://github.com/BrandonKowalski/slot) by Brandon T. Kowalski. Emulation by mGBA (MPL-2.0), gpSP (GPL-2.0) and libretro cores; every licence, and the source of the cores built for unslot, is in `System/licenses/`.
