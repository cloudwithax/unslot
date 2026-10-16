<div align="center">

<img src=".github/resources/banner.png" width="100%" alt="slot. A bespoke, Game Boy-centric frontend for the Anbernic RG SP.">

<a href="LICENSE"><img src="https://img.shields.io/github/license/BrandonKowalski/slot?style=for-the-badge&labelColor=242429&color=4d4d57" height="36" alt="License"></a>
<a href="https://github.com/BrandonKowalski/slot/stargazers"><img src="https://img.shields.io/github/stars/BrandonKowalski/slot?style=for-the-badge&labelColor=242429&color=4d4d57" height="36" alt="Stars"></a>
<a href="https://github.com/BrandonKowalski/slot/releases"><img src="https://img.shields.io/github/downloads/BrandonKowalski/slot/total?style=for-the-badge&labelColor=242429&color=4d4d57" height="36" alt="Downloads"></a>
<a href="https://slot-cfw.fyi"><img src="https://img.shields.io/badge/Guide-slot--cfw.fyi-4d4d57?style=for-the-badge&labelColor=242429" height="36" alt="Guide"></a>
<a href="CHANGELOG.md"><img src="https://img.shields.io/badge/Changelog-4d4d57?style=for-the-badge&labelColor=242429" height="36" alt="Changelog"></a>

</div>

---

# What is slot?

A bespoke, Game Boy-centric frontend for the Anbernic RG SP.

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

Games go in `Games/<Folder>`, label art in `Labels/<Folder>/<game>.png`, BIOS files in `BIOS`.
A multi-disc game is shelved once through its `.m3u` playlist, and its discs are swapped from
Gameplay in the game menu.

Services, under Network, turns on SSH and SFTP as `root` (BaseOS's password until one is set
there), sharing the card over SMB, Tailscale (signed in by scanning a code) and WireGuard. A
WireGuard tunnel is a wg-quick `.conf` in `Config/wireguard`.

> [!IMPORTANT]
> ROMs must be unzipped. slot does not read `.zip` or `.7z` files.

A full user guide can be found at [slot-cfw.fyi](https://slot-cfw.fyi).

Release notes can be found in the [changelog](CHANGELOG.md).

---

# What does it look like?

<p align="center">
<img src=".github/resources/screenshots/carousel.gif" width="48%" alt="Scrolling the cart carousel across the Game Boy Advance, Game Boy and Game Boy Color shelves">
<img src=".github/resources/screenshots/core-picker.gif" width="48%" alt="Flipping a cart to its circuit board to choose mGBA or gpSP">
</p>

<p align="center">
<img src=".github/resources/screenshots/link.gif" width="96.5%" alt="Two SPs racing in Mario Kart over the emulated link">
</p>

---

# Supported Devices

| Device             | Supported                  | Since Version |
|--------------------|----------------------------|---------------|
| Anbernic RG SP     | Yes                        | Always        |
| Anbernic RG34XX    | Yes                        | 1.4.0         |
| Anbernic RG34XXSP  | Yes                        | 1.4.0         |
| Anbernic RG35XXSP  | Yes                        | 1.5.0         |
| Anbernic 35XX Plus | Yes                        | 1.5.0         |
| Anbernic RG28XX    | Yes                        | 1.6.0         |

If you have one of the untested devices, or have tried slot on a device not listed,
please [open an issue on GitHub](https://github.com/BrandonKowalski/slot/issues) and let me know how it went.

---

# AI Disclosure

The Rust frontend, and the SVGs on [slot-cfw.fyi](https://slot-cfw.fyi), were put together by Claude.
I reviewed everything that was produced. All documentation is 100% free-range, meatbag prose.

My goal with this project was to recreate the feeling of playing my GBA SP in the back seat as a kid,
but with a few modern creature comforts.

Nothing about this project is exciting or groundbreaking from a technological perspective, and as a
result, I delegated the glue code. mGBA and gpSP have been around forever and are the true stars of
this show. The incredible skeuomorphic art is the work of [Markus](https://github.com/MarkusMaribu), an
artist who saw the bare-bones version and offered to dress it up.

I understand if my use of AI is a dealbreaker for you. Just know it's the least interesting part of
what's here.

Figured I should share the end result of all the wasted water. ✌🏻
