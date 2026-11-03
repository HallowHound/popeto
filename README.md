# Popeto

Portable desktop pets for Windows that run **Shimeji / Shimeji-ee pet packs natively**: no Java, no .NET,
no installer. Copy the folder anywhere and run `Popeto.exe` (~1 MB, ~15 MB RAM).

<p align="center"><img src="docs/pets.png" alt="Three bear pets standing together on the desktop"></p>

<p align="center">
  <img src="docs/chooser.png" alt="Pet chooser" width="49%">
  <img src="docs/settings.png" alt="Settings window, Life tab" width="40%">
</p>

## Features

- **Shimeji-ee compatible engine**: reads `actions.xml` / `behaviors.xml` (English or original Japanese schema),
  including the `${...}` / `#{...}` scripts, conditions, NextBehaviorList, hotspots (petting), cloning,
  sounds and the embedded actions (Fall, Jump, Dragged, Regist, Breed, Transform, WalkWithIE, ThrowIE,
  MoveWithTurn, ...).
- **Windows without configuration**: pets detect every normal window automatically, land on window tops
  (even windows below others), walk, climb, hang under them, ride along when you drag a window, carry and
  throw windows, and fall off when you shake them. Thrown windows can be kept on screen and restored.
- Multi-monitor, per-monitor DPI, fullscreen detection (hide over games, stay visible over Remote Desktop).
- **Pet chooser** with previews, **settings window** (General / Windows / Pranks), tray menu.
- **Importer**: add pets from a downloaded Shimeji zip or folder; shared `conf\` is attached automatically.
  Pets in the mobile Shimeji app format (shimeji.org: `manifest.json` + `animation.json` + WebP frames) are
  converted to Shimeji-ee using the template in `assets/template`.
- **Getting pets**: tray links to shimeji.org, Cachomon, Kilkakon, r/shimeji and DeviantArt; Popeto watches the
  Downloads folder and offers to add new Shimeji zips with one click; zips can also be dropped onto a pet.
  (Downloads always happen in your browser, so sites' ads, Patreon and terms are respected.)
- **Fresh internet pictures** (optional): a background thread fetches random funny pictures and memes
  (sources in `pranks\*\online.txt`, e.g. r/Pikabu for Russian) through WinHTTP and keeps them in memory only.
- **Life**: pets stand on each other, greet, play tag / follow the leader / fetch, hide behind and push
  windows, sleep while you're away (idle time from Windows, no hooks), react to the time of day, show
  emote particles; toys: cookie, ball, balloon, bed. Shimeji-ee `ScanMove`/`Interact`/`Broadcast` work too.
- **Extras**: break / drink-water reminders and a kitchen timer (a pet brings a note), pets dance when music
  plays (Windows peak meter), stand on desktop icons (Shell `IFolderView`, read only), laser pointer and chalk
  doodles, pet names shown on mouse over. Scripts can talk through pets: `Popeto.exe --say "Build done"`,
  `Popeto.exe --note "text"`.
- **LAN visits** (off by default): pets walk off your screen and visit other PCs running Popeto on the same
  network, then come back. Discovery by UDP broadcast on port 47811 (or listed addresses); a pet the other PC
  doesn't have is sent as a zip over TCP for the visit (`visitors\`, images/xml/wav only, only while visiting).
  Right click a pet > **Send with a message**: it carries your note (up to 300 characters) onto the other screen.
- **Pranks** (goose-style, all off by default): pets bring notes and pictures, leave muddy footprints, grab
  the cursor (shake to escape), talk in speech bubbles, honk, stomp/minimize windows, play with a ball.
  Panic hotkey (default `Ctrl+Shift+F12`) hides everything. No keyboard/mouse hooks, no injection.
- **Languages**: English and Russian UI (auto from Windows). Prank content in two layers:
  `pranks\universal` (any language) + `pranks\<lang>`.

## Download

Get `Popeto-<version>.zip` from [Releases](../../releases), unzip it anywhere and run `Popeto.exe`.
The exe isn't code-signed, so on first launch Windows SmartScreen may say "Windows protected your PC":
click **More info > Run anyway**.

Popeto checks GitHub for new versions (once a start and every 12 hours, switch off in Settings > General).
When one is out, click the tray notification or **tray > Update to ...**: only `Popeto.exe` is replaced, your pets,
pranks and settings stay, and Popeto restarts by itself.

## Build it yourself

1. Install [Rust](https://rustup.rs/) with the default MSVC toolchain (`x86_64-pc-windows-msvc`).
   Rustup offers to install the Visual Studio Build Tools if they are missing
   (workload "Desktop development with C++").
2. Clone and build:

   ```powershell
   git clone https://github.com/HallowHound/popeto.git
   cd popeto
   powershell -ExecutionPolicy Bypass -File scripts\package.ps1
   ```

   This builds `target\release\popeto.exe` and assembles the portable folder `dist\Popeto`
   (Popeto.exe, README.txt, LICENSE.txt, pranks\, img\ with Popi). `cargo build --release` alone builds just the exe;
   `cargo test` runs the tests.

The exe is statically linked (`+crt-static`, see `.cargo/config.toml`), so it needs no runtime on other PCs.

### Releases

Bump `version` in `Cargo.toml`, commit, then push a tag; GitHub Actions (`.github/workflows/release.yml`)
tests, packages and publishes `Popeto-<tag>.zip` as a release:

```powershell
git tag v0.2.0
git push origin v0.2.0
```

`Popeto.exe --settings` opens the settings window on start; `Popeto.exe <pets.zip>` imports pets;
`Popeto.exe --say "text"` / `--note "text"` makes a pet say it or bring it as a note (starts Popeto if needed).

## Layout of the portable folder

```
Popeto.exe
popeto.ini            settings (created on first run)
img\<Pet>\            Shimeji pets: shime*.png + conf\actions.xml, behaviors.xml
pranks\universal\     notes.txt, talk.txt, pictures\, sounds\  (any language)
pranks\en\, ru\       notes.txt, talk.txt, pictures\            (that language only)
```

Popeto comes with one simple pet of its own, **Popi** (`assets/pets/Popi`, drawn by
`scripts/make_default_pet.py`, MIT like the code). Other pet packs are not part of this repo; they belong to their artists. Get Shimeji / Shimeji-ee packs and add
them with **tray > Add pets from Shimeji zip...**

## Source map

| File | What |
|---|---|
| `src/expr.rs` | JavaScript-subset evaluator for Shimeji scripts |
| `src/config.rs` | Pack loader (XML, Japanese schema mapping) |
| `src/env.rs` | Areas, borders, window/environment view, script scope |
| `src/action.rs` | Runtime action tree (Stay, Move, Sequence, Fall, Jump, Dragged, ...) |
| `src/mascot.rs` | Behaviour selection |
| `src/win.rs` | Win32: layered windows, window scanning, tray, menus, dialogs |
| `src/main.rs` | App loop, input, menus, applying settings |
| `src/pranks.rs` | Pranks, overlays, ball |
| `src/chooser.rs`, `src/settings_ui.rs` | Pet chooser and settings windows |
| `src/importer.rs` | Shimeji zip/folder importer |
| `src/extras.rs` | Reminders, timer, `--say`, dancing, laser, chalk, name tags |
| `src/audio.rs`, `src/icons.rs` | Speaker peak meter, desktop icon positions (COM) |
| `src/lan.rs` | LAN visits |
| `src/i18n.rs` | UI translations |

## Credits

Prank pictures come from Wikimedia Commons under free licenses; authors and licenses are listed in
`assets/pranks/*/pictures/CREDITS.txt`. Russian captions were added for Popeto.

## License

Popeto's code is under the [MIT License](LICENSE). The prank pictures keep their own licenses
(CC0 / CC BY / CC BY-SA, see the `CREDITS.txt` files); pictures with Popeto captions are shared
under the same license as the original photo.
