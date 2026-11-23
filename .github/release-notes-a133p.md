A release for TrimUI's A133P handhelds (Brick Pro, Brick, Smart Pro) alone. H700 handhelds stay on the release they have.

## Changes

- **DraStic and PPSSPP inside slot**: on the A133P they now run in slot as they do on the H700, with slot's in-game menu, states and Exit Game. The A133P card of v2.17.0 carried DraStic's and PPSSPP's shims from before slot drove them, so each drew straight over slot's last frame, Menu never reached slot, and the game could not be left.

The full list is in [CHANGELOG.md](https://github.com/cloudwithax/unslot/blob/$TAG/CHANGELOG.md).

## Install

- **Already running unslot:** Settings → Online → Updates installs `unslot-$TAG-a133p.pak`. Or copy it to the root of the card yourself and restart with the power connected; your games, saves and settings stay.
- **New card:** unzip `unslot-$TAG-a133p.zip` and copy everything in its `unslot-$TAG-a133p` folder, the hidden `.tmp_update` folder too, to the root of a FAT32 or exFAT card, then boot the handheld with it. It runs on TrimUI's stock firmware in place of its own frontend.

---

unslot is a fork of [slot](https://github.com/BrandonKowalski/slot) by Brandon T. Kowalski. Emulation by mGBA (MPL-2.0), gpSP (GPL-2.0) and libretro cores; every licence, and the source of the cores built for unslot, is in `System/licenses/`.
