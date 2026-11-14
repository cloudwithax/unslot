## Changes

- Frame Blending on each system's Screen page and in-game: Mix averages consecutive frames, and Ghosting follows the Game Boy family's LCD response. GPU blending works under every core.
- Saturation and Contrast, under General and on the in-game Screen page, plus six software dim steps below the backlight's lowest brightness.
- Left Stick as D-Pad on handhelds with a left stick, on by default. Fast Forward gains 1.5× and Off, with 6× the default. Battery Display can show the icon, percentage, or both.
- A game that crashes while loading no longer loops at boot: unslot starts on the shelf and moves its resume state aside. RetroArch compressed saves load correctly, and unreadable saves are not overwritten.
- Crashed runs' temporary files are cleared at boot. Shutdown syncs the card around a bounded Wi-Fi unload and releases the log; audio opens only when needed and closes before power-off or restart.
- Gambatte boots the card's Game Boy BIOS when present. Upstream mGBA fixes cover MBC2 saves, flash-bank switching, centred Game Boy audio and the first frame after the LCD comes on.
- Nintendo DS, in `Games/NDS`, run by DraStic. No BIOS needed; saves, states and settings stay in `Saves/NDS`. DraStic's menu is on Select + X.
- Menu in a DS game opens unslot's menu over it (Resume, Exit Game), and the volume keys and mute work as in any other game.
- DS games show on the shelf as DS Game Cards, and Game Art fetches their art.
- Letter jumps stay smooth on shelves of thousands of games, and no longer wait for the labels they land on.
- Ejecting a game, or its core dying, closes its Core Options and Controls pages.
- Offline achievements survive switching between softcore and hardcore, and offline hardcore unlocks stay earned.
- With several update packs on the card, the newest version installs, not the last by name.
- Update packs are checked and unpacked a piece at a time, so larger future updates still install on 1 GB handhelds.
- rcheevos's MIT notice, DraStic's readme and SDL's licence now ship in `System/licenses`.

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
