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
  Getting pets: tray > "Get more pets" opens shimeji.org, Cachomon, Kilkakon, r/shimeji or DeviantArt
  in your browser. Download as usual there; when a Shimeji zip lands in your Downloads folder, Popeto
  shows a notification - click it (or the tray menu item "Add downloaded pet") to add the pet.
  You can also drag a zip onto any pet or onto the settings window.
  Pets from shimeji.org come in the mobile Shimeji app format (manifest.json, animation.json, .webp
  frames). Popeto converts them automatically into a normal pet (PNG frames + generated conf).
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

Life (Settings > Life; on by default except pushing windows)
  Pets stand on each other's heads, greet each other, play tag, walk in a line behind a leader,
  fetch the ball you throw, hide behind the window they climb, push windows (off by default),
  fall asleep when you haven't touched keyboard/mouse for a few minutes and wake up when you return,
  say good morning / lunch time / good night, and show little hearts, stars and zzz.
  Toys (tray > Toys, or Settings > Life): drop a cookie (pets run to eat it), throw a ball,
  release a balloon (pets jump for it; click to pop), put a bed (pets nap on it). Drag cookies and
  the bed around; right click a toy to remove it.

Extras (Settings > Extras)
  Reminders: take a break after N minutes at the computer, drink water every N minutes (0 = off).
  Timer: tray > Timer > 5..60 min; a pet brings a note when it rings.
  Scripts: Popeto.exe --say "Build done" (speech bubble) or --note "text" (a pet brings a note).
  Pets dance while music plays, stand on desktop icons, and show their name on mouse over
  (right click a pet > Name...). Toys: laser pointer (pets chase the red dot), chalk (pets draw).

LAN visits (Settings > Network, off by default)
  Pets walk off your screen to another PC running Popeto on the same network, stay a few minutes and
  come back. Right click a pet > Go visiting, or let them go by themselves. If the other PC doesn't have
  that pet, it is sent over the network for the visit (visitors folder); right click a guest > Keep a copy.
  Uses port 47811 (UDP + TCP); Windows may ask to allow Popeto in the firewall. If PCs don't find each
  other (office networks), type their addresses in Settings > Network.

Panic key (default Ctrl+Shift+F12, PanicKey= in popeto.ini): hides every pet and stops all pranks;
press again to bring them back. No keyboard/mouse hooks are used.

Prank content lives in pranks\ in two layers (see pranks\README.txt):
  pranks\universal\   for any language: pictures without text, sounds, symbol-only notes/bubbles
  pranks\en\, pranks\ru\   only for that interface language: texts and pictures with text
  Pets pick from universal + the current language together.

Language: Settings > General > Interface language (Auto = Windows language, English, Russian).
  It switches the menus, windows and the language-specific prank texts/pictures.

Options > Size and Options > Opacity change pet size (50-300%) and transparency.
