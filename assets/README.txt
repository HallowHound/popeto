Popeto - portable desktop pets (Shimeji compatible)
====================================================

Run Popeto.exe. Nothing to install: no Java, no .NET, no runtimes.
Copy this whole folder anywhere (USB stick, another PC) and it just works.

Pets
  img\<Pet name>\         one folder per pet, same layout as Shimeji-ee:
     shime*.png            images
     conf\actions.xml      animations   (or conf\<Pet name>\actions.xml next to Popeto.exe,
     conf\behaviors.xml    behaviours    or a shared conf\ folder next to Popeto.exe)
  Easiest: tray menu > "Add pets from Shimeji zip..." (or drag the zip onto Popeto.exe) and pick a
  downloaded Shimeji / Shimeji-ee zip, or any file inside an unpacked Shimeji folder. Popeto copies
  every pet into img\ together with its conf (old Shimeji-ee downloads keep conf outside the pet folder).
  Or drop a pet folder into img\ yourself and pick "Reload pets" in the tray menu.
  Folder img\unused is ignored.
  Tray menu > "Choose pets..." opens a window with every pet's picture and name: click to switch
  pets on/off, double-click to keep only one. Tick "Show at startup" to see it on every launch.

Mouse
  Drag a pet to pick it up, release to throw it. Click hotspots (e.g. a sitting bear's head) to pet it.
  Right click a pet: make it do something, follow the cursor, toggle behaviours, dismiss.
  Tray icon: summon pets, options, restore windows the pets moved, exit. Double click = summon random pet.

Settings
  Tray menu (or right click a pet) > "Settings..." opens the settings window: General (size, opacity,
  smoothing, max pets), Windows (how pets treat your windows, fullscreen apps, ignored apps) and Pranks.
  Popeto.exe --settings starts with the settings window open.

Windows
  Pets automatically use every normal window: they land on window tops, walk, climb the sides,
  hang under them, carry and throw them. No window titles to configure.
  Thrown windows stay on screen (option) and "Restore moved windows" puts them back.
  Exclude apps in popeto.ini -> IgnoreWindows (exe name or part of a title).

popeto.ini is created on first run; popeto.log lists pack problems if a pet doesn't load.

Pranks (tray > Pranks, everything OFF until you tick it)
  Bring notes        pets walk off-screen and drag back a sticky note (notes.txt, one per line)
  Bring pictures     same with random .png pictures (pictures folder)
    ...also fresh ones from the internet: random funny pictures and memes, kept in memory only and
    never saved. Sources are listed in online.txt (reddit <subreddit>, json <url> <field>, image <url>);
    adult and spoiler posts are skipped. Off by default.
  Muddy footprints   walking pets leave fading paw prints
  Grab the cursor    a pet jumps on your mouse pointer and drags it around - shake the mouse to escape
  Speech bubbles     random lines and reactions (talk.txt, [sections] per moment)
  Honk               plays a random .wav (sounds folder) when a pet is clicked or now and then
  Stomp & knock down windows    a pet on a window shakes it or minimizes it
  How often          Calm / Normal / Chaos
  Throw a ball       pets chase and kick it; drag to throw it, right click to take it away
  Notes: drag to move, click the x (or right click) to close. "Clean up" removes notes and footprints.

Panic key (default Ctrl+Shift+F12, PanicKey= in popeto.ini): hides every pet and stops all pranks;
press again to bring them back. No keyboard/mouse hooks are used.

Prank content lives in pranks\ in two layers (see pranks\README.txt):
  pranks\universal\   for any language: pictures without text, sounds, symbol-only notes/bubbles
  pranks\en\, pranks\ru\   only for that interface language: texts and pictures with text
  Pets pick from universal + the current language together.

Language: Settings > General > Interface language (Auto = Windows language, English, Russian).
  It switches the menus, windows and the language-specific prank texts/pictures.

Options > Size and Options > Opacity change pet size (50-300%) and transparency.
