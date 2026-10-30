# Popeto

Portable desktop pets for Windows that run **Shimeji / Shimeji-ee pet packs natively**: no Java, no .NET,
no installer. Copy the folder anywhere and run `Popeto.exe` (~1 MB, ~15 MB RAM).

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
- **Pranks** (goose-style, all off by default): pets bring notes and pictures, leave muddy footprints, grab
  the cursor (shake to escape), talk in speech bubbles, honk, stomp/minimize windows, play with a ball.
  Panic hotkey (default `Ctrl+Shift+F12`) hides everything. No keyboard/mouse hooks, no injection.
- **Languages**: English and Russian UI (auto from Windows). Prank content in two layers:
  `pranks\universal` (any language) + `pranks\<lang>`.

## Build

Requires Rust (MSVC toolchain). The exe is statically linked (`+crt-static`), so it needs no runtime.

```powershell
cargo build --release
# or build + assemble the portable folder dist\Popeto:
powershell -ExecutionPolicy Bypass -File scripts\package.ps1
```

`Popeto.exe --settings` opens the settings window on start; `Popeto.exe <pets.zip>` imports pets.

## Layout of the portable folder

```
Popeto.exe
popeto.ini            settings (created on first run)
img\<Pet>\            Shimeji pets: shime*.png + conf\actions.xml, behaviors.xml
pranks\universal\     notes.txt, talk.txt, pictures\, sounds\  (any language)
pranks\en\, ru\       notes.txt, talk.txt, pictures\            (that language only)
```

Pet packs are not part of this repo; they belong to their artists. Get Shimeji / Shimeji-ee packs and add
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
| `src/i18n.rs` | UI translations |

## Credits

Prank pictures come from Wikimedia Commons under free licenses; authors and licenses are listed in
`assets/pranks/*/pictures/CREDITS.txt`. Russian captions were added for Popeto.
