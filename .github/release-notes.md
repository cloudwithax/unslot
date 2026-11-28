## Changes

- **Playtime & RetroShelf**: automatic local playtime tracking across every system (shown on the shelf and in a new **Playtime** screen with sort modes and jump-to-game), plus optional QR-linked sync with [RetroShelf](https://retroshelf.org).
- **Dedicated Built-in App Screens & NES-Grey App Carts**: **Files** (full file manager with copy/cut/paste/rename/delete/new folder), **Gallery** (4×3 thumbnail grid + video player bar), **Books & Manuals** (6×2 cover shelf + paper reader) and **Music** (now-playing pane with cover art & tags), with redesigned NES-grey icon carts on the Apps shelf.
- **TrimUI A133P S3 Deep Sleep + RTC Auto Power-Off**: hardware Suspend-to-RAM (`mem`) with `/sys/class/rtc/rtc0/wakealarm` timed save-and-power-off, instant Power-button wake, and `xradio_wlan` quiesce retry.
- **Flicker-Free, Fast PSP & DS**: fixed PowerVR `/dev/fb0` flashing in PPSSPP via offscreen `EGLPbufferSurface`, moved PPSSPP & DraStic savestate exchange to RAM tmpfs, and added direct `--state=` preloading on PPSSPP (cutting Brick Pro PSP launch from ~4.4 s to ~1.3 s and exit from ~3.5 s to ~0.44 s).
- **Exact Content Frame Rate & Hide Any Shelf**: every core runs at its exact native refresh rate by default (fixing fast/sharp audio), and any shelf (including Apps and Ports) can now be hidden or shown with `X` under Settings → Screen.
- **Fixes & Improvements**: PlayStation memory cards fixed (#3), PICO-8 30 FPS input latching & Splore fixes (#6), overlays packaging/deploy checks (#7), configurable ScreenScraper artwork `region=` (#8), Upload from Phone any file/folder + Sort for Me, iOS/iPadOS writable Samba shares, PortMaster launcher hardening, and hung-core recovery.

The full list is in [CHANGELOG.md](https://github.com/cloudwithax/unslot/blob/$TAG/CHANGELOG.md).

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

## TrimUI A133P (Brick Pro, Brick, Smart Pro)

- **New card:** unzip `unslot-$TAG-a133p.zip` and copy everything in its `unslot-$TAG-a133p` folder, the hidden `.tmp_update` folder too, to the root of a FAT32 or exFAT card, then boot the handheld with it. It runs on TrimUI's stock firmware in place of its own frontend.
- **Already running unslot:** Settings → Online → Updates installs `unslot-$TAG-a133p.pak`.

## Already running BaseOS, or slot

- **slot:** copy `MinUI.zip` to the root of the card and restart with the power connected. BaseOS swaps slot for unslot before it starts, and your games, saves and settings stay.
- **BaseOS with another frontend:** copy the contents of the `unslot-$TAG` folder in `unslot-$TAG-h700.zip` to the root of its `BASEOS` drive, or of a card in TF2.

---

unslot is a fork of [slot](https://github.com/BrandonKowalski/slot) by Brandon T. Kowalski. Emulation by mGBA (MPL-2.0), gpSP (GPL-2.0) and libretro cores; every licence, and the source of the cores built for unslot, is in `System/licenses/`.
