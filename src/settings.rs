//! `popeto.ini` next to the exe. Plain key=value, rewritten when toggled from the tray menu.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Goose-style mischief. Everything is off until switched on in the tray menu.
#[derive(Default, Clone)]
pub struct Pranks {
    /// 0 = calm, 1 = normal, 2 = chaos (how often pranks happen).
    pub level: u8,
    pub notes: bool,
    pub memes: bool,
    /// Also fetch fresh pictures from the internet for the pictures prank.
    pub online: bool,
    pub footprints: bool,
    pub cursor: bool,
    pub talk: bool,
    pub honk: bool,
    pub windows: bool,
}

/// Things pets do on their own: with each other, with windows and with you.
#[derive(Clone)]
pub struct Life {
    pub stacking: bool,
    pub meet: bool,
    pub games: bool,
    pub hide: bool,
    pub push: bool,
    pub sleep: bool,
    pub sleep_minutes: u32,
    pub daytime: bool,
    pub emotes: bool,
    pub talk: bool,
    /// Pets dance while music plays.
    pub dance: bool,
    /// Desktop icons are platforms pets can stand on.
    pub icons: bool,
    /// Show a pet's name when the mouse is over it.
    pub show_names: bool,
    /// Break reminder after this many minutes at the computer (0 = off).
    pub break_minutes: u32,
    /// Drink-water reminder every this many minutes (0 = off).
    pub water_minutes: u32,
}

impl Default for Life {
    fn default() -> Self {
        Life { stacking: true, meet: true, games: true, hide: true, push: false, sleep: true, sleep_minutes: 5, daytime: true, emotes: true, talk: true, dance: true, icons: true, show_names: true, break_minutes: 0, water_minutes: 0 }
    }
}

/// Pets visiting other PCs on the local network.
#[derive(Clone, PartialEq)]
pub struct Lan {
    pub on: bool,
    /// Pets go visiting by themselves now and then.
    pub auto: bool,
    /// How long a visit lasts.
    pub minutes: u32,
    /// Addresses of other PCs, for networks where broadcasts don't get through.
    pub peers: Vec<String>,
    /// How this PC is called on the other screens ("" = computer name).
    pub name: String,
}

#[derive(Clone)]
pub struct Settings {
    pub path: PathBuf,
    /// 0 = follow Windows display scaling.
    pub scaling: f64,
    pub nearest: bool,
    pub opacity: f64,
    pub max_pets: usize,
    pub start_pets: String,
    pub interact: bool,
    pub throw_windows: bool,
    pub keep_on_screen: bool,
    pub shake: bool,
    pub hide_fullscreen: bool,
    /// Fullscreen apps (exe names) that keep pets visible, e.g. Remote Desktop.
    pub fs_show: Vec<String>,
    pub sounds: bool,
    pub cloning: bool,
    /// At most one pet of each kind on screen.
    pub unique_pets: bool,
    /// Offer to import Shimeji zips that appear in the Downloads folder.
    pub watch_downloads: bool,
    /// Look for new Popeto versions on GitHub (updating still asks).
    pub check_updates: bool,
    pub debug: bool,
    pub show_chooser: bool,
    /// No ini existed before this run.
    pub fresh: bool,
    pub blacklist: Vec<String>,
    pub disabled: HashMap<String, HashSet<String>>,
    pub pranks: Pranks,
    pub life: Life,
    /// Hotkey that sends pets away / brings them back, e.g. Ctrl+Shift+F12.
    pub panic_key: String,
    /// Interface and prank-text language: "auto" or a code like "en", "ru".
    pub language: String,
    /// Names given to pets, per pet kind (folder name -> name).
    pub names: HashMap<String, String>,
    pub lan: Lan,
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        let mut s = Settings {
            path: path.to_path_buf(),
            scaling: 0.0,
            nearest: false,
            opacity: 1.0,
            max_pets: 30,
            start_pets: "all".into(),
            interact: true,
            throw_windows: true,
            keep_on_screen: true,
            shake: true,
            hide_fullscreen: true,
            fs_show: vec!["mstsc".into()],
            sounds: true,
            cloning: true,
            unique_pets: false,
            watch_downloads: true,
            check_updates: true,
            debug: false,
            show_chooser: false,
            fresh: !path.exists(),
            blacklist: vec!["bongocat".into()],
            disabled: HashMap::new(),
            pranks: Pranks { level: 1, ..Default::default() },
            life: Life::default(),
            panic_key: "Ctrl+Shift+F12".into(),
            language: "auto".into(),
            names: HashMap::new(),
            lan: Lan { on: false, auto: true, minutes: 5, peers: vec![], name: String::new() },
        };
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let b = |v: &str| matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "on");
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with(';') || line.starts_with('#') { continue; }
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k.to_ascii_lowercase().as_str() {
                "scaling" => s.scaling = if v.eq_ignore_ascii_case("auto") { 0.0 } else { v.parse().unwrap_or(0.0) },
                "filter" => s.nearest = v.eq_ignore_ascii_case("nearest"),
                "opacity" => s.opacity = v.parse::<f64>().unwrap_or(1.0).clamp(0.05, 1.0),
                "maxpets" => s.max_pets = v.parse().unwrap_or(30),
                "startpets" => s.start_pets = v.to_string(),
                "interactwithwindows" => s.interact = b(v),
                "throwwindows" => s.throw_windows = b(v),
                "keepwindowsonscreen" => s.keep_on_screen = b(v),
                "shakewindows" => s.shake = b(v),
                "hideonfullscreen" => s.hide_fullscreen = b(v),
                "showoverfullscreen" => s.fs_show = v.split(',').map(|x| x.trim().trim_end_matches(".exe").to_lowercase()).filter(|x| !x.is_empty()).collect(),
                "sounds" => s.sounds = b(v),
                "cloning" => s.cloning = b(v),
                "uniquepets" => s.unique_pets = b(v),
                "watchdownloads" => s.watch_downloads = b(v),
                "checkupdates" => s.check_updates = b(v),
                "debug" => s.debug = b(v),
                "showpetchooser" => s.show_chooser = b(v),
                "pranklevel" => s.pranks.level = match v.to_ascii_lowercase().as_str() { "calm" => 0, "chaos" => 2, _ => 1 },
                "pranknotes" => s.pranks.notes = b(v),
                "prankpictures" => s.pranks.memes = b(v),
                "prankonlinepictures" => s.pranks.online = b(v),
                "prankfootprints" => s.pranks.footprints = b(v),
                "prankcursor" => s.pranks.cursor = b(v),
                "pranktalk" => s.pranks.talk = b(v),
                "prankhonk" => s.pranks.honk = b(v),
                "prankwindows" => s.pranks.windows = b(v),
                "panickey" => s.panic_key = v.to_string(),
                "lifestacking" => s.life.stacking = b(v),
                "lifemeet" => s.life.meet = b(v),
                "lifegames" => s.life.games = b(v),
                "lifehide" => s.life.hide = b(v),
                "lifepushwindows" => s.life.push = b(v),
                "lifesleepwhenaway" => s.life.sleep = b(v),
                "lifesleepminutes" => s.life.sleep_minutes = v.parse().unwrap_or(5).clamp(1, 240),
                "lifetimeofday" => s.life.daytime = b(v),
                "lifeemotes" => s.life.emotes = b(v),
                "lifetalk" => s.life.talk = b(v),
                "lifedance" => s.life.dance = b(v),
                "lifedesktopicons" => s.life.icons = b(v),
                "lifeshownames" => s.life.show_names = b(v),
                "breakreminderminutes" => s.life.break_minutes = v.parse().unwrap_or(0).min(600),
                "waterreminderminutes" => s.life.water_minutes = v.parse().unwrap_or(0).min(600),
                "language" => s.language = v.to_lowercase(),
                "lanvisits" => s.lan.on = b(v),
                "lanvisitbythemselves" => s.lan.auto = b(v),
                "lanvisitminutes" => s.lan.minutes = v.parse().unwrap_or(5).clamp(1, 240),
                "lanpeers" => s.lan.peers = v.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
                "lanname" => s.lan.name = v.to_string(),
                "ignorewindows" => s.blacklist = v.split(',').map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty()).collect(),
                _ => {
                    if let Some(pack) = k.strip_prefix("Name.") {
                        if !v.is_empty() { s.names.insert(pack.to_string(), v.to_string()); }
                    } else if let Some(pack) = k.strip_prefix("Disabled.") {
                        s.disabled.insert(pack.to_string(), v.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect());
                    }
                }
            }
        }
        if !path.exists() { s.save(); }
        s
    }

    pub fn save(&self) {
        let mut o = String::new();
        o += "; Popeto settings. Edit while Popeto is closed, or use the tray menu.\n\n";
        o += "; Interface and prank language: auto (= Windows language), en, ru\n";
        o += &format!("Language={}\n", self.language);
        o += "; Pet size: auto (= Windows display scaling) or a number like 1.0, 1.5, 2\n";
        o += &format!("Scaling={}\n", if self.scaling <= 0.0 { "auto".to_string() } else { self.scaling.to_string() });
        o += "; Image filter when scaled: smooth or nearest (pixel art)\n";
        o += &format!("Filter={}\n", if self.nearest { "nearest" } else { "smooth" });
        o += &format!("Opacity={}\n", self.opacity);
        o += "; Maximum number of pets (clones included)\n";
        o += &format!("MaxPets={}\n", self.max_pets);
        o += "; Pets to drop at startup: all, or comma separated folder names from img\\\n";
        o += &format!("StartPets={}\n", self.start_pets);
        o += "; Show the pet chooser window every time Popeto starts\n";
        o += &format!("ShowPetChooser={}\n\n", self.show_chooser);
        o += "; Pets walk on, climb, carry and throw your windows. No window titles to configure.\n";
        o += &format!("InteractWithWindows={}\n", self.interact);
        o += &format!("ThrowWindows={}\n", self.throw_windows);
        o += &format!("KeepWindowsOnScreen={}\n", self.keep_on_screen);
        o += &format!("ShakeWindows={}\n", self.shake);
        o += &format!("HideOnFullscreen={}\n", self.hide_fullscreen);
        o += "; Fullscreen apps that still show pets on top (exe names). mstsc = Remote Desktop\n";
        o += &format!("ShowOverFullscreen={}\n", self.fs_show.join(","));
        o += &format!("Sounds={}\n", self.sounds);
        o += &format!("Cloning={}\n", self.cloning);
        o += "; Only one pet of each kind (no duplicates)\n";
        o += &format!("UniquePets={}\n", self.unique_pets);
        o += "; Offer to add Shimeji zips that appear in the Downloads folder\n";
        o += &format!("WatchDownloads={}\n", self.watch_downloads);
        o += "; Look for new Popeto versions on GitHub; updating asks first (tray > Update to ...)\n";
        o += &format!("CheckUpdates={}\n", self.check_updates);
        o += "; Windows pets ignore: exe names (without .exe) or parts of window titles, comma separated\n";
        o += &format!("IgnoreWindows={}\n\n", self.blacklist.join(","));
        let p = &self.pranks;
        o += "; Pranks (all off by default). Texts, pictures and sounds live in the pranks folder.\n";
        o += &format!("PrankLevel={}\n", ["calm", "normal", "chaos"][p.level.min(2) as usize]);
        o += &format!("PrankNotes={}\nPrankPictures={}\nPrankFootprints={}\nPrankCursor={}\nPrankTalk={}\nPrankHonk={}\nPrankWindows={}\n",
            p.notes, p.memes, p.footprints, p.cursor, p.talk, p.honk, p.windows);
        o += "; Pictures prank also brings fresh pictures from the internet (sources: pranks\\*\\online.txt)\n";
        o += &format!("PrankOnlinePictures={}\n", p.online);
        let l = &self.life;
        o += "; Life: pets stand on each other, greet, play tag/follow, hide behind windows, push windows,\n";
        o += "; sleep when you are away (minutes), react to the time of day, show hearts/stars, talk\n";
        o += &format!("LifeStacking={}\nLifeMeet={}\nLifeGames={}\nLifeHide={}\nLifePushWindows={}\nLifeSleepWhenAway={}\nLifeSleepMinutes={}\nLifeTimeOfDay={}\nLifeEmotes={}\nLifeTalk={}\n",
            l.stacking, l.meet, l.games, l.hide, l.push, l.sleep, l.sleep_minutes, l.daytime, l.emotes, l.talk);
        o += "; Dance to music, stand on desktop icons, show names on mouse over\n";
        o += &format!("LifeDance={}\nLifeDesktopIcons={}\nLifeShowNames={}\n", l.dance, l.icons, l.show_names);
        o += "; Reminders: a pet brings a note. Minutes, 0 = off. The break timer restarts when you are away 5+ minutes.\n";
        o += &format!("BreakReminderMinutes={}\nWaterReminderMinutes={}\n", l.break_minutes, l.water_minutes);
        let n = &self.lan;
        o += "; LAN visits: pets walk to other PCs running Popeto on the same network (UDP/TCP port 47811)\n";
        o += "; LanPeers: addresses of other PCs if they are not found automatically. LanName: empty = computer name\n";
        o += &format!("LanVisits={}\nLanVisitByThemselves={}\nLanVisitMinutes={}\nLanPeers={}\nLanName={}\n", n.on, n.auto, n.minutes, n.peers.join(","), n.name);
        o += "; Hotkey that hides all pets and stops pranks (press again to bring them back)\n";
        o += &format!("PanicKey={}\n\n", self.panic_key);
        o += "; Pet names (right click a pet > Name...)\n";
        let mut keys: Vec<_> = self.names.keys().collect();
        keys.sort();
        for k in keys { o += &format!("Name.{k}={}\n", self.names[k]); }
        o += "\n; Behaviours switched off per pet (right click a pet > Allowed behaviours)\n";
        let mut keys: Vec<_> = self.disabled.keys().collect();
        keys.sort();
        for k in keys {
            let mut v: Vec<_> = self.disabled[k].iter().cloned().collect();
            v.sort();
            o += &format!("Disabled.{k}={}\n", v.join(","));
        }
        let _ = std::fs::write(&self.path, o);
    }
}
