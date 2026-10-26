# Changelog

## v2.8.2

- The menus fill a square screen, such as the RG CubeXX's. They were drawn in a 720 by 480 strip across the middle with black above and below. The shelf, menus and settings are now laid out at 720 by 720, with the cart slot and footer at the bottom and the shelf and lists in the middle. Games keep their shape as before. Other handhelds are unchanged.

## v2.8.1

- The Xbox Wireless Controller stays connected over Bluetooth. Pairing was asked for before bluetoothctl's agent was in, and bluez turns bonding off while no agent is registered, so the pad linked up, was never bonded and dropped straight after. Pairing now waits for the agent first.
- Bluetooth and USB controllers have their own button map, with A, B, X and Y as printed on an Xbox pad, the bumpers as L1 and R1, the analog triggers as L2 and R2, View, Menu and the Xbox button as Select, Start and Menu, and the stick clicks as L3 and R3. They used the handheld's own button codes, so most buttons were wrong. The Xbox pad's right stick is read too.
- Games that rumble, Dreamcast, N64, PlayStation and the GBA rumble carts among them, rumble a paired Xbox Wireless Controller as well as the handheld.
- Sound on TV, under General, while the picture is on a TV. Off, the sound stays on the speaker or headphones, for a screen with no speakers.

## v2.8.0

- Game Art fetches the game's real cartridge or disc. When ScreenScraper has a render of the actual cart, under physical media, the shelf draws that cart as it is in place of unslot's model. Failing that, the scan of the real label goes on the model, and only after that the box art and logo as before. Renders are kept in `Labels/<system>/Renders` next to the labels. Games that already have a label keep it; delete the label and fetch again to get the real one.
- Cast is in the menu over a game too, so a game can be put on the TV without leaving it. B on the list of TVs goes back to the menu.
- The footer shows a cast icon next to Wi-Fi and Bluetooth while the screen is being cast.

## v2.7.1

- Paired Bluetooth controllers and headphones stay paired across a restart. bluez keeps its pairings in `/var/lib/bluetooth`, which BaseOS clears at every boot, so each one had to be paired again. They are now copied to `Config/bluetooth` on the card whenever something pairs or is forgotten and when Bluetooth turns off, and put back before Bluetooth turns on.
- The PSP's UMD on the shelf looks like a UMD: the round-topped white shell with the disc showing through its window, the game's art on the disc round the white UMD hub, and the PSP logo and the controller on the strip below.

## v2.7.0

- Cast the screen to a Chromecast or a Google TV over Wi-Fi, under Network → Cast. Pick a TV from the list and it shows what the handheld does, games and menus alike, with the sound; the handheld's speaker and headphones go quiet until the cast stops. A on that TV in the list again stops it, and Y looks for TVs again. The picture keeps its shape, with black bars at the sides. It runs at up to 60 fps, and heavier games, such as Dreamcast ones, at about 35.

## v2.6.1

- Bluetooth LE controllers pair, the Xbox Wireless Controller among them. The radio was powered on before bluetoothd was running, and BaseOS's kernel only turns on LE security when bluetoothd powers it, so an LE pad linked up but never paired. Turning Bluetooth on now hands the power to bluetoothd.
- A controller only shows as Connected once its buttons reach unslot, and headphones once they are paired. bluez called a device connected the moment a link came up, so a pad that was never going to work showed as Connected, on and off.
- The Devices list no longer reorders as things connect and drop, so A acts on the device it is on. It refreshes every second while it is open, and reads every device in one go instead of one at a time.
- A pad that links up but never pairs is forgotten, so it stops reconnecting in a loop, and the page says so.
- Bluetooth works again after being turned off and on. Turning it off cut the chip's power, and turning it on did not load the chip's firmware again, so it stopped answering until a restart.

## v2.6.0

- Nodosumi comes with unslot and runs Dreamcast games, with Flycast as the other choice. An update replaces `System/`, so a Nodosumi copied there by hand used to be swapped back for Flycast. Saves and states made under Flycast's name stay in `States/DC/flycast`.
- HDMI out. With a cable in, unslot draws on the TV at 720p and sends the sound there; pull it out and unslot comes back on the panel. A game in play is saved and picks up where it was. Closing the lid while on the TV doesn't put the game to sleep. Ported from slotPlus.
- RetroAchievements, under Network → RetroAchievements. Sign in with your username and password. Only the token RetroAchievements sends back is kept, in `Config/retroachievements.conf`, and entering no username signs you out. Unlocks, mastery and leaderboard scores pop up at the top of the screen. It is softcore only, and the Dreamcast, N64 and PSP cores are left out because they run in their own process. The rcheevos glue comes from machomano's slot-cheevos.

## v2.5.0

- 32 more cores to download, under Network → Cores, or from a game's core list, where a core not on the card says Download. They come from unslot's own `cores` release, copied from libretro's buildbot and signed with the release key, and go in `Cores/` on the card, which updates leave alone. X on the Cores page removes one. Each loads on an RG34XXSP, and those tried with a game there ran at full speed.
- The GBA keeps its board of two cores until another GBA core is downloaded, and from then on lists its cores as other systems do.

## v2.4.0

- A core can be set for a whole system as well as for one game. START on a game opens its cores as before: A sets one for that game, and Y for every game on its system. A game given its system's core follows the system from then on. The picks are kept in `Config/system_core.txt`.
- X and Y reach the game when turbo is off, so a Dreamcast pad has all four face buttons.
- A core that falls behind no longer pitches its sound down: the sound's rate stays within 1% of the core's.

## v2.3.0

- Power, on the main menu: Restart, Shut Down, and Reload unslot, which starts unslot again without restarting the device.
- Dreamcast games are no longer autosaved, and other games are autosaved every five minutes instead of every minute. A Dreamcast autosave held the game up long enough to drop frames.
- A save state comes back from the core through shared memory and is packed with LZ4, so saving one doesn't stall the game. States saved before this still load.
- When a core crashes during a game, the cart comes back out with the alert for a game that won't open, instead of the game freezing.
- A disc set kept in its own folder, as Redump sets are, shows on the shelf as the disc named after the folder.
- File sharing and Syncthing keep running when unslot restarts, so a transfer isn't cut off.
- Wi-Fi gets an address on a flashed card image: the script that starts DHCP was not executable there.

## v2.2.1

- Game Art passes over a game ScreenScraper turns away with a 403 or another error, instead of stopping the whole run, and says on the Game Art page how many it skipped. The game still gets its turn at libretro.
- The boot logo is drawn the moment the panel is unslot's, so a restart is not black while the shelf is built.
- Dreamcast games show an FPS counter.
- A GPU core's frames are handed to the frontend in step, and its sound is held to its rate, rather than run dry or fill and wander.
- Syncthing, under Network → Services. Its web page is at `http://<device>:8384`, signed in to as root, and X shows the device ID as a code for the Syncthing app to scan. Folders added there go on the card.
- Games Syncthing brings go on the shelves as they land, new systems too, and the top of the shelf says how far it has got, how much is left and about how long it will take.
- File sharing starts: the Samba it uses needed a library BaseOS does not have, which now comes with it.
- A flashed card image keeps unslot's space on ext4, which a reset mid-write cannot corrupt, and grows it in place on first boot. Windows and macOS cannot open it: add games over File sharing or Syncthing. Updates and slot cards stay on FAT.

## v2.2.0

- Forty-two more systems, fifty-five in all: arcade (FinalBurn Neo and MAME 2003-Plus, with CPS-1, CPS-2, CPS-3 and Neo Geo on shelves of their own), Neo Geo CD, Famicom Disk System, Nintendo 64, Virtual Boy, Pokémon mini, Game & Watch, 32X, Sega CD, SG-1000, Dreamcast, PC Engine CD, SuperGrafx, PC-FX, the Atari 2600, 5200, 7800, 800 and ST, ColecoVision, Intellivision, Vectrex, Odyssey², Channel F, Supervision, Mega Duck, MSX, Commodore 64, Amiga, ZX Spectrum, Amstrad CPC, DOS, PC-98, X68000, ScummVM, PICO-8, TIC-80 and the PSP, each on its own media.
- Nintendo 64, Dreamcast and PSP draw on the GPU: their cores are built for the Mali's OpenGL ES and get a context of their own.
- Arcade sets show the game's name, not the zip's, and a board's BIOS zip stays off the shelf.
- START on a game whose system has more than one core picks the core from a list.
- X on the shelf searches every system's games by name. The results show each game's media with its label, and A turns to its shelf with it in the slot.
- The Ko-fi link is gone: FinalBurn Neo's licence does not allow asking for donations.
- Updates come as `unslot-<version>.pak`: the release's MinUI.zip, untouched, with its version and checksum signed by unslot's release key. unslot installs a pack from the Updates page or the card's root only when it checks out, and leaves any other as `.bad`.

## v2.1.0

The first unslot, forked from slot 1.7.0.

- Ten more systems: NES, SNES, Genesis, Master System, Game Gear, PC Engine, Neo Geo Pocket, WonderSwan, Lynx and PlayStation, each on its own shelf as its own media.
- Wi-Fi with an on-screen keyboard; Bluetooth controllers and headphones.
- Analog sticks on the models that have them.
- Services: SSH, card sharing over SMB, Tailscale and WireGuard.
- Updates over the air for unslot and BaseOS. A release's `MinUI.zip` also turns a slot card into an unslot one.
- Game Art: labels found online, by CRC and file name. It keeps going while you play, and each label shows on the shelf as soon as it is found.
- Twelve bundled wallpapers, and JPG wallpapers of your own.
- Wi-Fi and Bluetooth in the shelf footer, and the unslot name and logo throughout.
- Moulded plastic grain and softer light on the new systems' carts, cards and discs.
- Every page's buttons are shown with their icons and say what they will do for the row you are on.
- Menu sounds recorded from real buttons and switches, and haptics for a cart going home, a setting changed or something refused. Each can be turned off in Settings → General.

## slot

## [1.7.0](https://github.com/BrandonKowalski/slot/releases/tag/v1.7.0)

- The slot, carts and shelf now have a skeuomorphic effect, with light and dark themes and bundled wallpapers. Huge thanks to Markus for this amazing work! (#74, @MarkusMaribu)
- An in-game menu: tap MENU to pause the game behind it. A double tap still goes straight to States.
- Favorites: Y stars a cart onto its own shelf. (#89, @diluno)
- A Recents shelf of the last 10 games, switched on from Settings -> General.
- slot remembers the shelf and cart at power off and boots back onto them. (#53)
- Per-platform bezels. Markus again! (#77, @MarkusMaribu)
- On 4:3 panels the shelf fills the screen. (#81, @diluno)
- Game Titles on the General page fades the shelf name into the selected game's name. (#84)
- A 1 frame run-ahead setting for GBA games on mGBA. (#56, @454135431515)
- Wallpaper Tint darkens your own wallpaper on the shelf.
- GBA, GB and GBC each get their own Shader and Color Correction rows. (#83, @paulocoe-creator)
- LCD3x is offered for GB and GBC games. (#88, @paulocoe-creator)
- Game Boy stretch now properly only widens the picture.
- The backlight steps follow a curve, so the low-end goes dimmer. (#54, @flo333)
- Holding a direction repeats it in menus, the clock and the shelf letter jump, but never in play.
- RG34XX uses its own chords for brightness and the menu display. (#85, @paulocoe-creator)
- The RG28XX picture is turned the way BaseOS says.
- Restart no longer hangs on a black screen: the WiFi driver is unloaded first.
- Muted play keeps its frame rate. (#51, @flo333)
- Fast-forward no longer stutters on frames that take longer than average. (#70)
- gpSP no longer crashes when a crowded boot left its JIT cache unmapped.
- Core options follow the platform that is playing. (#82)
- Card logs moved into a `Logs` folder, and the session log rotates. Card alarms are logged too.

## [1.6.0](https://github.com/BrandonKowalski/slot/releases/tag/v1.6.0)

- Runs on the RG28XX: its panel is mounted portrait, so slot turns the picture onto it. (#35, @GamerAnim)
- Pokémon Pinball: Ruby & Sapphire rumble. mGBA now sees the game's Game Boy Player check. You still need to turn rumble on in game settings. (#25)
- A red alarm shows when the SD card turns read-only or a save fails to write.
- A new shortcut for mute: SELECT + VOL+ / VOL-. The 34XX has a fake volume wheel so VOL+ + VOL- isn't possible. (#39)
- Game Boy cartridge clocks are kept in the .sav. (#22)
- The GBA cartridge clock keeps running on gpSP while the game is off.
- Large libraries now should boot faster, and the first frame shows sooner.
- Games start faster: resting on a cart reads its ROM ahead, so big ROMs no longer wait on the SD card when you insert them.
- Wallpapers are dealt from a shuffled deck, so none repeats until all have shown.
- PNG art is turned the way its EXIF orientation tag says.
- When slot crashes it logs where, and keeps that run's log in `crash/` to attach to a bug report.
- The BaseOS startup sound no longer leaves slot silent. (#20)
- A user defined `Config/bootlogo.bmp` will take precedence over the shipped one. (#30)
- Cart shell, core and video mode files are now `.txt` instead of `.ini`, so Chrome on Windows opens them. slot renames them on first boot. (#15)
- The grid shader's lines stay even on the RG34XX and at fractional scales, and the dot shader no longer darkens a white screen as much. (#26, #36)

## [1.5.0](https://github.com/BrandonKowalski/slot/releases/tag/v1.5.0)

- Rumble regression fixed.
- The boot logo is built to the size of the one BaseOS installed, so it fits every panel instead of only 720x480.
- Runs on the RG35XXSP: the picture fills the 4:3 screen's width and a GBA SP bezel sits underneath while a game plays.
- Screen shaders chosen per platform: Off, LCD3x, Grid or Dot for GBA, and Off, Grid or Simpletex for Game Boy and Game Boy Color.
- SELECT + B / A in game steps back and forth through the screen shaders. A game's own soft reset that holds SELECT with A or B no longer reaches it; hold A on the carousel to start a game fresh instead.
- New settings: auto save on eject, turbo buttons and rewind can each be switched off.
- Settings are grouped into Screen and Gameplay pages, and every value wraps from the last choice to the first.
- Label art is cached on the card, so the shelf no longer stutters building cart faces. New labels are cached once at boot behind a Caching New Labels screen.
- Named palettes for Game Boy games: turn on GB Palettes in the Screen settings, then SELECT + L2 / R2 in game steps back and forth through mGBA's 48 palettes.
- The speaker no longer buzzes while nothing is playing. slot lets go of the audio device after 3 seconds of silence and takes it back when sound starts. Thanks to flo333 for finding it.

## [1.4.0](https://github.com/BrandonKowalski/slot/releases/tag/v1.4.0)

- The volume bar shows a headphones icon while they are plugged in.
- Speaker and headphones keep their own volume and mute, and switch the moment you plug / unplug.
- Settings moved from `System/` to a new `Config/` folder, so replacing `System/` on update no longer resets them. slot moves them on first boot.
- The quick menu and power menu wrap from the last item to the first, and back.

## [1.3.0](https://github.com/BrandonKowalski/slot/releases/tag/v1.3.0)

- The SP boots to a slot logo instead of BaseOS's. slot installs `System/bootlogo.bmp` on first run and keeps the original as `bootlogo.baseos.bmp`.

## [1.2.2](https://github.com/BrandonKowalski/slot/releases/tag/v1.2.2)

- Fix for hitch in games caused by autosave. The autosave now writes to the card in the background.

## [1.2.1](https://github.com/BrandonKowalski/slot/releases/tag/v1.2.1)

- Shell colors from Cart Studio now apply to games with accented names.
- Clear GBA carts no longer show their circuit board above the label.

## [1.2.0](https://github.com/BrandonKowalski/slot/releases/tag/v1.2.0)

- Hold X for turbo A and Y for turbo B.
- Carts on the shelf are larger, and shrink into the slot as they go in.
- Carts are drawn at their real proportions, so full label scans fit without cropping.
- Jumping a letter with Up or Down shows the letter in the slot.
- Saves now stick for games whose save is smaller than the core first reports, such as Golden Sun on mGBA.
- The seated cart no longer shows through the slot as a game starts.
- SELECT reaches the game as soon as it can no longer be the start of a shortcut.

## [1.1.0](https://github.com/BrandonKowalski/slot/releases/tag/v1.1.0)

- Fast-forward runs several game frames per refresh, so it is properly fast, and rewind no longer records while fast forwarding.
- The About label credits BaseOS.

## [1.0.0](https://github.com/BrandonKowalski/slot/releases/tag/v1.0.0)

- Game Boy and Game Boy Color, each on its own shelf. L1 and R1 switch shelves, and carts are drawn at their own size in their own plastics.
- Two cores, mGBA and gpSP. Press START on a cart to open it and swap the chip.
- Link play between two RG SPs over WiFi: the link cable and the Wireless Adapter for Pokémon trades and Advance Wars, mGBA's link mode for Mario Kart, and Game Boy link for Tetris.
- A quick menu on the shelf for fast forward speed and sound, rumble, color correction, the date and time, and About.
- Game Boy games stretch to fill the screen with L1 and go back with R1.
- The real BIOS boot animation plays when the card has one.
- Per-cart shell colors, set in `cart_shell.ini`.
- Up and Down jump through the shelf a letter at a time.
- Your own folder layout on the card is kept, and slot runs on upstream BaseOS.

## [0.1.1](https://github.com/BrandonKowalski/slot/releases/tag/v0.1.1)

- The game holds still behind the power menu.
- Rewind records at every speed and replays exactly what happened.

## [0.1.0](https://github.com/BrandonKowalski/slot/releases/tag/v0.1.0)

- First release: a GBA-only shelf of cartridges with an insert scrape and clunk.
- The game in the slot resumes on the next boot. Closing the lid sleeps and saves.
- Save states with a switcher, rewind, fast-forward, brightness, blue light and volume.
- A power menu on a held POWER, and an About label.
