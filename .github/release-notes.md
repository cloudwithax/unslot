## Changes

- **Sleep After** (Settings → Power): the handheld can now go to sleep on its own after 1, 3, 5 or 10 minutes without input, on the shelf, in menus or in games. Sleeping saves the game, as a tap of Power does. It stays awake during a link session, a Gallery video, music playback, casting and downloads. It is Off by default, so nothing changes until you turn it on.
- **Power Off After** (Settings → Power): how long the handheld stays asleep before powering off, from 1 to 30 minutes, or Never. The default is 3 minutes, as before.
- **Hostname** (Settings → Network): rename the handheld on your network (`<name>.local`) with the on-screen keyboard. The name is checked as BaseOS checks it and saved in `baseos.conf`; it takes effect after a restart.
- **Boot Logo** (Settings → General): choose unslot's logo, BaseOS's original, or your own PNG, JPEG or BMP from the new `Bootlogos/` folder. A picture larger than the screen is scaled down to fit and centred on the colour of its top-left pixel. Upload from Phone has a new Boot logo option that sends a picture straight there.

Verified on RG CubeXX: Sleep After at 1 minute slept after a minute without input and woke on Power; a hostname with a space was refused and a valid one changed only its line in `baseos.conf`; choosing a picture, uploading one from the phone page, and switching back to unslot's logo each rewrote the boot partition's logo, with the last restoring the original byte for byte. Not verified on hardware: Power Off After actually powering the handheld off, and the new name and logo after a full reboot. Both are covered by tests.

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
