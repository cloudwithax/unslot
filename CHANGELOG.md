# Changelog

## v2.12.0

- Frame Blending, on each system's Screen settings and the Screen page in the menu over a game: Mix averages each frame with the last, as flickering transparency was drawn for, and Ghosting fades like the Game Boy, Game Boy Color or Game Boy Advance LCD. It is done on the GPU, so it works under every core.
- Saturation and Contrast, under General and on the in-game Screen page, nine steps each.
- Six more steps of brightness below the backlight's lowest: the brightness keys keep going down by dimming the picture, and up undoes them first.
- Left Stick as D-Pad, under Gameplay on handhelds with a left stick, on by default: the stick presses the D-pad past half tilt, so D-pad games play with it. The D-pad wins while both are held.
- Fast Forward has 1.5× and an Off, and its default is 6×.
- Battery Display, under General: the icon and percentage, the icon, or the percentage.
- A game that takes unslot down while it loads is not started again at the next boot. unslot opens on the shelf, and the resume state it went down on is moved aside, so the game starts clean from its save.
- Saves RetroArch wrote compressed are read, instead of the game starting blank and writing over them. A save that is there but cannot be read is never written over.
- Gambatte boots the Game Boy BIOS on the card, when there is one.
- Kinder to the SD card: temporary files a crash left behind are cleared at start, and before a power off or restart the card is synced, the Wi-Fi driver let go of, waiting at most five seconds, and synced again, with nothing left writing to the log.
- The speaker stays quiet until there is sound, and is shut before a power off or restart, so neither ends on a pop.
- From slot upstream: mGBA's fixes for MBC2 saves, flash saves past a bank switch, centred Game Boy channels and the first frame after the LCD comes on.

- Nintendo DS, from `Games/NDS`, run by DraStic. unslot closes while a DS game runs, as it does for a port, and comes back when the game ends. DraStic brings its own stand-ins for the DS BIOS, so none goes in `BIOS`, and keeps its saves, states and settings in `Saves/NDS`, which updates leave alone. Its own menu, with its states and settings, is on Select + X. The Screen settings have no DS row, as DraStic draws the game.
- A tap of Menu in a DS game stops it and opens unslot's menu over its picture, with Resume and Exit Game; Exit Game has DraStic save and close. The volume keys and the mute chord work as in any other game, at the same level for the speaker and the headphones, and each press shows a bar over the game for a moment.
- DS games sit on the shelf as DS Game Cards, with the corner cut at the top right and NINTENDO DS on the strip under the label, and Game Art fetches their art.
- A card whose only game is a DS game opens on the shelf instead of starting it, as a single game of another system is, so the game does not start again each time DraStic is left.
- Jumping by letter through a shelf of thousands of games stays smooth. Each frame measured every cart between the old place and the new one; now it measures the few near the screen. The shelf glides over the last few carts of a jump instead of flying the whole letter past an empty screen.
- A letter jump no longer holds the screen until the labels it lands on are drawn; they come in as they are ready. Only the first screen at start waits for them, and for no more than five seconds.
- Ejecting a game closes its Core Options page too, and a core dying under a game closes its Controls and Core Options pages.
- Achievements work offline after switching between softcore and hardcore, and hardcore unlocks stay earned when the game is started again without a connection.
- With more than one update pack on the card, the newest version goes in, not whichever sorts last by name: `unslot-v2.9.1.pak` no longer wins over `unslot-v2.12.0.pak`.
- Update packs are checked and unpacked a piece at a time instead of whole in memory. Unpacking v2.11.0's 240 MB pack took about 460 MB of the handheld's 1 GB, so a pack would have stopped going in a few releases from now. A pack can also carry new, compressed entries or new lines in its note, and this release still installs it; the note it writes is byte for byte what every unslot since v2.2.0 reads.
- `System/licenses` has rcheevos's MIT notice, which its licence requires to travel with the achievements code, and DraStic's readme and SDL's licence beside DraStic, with where each came from.

## v2.11.0

- Hardcore achievements, under Settings → Gameplay. Turning it on takes effect from the next game; turning it off counts at once. In hardcore nothing may rewind or load a state, so the States page hides Load, and, as in RetroArch, a little play is wanted between pauses, so that pausing cannot step through a game a few frames at a time. RetroAchievements now accepts hardcore unlocks from unslot, which identifies itself as a libretro frontend.
- A ROM or disc whose hash RetroAchievements does not know, often another region's release, falls back to the set of the game of the same title there, in softcore, as another release's memory may not line up for every achievement.

## v2.10.0

- Bezels for every system. Each system's Screen settings, and the Screen page in the menu over a game, have a Bezel row, with the overlays from R. Savant's Anbernic H700 Overlays pack built in at 640 by 480, 720 by 480 and 720 by 720 for the NES, SNES, Genesis and Mega Drive, Master System, Game Gear, PC Engine, Game Boy family, Neo Geo, WonderSwan and more. The game is drawn in the bezel's window, as RetroArch does, and the bezel over the whole screen. A PNG of your own in `Bezels/<system>` is listed with them; with a KNULLI or Batocera `.info` beside it the game goes where that says, else in the PNG's clear middle. Stretch shows the game without the bezel.
- Apps, in the menu: Files, Books, Manuals, Gallery and Music, and ports. Books reads text, EPUBs and PDFs and keeps your page; Gallery shows pictures; videos play with L1 and R1 skipping ten seconds; Music plays on with its page closed and holds while a game runs. Ports in `Ports`, laid out as PortMaster does, run with unslot closed and it comes back when they end.
- Upload from Phone: scan the code with a phone on the same Wi-Fi to send the card a wallpaper or a game.
- Controls: remap the buttons for every game, for a system or for one game.
- The SP's slot comes in Graphite, Platinum, Pearl Pink, Pearl Blue and Pearl Blue Dark, and slots of your own can be added to the card.
- Clear All Art on the Game Art page, after a second press, takes every label off the card.
- Auto-Update Cores fetches new builds of the cores on the card once a start has the Wi-Fi up.
- D-Pad as Stick: on a handheld without an analog stick, the D-pad moves the left stick in games.

## v2.9.1

- Nodosumi, the Dreamcast core, is now 1.1. Games that support vibration rumble, rendering is faster, which smooths out Crazy Taxi 2, and depth is fast by default; a game that shows z-fighting can turn on the new Accurate depth core option. Each release now ships Nodosumi's latest release.

## v2.9.0

- Cast to a Miracast TV as well as a Chromecast. Under Network → Cast, TVs and projectors that take Miracast over Wi-Fi Direct, such as LG, Samsung and Roku TVs and the HY300, are listed with the Chromecasts and marked Miracast. Pick one, accept the handheld on the TV if it asks, and it shows the handheld's screen at 640 by 480 and up to 60 fps, less in the heaviest games, with the sound. The sound goes as AAC to a TV that takes it, as Android-based ones need, and as LPCM to the rest. Wi-Fi Direct uses the handheld's second radio, so the home Wi-Fi stays connected while casting. The TV has to lead the Wi-Fi Direct group; a TV that wants the handheld to lead it says so and is not cast to.

## v2.8.3

- Real carts fetched from ScreenScraper are all drawn the same size. Each was fitted inside its own system's model, so a SNES cart came out big and a Genesis cart small. Every real cart now stands as wide as a GBA cart or as tall as a Game Boy cart, whichever it reaches first, and goes into the slot the same way.
- Game Art no longer fetches a picture of the physical media for arcade games, the computers, Game & Watch, PICO-8 and TIC-80. ScreenScraper has a cabinet, a machine or a whole set there, not a cart, so these keep unslot's own model with the real label. Pictures already fetched for them are no longer drawn.

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
