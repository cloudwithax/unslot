<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/brand/unslot-dark.svg">
  <img src="assets/brand/unslot-light.svg" width="420" alt="unslot.">
</picture>

**A skeuomorphic, multi-system frontend for Anbernic's H700 handhelds, running on [BaseOS](https://github.com/pvaibhav/BaseOS).**

A fork of [slot](https://github.com/BrandonKowalski/slot) that takes its cart shelves past the Game Boy:
thirteen systems, Wi-Fi and Bluetooth, analog sticks, network services and updates over the air.

<a href="LICENSE"><img src="https://img.shields.io/github/license/cloudwithax/unslot?style=for-the-badge&labelColor=242429&color=4d4d57" height="32" alt="License"></a>
<a href="https://github.com/cloudwithax/unslot/releases"><img src="https://img.shields.io/github/v/release/cloudwithax/unslot?style=for-the-badge&labelColor=242429&color=e8473f" height="32" alt="Release"></a>

</div>

<p align="center">
<img src=".github/resources/screenshots/unslot/shelf-pc-engine.png" width="49%" alt="PC Engine HuCards on the shelf over a pixel-art city">
<img src=".github/resources/screenshots/unslot/shelf-playstation.png" width="49%" alt="A PlayStation disc on the shelf, its system named above the slot">
</p>

---

## What it does

- **Thirteen systems**, each on its own shelf as the media it came on: carts, HuCards and discs.
- **Wi-Fi** with an on-screen keyboard, and **Bluetooth** controllers and headphones.
- **Analog sticks** on the models that have them.
- **Services:** SSH and SFTP, sharing the card over SMB, Tailscale (sign in by scanning a code) and WireGuard.
- **Updates over the air**, for unslot and for BaseOS under it, with an automatic rollback if a new version never starts.
- **Game Art** finds a label for each game online, matched by CRC and file name.
- **Wallpapers:** a bundled set, or any PNG or JPG of your own.

Everything slot does is still here: the carts, the shelves, the link cable, the in-game menu and the save handling.

## Install

unslot runs on BaseOS, so it needs BaseOS on the card in TF1 first.

**New card.** Flash your model's BaseOS image to TF1 and boot it once; it grows its `BASEOS` volume to fill the card. Then copy the contents of a release's `unslot-<version>` folder onto that volume (or onto a card in TF2), add games and restart.

**Already running slot?** Copy the release's `MinUI.zip` to the root of the card and restart with the power connected. BaseOS installs it before the frontend starts: `System/` is replaced, and your games, saves, labels and settings stay. This is also how unslot updates, and Settings → Network → Updates can download it for you.

> [!IMPORTANT]
> ROMs must be unzipped. unslot does not read `.zip` or `.7z` files.

## Systems

| System | Folder | Files | Core | BIOS |
|---|---|---|---|---|
| Game Boy Advance | `GBA` | `.gba` | mGBA, gpSP | `gba_bios.bin` (optional) |
| Game Boy | `GB` | `.gb`, `.gbc` | mGBA | |
| Game Boy Color | `GBC` | `.gb`, `.gbc` | mGBA | |
| Nintendo Entertainment System | `NES` | `.nes`, `.unf`, `.unif` | FCEUmm | |
| Super Nintendo | `SNES` | `.sfc`, `.smc` | Snes9x | |
| Sega Genesis / Mega Drive | `MD` | `.md`, `.gen`, `.bin`, `.smd` | Genesis Plus GX | |
| Sega Master System | `SMS` | `.sms` | Genesis Plus GX | |
| Sega Game Gear | `GG` | `.gg` | Genesis Plus GX | |
| PC Engine / TurboGrafx-16 | `PCE` | `.pce` | Beetle PCE Fast | |
| Neo Geo Pocket (Color) | `NGP` | `.ngp`, `.ngc` | RACE | |
| WonderSwan (Color) | `WS` | `.ws`, `.wsc` | Beetle Cygne | |
| Atari Lynx | `LYNX` | `.lnx` | Handy | `lynxboot.img` (required) |
| PlayStation | `PS` | `.cue`, `.chd`, `.pbp`, `.m3u` | PCSX ReARMed | `scph5501.bin` etc. (optional) |

## On the card

| Folder | What goes there |
|---|---|
| `Games/<Folder>` | Your games, by the folder in the table above. A multi-disc game goes in once, as an `.m3u` playlist; swap discs from Gameplay in the game menu. |
| `Labels/<Folder>` | A label for a game, as `<game>.png`. Game Art fills these in for you. |
| `BIOS` | BIOS files. |
| `Wallpapers` | Your own wallpapers, PNG or JPG. |
| `Config/wireguard` | WireGuard tunnels, as wg-quick `.conf` files. |
| `Config/screenscraper.conf` | Optional: a ScreenScraper developer id (`devid=`, `devpassword=`) and your own login (`ssid=`, `sspassword=`), which Game Art then asks first. |
| `System` | unslot itself. Updates replace it whole. |

## A look around

<p align="center">
<img src=".github/resources/screenshots/unslot/network.png" width="49%" alt="The Network page: Wi-Fi, Bluetooth, Services, Updates and Game Art">
<img src=".github/resources/screenshots/unslot/game-art.png" width="49%" alt="Game Art, counting the labelled games on each shelf">
</p>
<p align="center">
<img src=".github/resources/screenshots/unslot/wallpaper.png" width="49%" alt="Picking a bundled wallpaper: the Lagoon Nebula">
<img src=".github/resources/screenshots/unslot/about.png" width="49%" alt="The About page's device sticker">
</p>

These are screenshots from an RG34XX SP.

## Devices

unslot targets every H700 model BaseOS supports. It has been run on the **RG34XX SP**. slot itself supports the RG SP, RG34XX, RG34XX SP, RG35XX SP, RG35XX Plus and RG28XX. If you try unslot on another model, [open an issue](https://github.com/cloudwithax/unslot/issues) and say how it went.

## Building

Builds use [Task](https://taskfile.dev). The device build runs in an arm64 Debian container, natively or through Docker.

| Task | What it makes |
|---|---|
| `task dist:device` | The card tree in `dist-device`. |
| `task dist:update` | The update pack, `MinUI.zip`. |
| `task image:device TARGET=rg34xxsp` | A whole card image to flash to TF1: BaseOS with unslot already on it (Linux only). |
| `task test` | The tests. |

## Credits

unslot is a fork of **[slot](https://github.com/BrandonKowalski/slot)** by Brandon T. Kowalski. Its skeuomorphic art is by **[Markus](https://github.com/MarkusMaribu)**, and the carts, shelves, link cable and most of what you see came from slot.

| Area | Credit |
|---|---|
| Operating system | [BaseOS](https://github.com/pvaibhav/BaseOS) by pvaibhav |
| Emulation | [mGBA](https://mgba.io), [gpSP](https://github.com/libretro/gpsp), and the libretro cores in the systems table |
| Labels | Game Art's style follows [Cart Studio](https://github.com/BrandonKowalski/slot-cart-studio). Names come from [libretro-database](https://github.com/libretro/libretro-database) and art from libretro-thumbnails and [ScreenScraper](https://www.screenscraper.fr). |
| Services | [Tailscale](https://github.com/tailscale/tailscale), [wireguard-go](https://git.zx2c4.com/wireguard-go) and [spruceOS](https://github.com/spruceUI/spruceOS)'s Samba build |
| Wallpapers | NASA, ESA, CSA and STScI; Poly Haven; ambientCG; and Luis Zuno. Each is credited in `System/licenses/wallpapers.txt`. |
| Type | Open Sans, and Nerd Fonts Symbols by Ryan L McIntyre |

Every licence, with gpSP's source, ships in `System/licenses/`.

## AI disclosure

Like slot, unslot is largely written by AI. unslot's additions were written with Claude, directed and tested on hardware by its maintainer. slot's own disclosure, and its author's reasons, are in [slot's README](https://github.com/BrandonKowalski/slot#ai-disclosure).

## Licence

GPL-3.0, as slot. See [LICENSE](LICENSE).
