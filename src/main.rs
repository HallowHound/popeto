#![windows_subsystem = "windows"]
//! Popeto: portable desktop pets that run Shimeji / Shimeji-ee packs natively. No Java, no install.

mod action;
mod chooser;
mod config;
mod convert;
mod downloads;
mod env;
mod expr;
mod importer;
mod life;
mod i18n;
mod mascot;
mod online;
mod pranks;
mod settings;
mod settings_ui;
mod win;

use config::Pack;
use env::{EnvView, Out, Rect, World};
use expr::random;
use i18n::{t, tf};
use mascot::Mascot;
use settings::Settings;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use win::{Bmp, Ev, Menu};
use windows_sys::Win32::Foundation::{HWND, GetLastError, ERROR_ALREADY_EXISTS};
use windows_sys::Win32::System::Threading::{CreateMutexW, CreateWaitableTimerExW, SetWaitableTimer, INFINITE};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const TICK_MS: f64 = 25.0;

struct PetWin {
    hwnd: HWND,
    shown: bool,
    last: Option<(usize, bool)>,
    pos: (i32, i32),
    size: (i32, i32),
    /// Hidden because a fullscreen app covers its monitor.
    hidden: bool,
}

struct Pet {
    m: Mascot,
    w: PetWin,
}

struct Shake {
    hwnd: isize,
    t: usize,
    base: (i32, i32),
    /// Ticks to wait before starting (for repeated stomps).
    delay: usize,
}

struct App {
    root: PathBuf,
    settings: Settings,
    scale: f64,
    packs: Vec<Pack>,
    bmps: Vec<Vec<Option<Option<Bmp>>>>,
    pets: Vec<Pet>,
    world: World,
    scanner: win::Scanner,
    painter: win::Painter,
    tray: HWND,
    icon: HICON,
    next_id: u32,
    paused: bool,
    tick_n: u64,
    log: Vec<String>,
    logged: HashSet<String>,
    log_dirty: bool,
    originals: HashMap<isize, (i32, i32)>,
    shakes: Vec<Shake>,
    fullscreen: Option<usize>,
    fs_pending: (Option<(usize, String)>, u32),
    fs_settled: Option<(usize, String)>,
    /// Monitor with a fullscreen app that pets stay visible over (its work area = whole screen).
    fs_visible: Option<usize>,
    empty: HashSet<String>,
    quit: bool,
    pranks: pranks::Pranks,
    online: online::Online,
    downloads: downloads::Watcher,
    life: life::Life,
    /// Shimeji zips found in Downloads, waiting for the user to add them.
    pending: Vec<(std::path::PathBuf, Vec<String>)>,
}

// menu ids
const ID_EXIT: usize = 1;
const ID_DISMISS_ALL: usize = 2;
const ID_RESTORE: usize = 3;
const ID_INTERACT: usize = 4;
const ID_THROW: usize = 5;
const ID_KEEP: usize = 6;
const ID_SHAKE: usize = 7;
const ID_FULLSCREEN: usize = 8;
const ID_SOUNDS: usize = 9;
const ID_PAUSE: usize = 10;
const ID_FOLDER: usize = 11;
const ID_RELOAD: usize = 12;
const ID_SETTINGS: usize = 13;
const ID_RANDOM: usize = 14;
const ID_CLONING: usize = 15;
const ID_ALL_CHASE: usize = 16;
const ID_UNIQUE: usize = 24;
const ID_CHASE: usize = 20;
const ID_DISMISS: usize = 21;
const ID_DISMISS_OTHERS: usize = 22;
const ID_ANOTHER: usize = 23;
const ID_CHOOSE: usize = 17;
const ID_SETTINGS_WINDOW: usize = 18;
const ID_TOY_COOKIE: usize = 65;
const ID_TOY_BALLOON: usize = 66;
const ID_TOY_BED: usize = 67;
const ID_TOY_CLEAR: usize = 68;
const ID_ADD_PETS: usize = 19;
const ID_PRANK_LEVEL: usize = 40;
const ID_PRANK_TOGGLE: usize = 50;
const ID_PRANK_NOW: usize = 60;
const ID_BALL: usize = 61;
const ID_BALL_REMOVE: usize = 62;
const ID_PRANK_CLEAR: usize = 63;
const ID_PRANK_FOLDER: usize = 64;
const ID_SIZE: usize = 70;
const ID_LANG: usize = 90;
const ID_OPACITY: usize = 80;
const SIZES: [(f64, &str); 7] = [(0.0, "Auto (Windows display scaling)"), (0.5, "50%"), (0.75, "75%"), (1.0, "100%"), (1.25, "125%"), (1.5, "150%"), (2.0, "200%")];
const OPACITIES: [f64; 4] = [1.0, 0.9, 0.75, 0.5];
const ID_SUMMON: usize = 1000;
/// Tray "Get more pets" links (settings_ui::SITES).
const ID_LINK: usize = 4000;
const ID_PENDING: usize = 4100;

fn scale_for(s: &Settings) -> f64 {
    if s.scaling > 0.0 { s.scaling } else { unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForSystem() as f64 / 96.0 } }
}
const ID_BEHAVE: usize = 2000;
const ID_TOGGLE: usize = 3000;

impl App {
    fn new(root: PathBuf) -> App {
        let settings = Settings::load(&root.join("popeto.ini"));
        let scale = scale_for(&settings);
        let prank_state = pranks::Pranks::new(&root);
        let scanner = win::Scanner::new(settings.blacklist.clone());
        let tray = win::create_tray_window();
        App {
            root,
            settings,
            scale,
            packs: vec![],
            bmps: vec![],
            pets: vec![],
            world: World::default(),
            scanner,
            painter: win::Painter::new(),
            tray,
            icon: null_mut(),
            next_id: 1,
            paused: false,
            tick_n: 0,
            log: vec![],
            logged: HashSet::new(),
            log_dirty: false,
            originals: HashMap::new(),
            shakes: vec![],
            fullscreen: None,
            fs_pending: (None, 0),
            fs_settled: None,
            fs_visible: None,
            empty: HashSet::new(),
            quit: false,
            pranks: prank_state,
            online: online::Online::start(),
            downloads: downloads::Watcher::new(),
            life: life::Life::default(),
            pending: vec![],
        }
    }

    fn note(&mut self, s: String) {
        self.log.push(s);
    }

    fn flush_log(&mut self) {
        for l in self.log.drain(..) {
            if self.logged.len() < 2000 && self.logged.insert(l) { self.log_dirty = true; }
        }
        if self.log_dirty {
            self.log_dirty = false;
            let mut v: Vec<&String> = self.logged.iter().collect();
            v.sort();
            let text: String = v.iter().map(|s| format!("{s}\n")).collect();
            let _ = std::fs::write(self.root.join("popeto.log"), text);
        }
    }

    fn load_packs(&mut self) {
        for p in config::discover(&self.root) {
            match config::load_pack(&self.root, &p, self.scale) {
                Ok((pack, warnings)) => {
                    for w in warnings { self.note(format!("{}: {w}", pack.name)); }
                    self.bmps.push((0..pack.images.len()).map(|_| None).collect());
                    self.packs.push(pack);
                }
                Err(e) => self.note(format!("{}: not loaded: {e}", p.display())),
            }
        }
        if self.icon.is_null() {
            let src = self.root.join("icon.png");
            let p = if src.exists() { Some(src) } else { self.packs.first().and_then(|p| p.icon_img.map(|i| p.images[i].clone())) };
            self.icon = p.map(|p| win::make_icon(&p)).unwrap_or(null_mut());
        }
    }

    fn unload(&mut self) {
        for p in self.pets.drain(..) { win::destroy(p.w.hwnd); }
        for v in self.bmps.drain(..) { for b in v.into_iter().flatten().flatten() { win::free_bmp(&b); } }
        self.packs.clear();
    }

    fn bmp(&mut self, pack: usize, img: usize) -> Option<&Bmp> {
        let slot = &mut self.bmps[pack][img];
        if slot.is_none() {
            let p = &self.packs[pack];
            let b = win::make_bmp(&p.images[img], p.scale, self.settings.nearest);
            if b.is_none() { self.log.push(format!("{}: cannot load {}", p.name, p.images[img].display())); }
            *slot = Some(b);
        }
        self.bmps[pack][img].as_ref().unwrap().as_ref()
    }

    fn pack_index(&self, name: &str) -> Option<usize> {
        self.packs.iter().position(|p| p.name.eq_ignore_ascii_case(name))
    }

    fn counts(&self) -> Vec<usize> {
        let mut c = vec![0; self.packs.len()];
        for p in &self.pets { c[p.m.pack] += 1; }
        c
    }

    fn spawn(&mut self, pack: usize, anchor: Option<(i32, i32)>, look_right: bool, behavior: Option<&str>) {
        if self.pets.len() >= self.settings.max_pets || self.world.monitors.is_empty() { return; }
        if self.settings.unique_pets && self.pets.iter().any(|p| p.m.pack == pack) { return; }
        let anchor = anchor.unwrap_or_else(|| {
            let w = self.world.monitors[0].work;
            (w.l + 100 + (random() * (w.w() - 200).max(1) as f64) as i32, w.t - 128)
        });
        let mut m = Mascot::new(self.next_id, pack, anchor, look_right);
        self.next_id += 1;
        m.st.monitor = self.world.monitor_for(0, anchor.0, anchor.1);
        let count = self.counts()[pack] + 1;
        self.world.total = self.pets.len() + 1;
        let p = &self.packs[pack];
        let disabled = self.settings.disabled.get(&p.name).unwrap_or(&self.empty);
        let mut env = EnvView::new(&self.world, m.st.monitor, None, count);
        env.self_id = m.id;
        let b = behavior.map(|s| s.to_string()).unwrap_or_else(|| p.required("Fall"));
        m.set_behavior(p, &b, &mut env, disabled, &mut self.log);
        let hwnd = win::create_pet_window();
        self.pets.push(Pet { m, w: PetWin { hwnd, shown: false, last: None, pos: (0, 0), size: (0, 0), hidden: false } });
    }

    /// Which packs are switched on by `StartPets`.
    fn active_set(&self) -> Vec<bool> {
        let want = self.settings.start_pets.trim();
        if want.is_empty() || want.eq_ignore_ascii_case("all") { return vec![true; self.packs.len()]; }
        let names: Vec<&str> = want.split(',').map(|n| n.trim()).collect();
        let v: Vec<bool> = self.packs.iter().map(|p| names.iter().any(|n| n.eq_ignore_ascii_case(&p.name))).collect();
        if v.iter().any(|b| *b) { v } else { vec![true; self.packs.len()] }
    }

    fn choose_pets(&mut self, startup: bool) {
        let items: Vec<chooser::Choice> = self.packs.iter().zip(self.active_set()).map(|(p, selected)| {
            let pv = p.dir.join("preview.png");
            let preview = if pv.exists() { Some(pv) } else { p.icon_img.map(|i| p.images[i].clone()) };
            chooser::Choice { name: p.name.clone(), preview, selected }
        }).collect();
        let Some((chosen, show)) = chooser::run(&items, self.settings.show_chooser, self.icon) else { return };
        self.settings.show_chooser = show;
        self.settings.start_pets = if chosen.iter().all(|c| *c) {
            "all".into()
        } else {
            self.packs.iter().zip(&chosen).filter(|(_, c)| **c).map(|(p, _)| p.name.clone()).collect::<Vec<_>>().join(",")
        };
        self.settings.save();
        if startup { return; }
        let mut k = 0;
        while k < self.pets.len() { if !chosen[self.pets[k].m.pack] { self.remove(k); } else { k += 1; } }
        let counts = self.counts();
        for (i, c) in chosen.iter().enumerate() {
            if *c && counts[i] == 0 { self.spawn(i, None, random() < 0.5, None); }
        }
    }

    fn spawn_start(&mut self) {
        let want = self.settings.start_pets.trim().to_string();
        if want.is_empty() || want.eq_ignore_ascii_case("all") {
            for i in 0..self.packs.len() { self.spawn(i, None, random() < 0.5, None); }
        } else {
            for n in want.split(',') {
                if let Some(i) = self.pack_index(n.trim()) { self.spawn(i, None, random() < 0.5, None); }
            }
        }
        if self.pets.is_empty() && !self.packs.is_empty() { self.spawn(0, None, false, None); }
    }

    fn remove(&mut self, i: usize) {
        let p = self.pets.remove(i);
        win::destroy(p.w.hwnd);
    }

    /// Run `f` on pet `i` with its environment.
    fn with_pet<R>(&mut self, i: usize, f: impl FnOnce(&mut Mascot, &Pack, &mut EnvView, &HashSet<String>, &mut Vec<String>) -> R) -> R {
        let counts = self.counts();
        self.world.total = self.pets.len();
        let pet = &mut self.pets[i];
        let pack = &self.packs[pet.m.pack];
        let disabled = self.settings.disabled.get(&pack.name).unwrap_or(&self.empty);
        pet.m.st.monitor = self.world.monitor_for(pet.m.st.monitor, pet.m.st.anchor.0, pet.m.st.anchor.1);
        let mut env = EnvView::new(&self.world, pet.m.st.monitor, pet.m.st.ie, counts[pet.m.pack]);
        env.self_id = pet.m.id;
        f(&mut pet.m, pack, &mut env, disabled, &mut self.log)
    }

    fn set_behavior(&mut self, i: usize, name: &str) {
        let name = name.to_string();
        self.with_pet(i, |m, pack, env, dis, log| {
            let n = pack.required(&name);
            m.set_behavior(pack, &n, env, dis, log)
        });
    }

    fn hotspot_at(&mut self, i: usize, x: i32, y: i32) -> Option<Option<String>> {
        let (pack, img, lr) = {
            let m = &self.pets[i].m;
            (m.pack, m.st.frame.as_ref()?.img, m.st.look_right)
        };
        let w = self.bmp(pack, img)?.w;
        let ix = if lr { w - 1 - x } else { x };
        self.with_pet(i, |m, _, env, _, _| {
            let (anim, _) = m.current(env)?;
            anim.hotspots.iter().find(|h| {
                if h.ellipse {
                    let (cx, cy, rx, ry) = (h.x as f64 + h.w as f64 / 2.0, h.y as f64 + h.h as f64 / 2.0, h.w as f64 / 2.0, h.h as f64 / 2.0);
                    rx > 0.0 && ry > 0.0 && ((ix as f64 - cx) / rx).powi(2) + ((y as f64 - cy) / ry).powi(2) <= 1.0
                } else {
                    ix >= h.x && ix < h.x + h.w && y >= h.y && y < h.y + h.h
                }
            }).map(|h| h.behavior.clone())
        })
    }

    fn find(&self, hwnd: isize) -> Option<usize> { self.pets.iter().position(|p| p.w.hwnd as isize == hwnd) }

    // ------------------------------------------------------------ input

    fn on_event(&mut self, e: Ev) {
        match e {
            Ev::Press(h, x, y) => {
                if self.toy_press(h) { return; }
                if self.pranks.is_ball(h) { self.grab_ball(true); return; }
                let Some(i) = self.find(h) else { return };
                if self.paused { return; }
                self.honk();
                match self.hotspot_at(i, x, y) {
                    Some(Some(b)) if self.packs[self.pets[i].m.pack].behavior(&b).is_some() => {
                        self.set_behavior(i, &b);
                        let m = &mut self.pets[i].m;
                        m.hotspot_beh = true;
                        m.holding = true;
                        win::capture(h as HWND);
                        let id = m.id;
                        self.say(id, "petted");
                        self.emote(id, life::Emote::Heart);
                    }
                    _ => {
                        let drag = self.with_pet(i, |m, _, env, _, _| m.current(env).map(|c| c.1).unwrap_or(true));
                        if drag {
                            self.pets[i].m.st.dragging = true;
                            self.set_behavior(i, "Dragged");
                            win::capture(h as HWND);
                        }
                    }
                }
            }
            Ev::Release(h) => {
                if self.toy_release(h) { return; }
                if self.pranks.is_ball(h) { if self.pranks.ball.as_ref().map(|b| b.dragging).unwrap_or(false) { self.grab_ball(false); } return; }
                let Some(i) = self.find(h) else { return };
                let m = &mut self.pets[i].m;
                let (dragging, holding, hb) = (m.st.dragging, m.holding, m.hotspot_beh);
                m.holding = false;
                if dragging {
                    m.st.dragging = false;
                    let id = m.id;
                    self.set_behavior(i, "Thrown");
                    let d = self.world.cursor_d;
                    if d.0.abs() + d.1.abs() > 25 { self.say(id, "thrown"); self.emote(id, life::Emote::Bang); }
                } else if holding && hb {
                    self.with_pet(i, |m, pack, env, dis, log| m.next_after_hotspot(pack, env, dis, log));
                }
                if dragging || holding { win::release_capture(); }
            }
            Ev::RClick(h) => {
                if self.remove_toy(h) { return; }
                if self.pranks.is_ball(h) { self.remove_ball(); return; }
                if let Some(i) = self.find(h) { self.pet_menu(i); }
            }
            Ev::CloseNote(h) => self.pranks.close_note(h),
            Ev::Panic => self.panic(),
            Ev::SettingsApply(close) => {
                if let Some(new) = settings_ui::read(&self.settings) { self.apply_settings(new); }
                if close { settings_ui::close(); }
            }
            Ev::Command(id) => self.command(id, None),
            Ev::Import(p) => self.import_from(std::path::Path::new(&p)),
            Ev::Tray(msg) => match msg {
                win::MSG_RUP | win::MSG_CTX => self.tray_menu(),
                win::MSG_LDBL => self.summon_random(),
                win::MSG_BALLOON_CLICK => if !self.pending.is_empty() { let (p, _) = self.pending.remove(0); self.import_from(&p); },
                _ => {}
            },
            Ev::TaskbarCreated => win::tray(self.tray, self.icon, win::TRAY_ADD),
            Ev::Display => self.refresh_monitors(),
        }
    }

    fn summon_random(&mut self) {
        let counts = self.counts();
        // with unique pets, pick only among kinds that aren't on screen yet
        let free: Vec<usize> = (0..self.packs.len()).filter(|&i| !self.settings.unique_pets || counts[i] == 0).collect();
        if free.is_empty() { return; }
        let i = free[(random() * free.len() as f64) as usize % free.len()];
        self.spawn(i, None, random() < 0.5, None);
    }

    fn tray_menu(&mut self) {
        let s = &self.settings;
        let summon = Menu::new();
        let counts = self.counts();
        // with unique pets, a tick marks kinds already on screen
        for (i, p) in self.packs.iter().enumerate() { summon.item(ID_SUMMON + i, &p.name, s.unique_pets && counts[i] > 0); }
        summon.sep().item(ID_RANDOM, t("Random pet"), false);
        let opts = Menu::new();
        opts.item(ID_INTERACT, t("Play with windows"), s.interact)
            .item(ID_THROW, t("Allow carrying && throwing windows"), s.throw_windows)
            .item(ID_KEEP, t("Keep thrown windows on screen"), s.keep_on_screen)
            .item(ID_SHAKE, t("Shake windows on hard landing"), s.shake)
            .item(ID_CLONING, t("Allow cloning"), s.cloning)
            .item(ID_UNIQUE, t("Only one of each pet"), s.unique_pets)
            .item(ID_FULLSCREEN, t("Hide during fullscreen apps"), s.hide_fullscreen)
            .item(ID_SOUNDS, t("Sounds"), s.sounds);
        let size = Menu::new();
        for (k, (v, label)) in SIZES.iter().enumerate() { size.item(ID_SIZE + k, t(label), (s.scaling - v).abs() < 1e-6); }
        let opac = Menu::new();
        for (k, v) in OPACITIES.iter().enumerate() { opac.item(ID_OPACITY + k, &format!("{}%", (v * 100.0) as i32), (s.opacity - v).abs() < 1e-6); }
        let lang = Menu::new();
        lang.item(ID_LANG, t("Auto (Windows language)"), s.language.eq_ignore_ascii_case("auto"));
        for (k, (code, name)) in i18n::LANGUAGES.iter().enumerate() { lang.item(ID_LANG + 1 + k, name, s.language.eq_ignore_ascii_case(code)); }
        opts.sep().sub(t("Size"), size).sub(t("Opacity"), opac).sub(t("Language"), lang);
        let p = &s.pranks;
        let (notes, prints) = self.pranks.counts();
        let lvl = Menu::new();
        for (k, n) in ["Calm", "Normal", "Chaos"].iter().enumerate() { lvl.item(ID_PRANK_LEVEL + k, t(n), p.level as usize == k); }
        let pr = Menu::new();
        pr.item(ID_PRANK_TOGGLE, t("Bring notes"), p.notes)
            .item(ID_PRANK_TOGGLE + 1, t("Bring pictures"), p.memes)
            .item(ID_PRANK_TOGGLE + 7, t("    ...also fresh ones from the internet (not saved)"), p.online)
            .item(ID_PRANK_TOGGLE + 2, t("Muddy footprints"), p.footprints)
            .item(ID_PRANK_TOGGLE + 3, t("Grab the cursor (shake mouse to escape)"), p.cursor)
            .item(ID_PRANK_TOGGLE + 4, t("Speech bubbles"), p.talk)
            .item(ID_PRANK_TOGGLE + 5, t("Honk"), p.honk)
            .item(ID_PRANK_TOGGLE + 6, t("Stomp && knock down windows"), p.windows)
            .sep()
            .sub(t("How often"), lvl)
            .item(ID_PRANK_NOW, t("Do a prank now"), false)
            .item(if self.pranks.ball.is_some() { ID_BALL_REMOVE } else { ID_BALL }, if self.pranks.ball.is_some() { t("Take the ball away") } else { t("Throw a ball") }, false)
            .item(ID_PRANK_CLEAR, &tf("Clean up notes && footprints ({})", &[&(notes + prints)]), false)
            .item(ID_PRANK_FOLDER, t("Open pranks folder"), false)
            .sep()
            .disabled(&tf("Panic key: {}", &[&s.panic_key]));
        let toys = Menu::new();
        toys.item(ID_TOY_COOKIE, t("Drop a cookie"), false)
            .item(if self.pranks.ball.is_some() { ID_BALL_REMOVE } else { ID_BALL }, if self.pranks.ball.is_some() { t("Take the ball away") } else { t("Throw a ball") }, false)
            .item(ID_TOY_BALLOON, t("Release a balloon"), false)
            .item(ID_TOY_BED, t("Bed"), self.life.has_bed())
            .sep()
            .item(ID_TOY_CLEAR, t("Remove all toys"), false);
        let links = Menu::new();
        for (k, (name, _, _)) in settings_ui::SITES.iter().enumerate() { links.item(ID_LINK + k, name, false); }
        let m = Menu::new();
        m.disabled(&tf("Popeto - {} pet(s)", &[&self.pets.len()])).sep();
        for (k, (_, names)) in self.pending.iter().enumerate() {
            m.item(ID_PENDING + k, &tf("Add downloaded pet: {}", &[&names.join(", ")]), false);
        }
        if !self.pending.is_empty() { m.sep(); }
        m
            .item(ID_SETTINGS_WINDOW, t("Settings..."), false)
            .item(ID_CHOOSE, t("Choose pets..."), false)
            .item(ID_ADD_PETS, t("Add pets from Shimeji zip..."), false)
            .sub(t("Get more pets"), links)
            .sub(t("Summon"), summon)
            .item(ID_ALL_CHASE, t("Everyone: follow cursor"), false)
            .item(ID_RESTORE, &tf("Restore moved windows ({})", &[&self.originals.len()]), false)
            .item(ID_DISMISS_ALL, t("Dismiss all"), false)
            .item(ID_PAUSE, t("Pause"), self.paused)
            .sep()
            .sub(t("Toys"), toys)
            .sub(t("Pranks"), pr)
            .sub(t("Options"), opts)
            .item(ID_FOLDER, t("Open pets folder"), false)
            .item(ID_SETTINGS, t("Edit settings file"), false)
            .item(ID_RELOAD, t("Reload pets"), false)
            .sep()
            .item(ID_EXIT, t("Exit"), false);
        let id = m.run(self.tray);
        self.command(id, None);
    }

    fn pet_menu(&mut self, i: usize) {
        let pack = &self.packs[self.pets[i].m.pack];
        let dis = self.settings.disabled.get(&pack.name).unwrap_or(&self.empty);
        let mut names: Vec<&str> = Vec::new();
        let mut toggles: Vec<&str> = Vec::new();
        for b in &pack.behaviors {
            if !b.hidden && !names.contains(&b.name.as_str()) { names.push(&b.name); }
            if b.toggleable && !toggles.contains(&b.name.as_str()) { toggles.push(&b.name); }
        }
        let bm = Menu::new();
        for (k, n) in names.iter().enumerate() { bm.item(ID_BEHAVE + k, n, false); }
        let tm = Menu::new();
        for (k, n) in toggles.iter().enumerate() { tm.item(ID_TOGGLE + k, n, !dis.contains(*n)); }
        let m = Menu::new();
        m.disabled(&pack.name).sep()
            .item(ID_CHASE, t("Follow cursor"), false)
            .item(ID_SETTINGS_WINDOW, t("Settings..."), false)
            .sub(t("Do something"), bm);
        if !toggles.is_empty() { m.sub(t("Allowed behaviours"), tm); }
        m.sep();
        if !self.settings.unique_pets { m.item(ID_ANOTHER, &tf("Another {}", &[&pack.name]), false); }
        m.item(ID_RESTORE, t("Restore moved windows"), false)
            .sep()
            .item(ID_DISMISS, t("Dismiss"), false)
            .item(ID_DISMISS_OTHERS, t("Dismiss others"), false)
            .item(ID_DISMISS_ALL, t("Dismiss all"), false);
        let names: Vec<String> = names.iter().map(|s| s.to_string()).collect();
        let toggles: Vec<String> = toggles.iter().map(|s| s.to_string()).collect();
        let owner = self.pets[i].w.hwnd;
        let id = m.run(owner);
        let pid = self.pets[i].m.id;
        if id >= ID_TOGGLE && id < ID_TOGGLE + toggles.len() {
            let pname = self.packs[self.pets[i].m.pack].name.clone();
            let set = self.settings.disabled.entry(pname).or_default();
            let n = &toggles[id - ID_TOGGLE];
            if !set.remove(n) { set.insert(n.clone()); }
            self.settings.save();
        } else if id >= ID_BEHAVE && id < ID_BEHAVE + names.len() {
            self.set_behavior(i, &names[id - ID_BEHAVE]);
        } else {
            self.command(id, Some(pid));
        }
    }

    fn command(&mut self, id: usize, pet: Option<u32>) {
        let idx = pet.and_then(|p| self.pets.iter().position(|x| x.m.id == p));
        let s = &mut self.settings;
        match id {
            ID_EXIT => self.quit = true,
            ID_DISMISS_ALL => while !self.pets.is_empty() { self.remove(0); },
            ID_RESTORE => {
                for (h, (l, t)) in self.originals.drain() { win::move_app_window(h, l, t); }
            }
            ID_INTERACT => { s.interact = !s.interact; s.save(); }
            ID_THROW => { s.throw_windows = !s.throw_windows; s.save(); }
            ID_KEEP => { s.keep_on_screen = !s.keep_on_screen; s.save(); }
            ID_SHAKE => { s.shake = !s.shake; s.save(); }
            ID_CLONING => { s.cloning = !s.cloning; s.save(); }
            ID_UNIQUE => {
                let mut n = self.settings.clone();
                n.unique_pets = !n.unique_pets;
                self.apply_settings(n);
            }
            ID_FULLSCREEN => { s.hide_fullscreen = !s.hide_fullscreen; s.save(); }
            ID_SOUNDS => { s.sounds = !s.sounds; s.save(); }
            ID_PAUSE => self.paused = !self.paused,
            ID_FOLDER => win::open(&self.root.join("img")),
            ID_SETTINGS => win::open(&self.settings.path),
            ID_SETTINGS_WINDOW => settings_ui::open(&self.settings, self.icon),
            _ if (ID_LINK..ID_LINK + settings_ui::SITES.len()).contains(&id) => win::open_url(settings_ui::SITES[id - ID_LINK].1),
            _ if (ID_PENDING..ID_PENDING + self.pending.len()).contains(&id) => {
                let (p, _) = self.pending.remove(id - ID_PENDING);
                self.import_from(&p);
            }
            ID_ADD_PETS => {
                let pick = win::open_file(self.tray, t("Choose a Shimeji zip, or any file inside an unpacked Shimeji folder"),
                    &[(t("Shimeji pets (zip, Shimeji-ee.jar, png)"), "*.zip;*.jar;*.png"), (t("All files"), "*.*")]);
                if let Some(p) = pick { self.import_from(&p); }
            }
            _ if (ID_LANG..ID_LANG + 1 + i18n::LANGUAGES.len()).contains(&id) => {
                let mut n = self.settings.clone();
                n.language = if id == ID_LANG { "auto".into() } else { i18n::LANGUAGES[id - ID_LANG - 1].0.into() };
                self.apply_settings(n);
            }
            ID_PRANK_NOW => {
                pranks::ensure_files(&self.root);
                self.prank_now(None);
            }
            ID_BALL => self.throw_ball(),
            ID_TOY_COOKIE => self.drop_toy(life::ToyKind::Cookie),
            ID_TOY_BALLOON => self.drop_toy(life::ToyKind::Balloon),
            ID_TOY_BED => self.drop_toy(life::ToyKind::Bed),
            ID_TOY_CLEAR => { self.clear_toys(); self.remove_ball(); }
            ID_BALL_REMOVE => self.remove_ball(),
            ID_PRANK_CLEAR => self.pranks.clear(),
            ID_PRANK_FOLDER => { let d = pranks::ensure_files(&self.root); win::open(&d); }
            _ if (ID_PRANK_LEVEL..ID_PRANK_LEVEL + 3).contains(&id) => { s.pranks.level = (id - ID_PRANK_LEVEL) as u8; s.save(); }
            _ if (ID_PRANK_TOGGLE..ID_PRANK_TOGGLE + 8).contains(&id) => {
                let p = &mut s.pranks;
                let f = [&mut p.notes, &mut p.memes, &mut p.footprints, &mut p.cursor, &mut p.talk, &mut p.honk, &mut p.windows, &mut p.online];
                let k = id - ID_PRANK_TOGGLE;
                *f[k] = !*f[k];
                s.save();
                pranks::ensure_files(&self.root);
                self.configure_online();
            }
            _ if (ID_SIZE..ID_SIZE + SIZES.len()).contains(&id) => {
                s.scaling = SIZES[id - ID_SIZE].0;
                s.save();
                self.reload_keep();
            }
            _ if (ID_OPACITY..ID_OPACITY + OPACITIES.len()).contains(&id) => {
                s.opacity = OPACITIES[id - ID_OPACITY];
                s.save();
                for p in &mut self.pets { p.w.last = None; }
            }
            ID_RELOAD => {
                self.unload();
                let path = self.settings.path.clone();
                self.settings = Settings::load(&path);
                self.scanner.blacklist = self.settings.blacklist.clone();
                self.load_packs();
                self.spawn_start();
            }
            ID_RANDOM => self.summon_random(),
            ID_CHOOSE => self.choose_pets(false),
            ID_ALL_CHASE => for i in 0..self.pets.len() { self.set_behavior(i, "ChaseMouse"); },
            ID_CHASE => if let Some(i) = idx { self.set_behavior(i, "ChaseMouse"); },
            ID_DISMISS => if let Some(i) = idx { self.remove(i); },
            ID_DISMISS_OTHERS => if let Some(p) = pet {
                let mut k = 0;
                while k < self.pets.len() { if self.pets[k].m.id != p { self.remove(k); } else { k += 1; } }
            },
            ID_ANOTHER => if let Some(i) = idx { let pk = self.pets[i].m.pack; self.spawn(pk, None, random() < 0.5, None); },
            _ if id >= ID_SUMMON && id < ID_SUMMON + self.packs.len() => self.spawn(id - ID_SUMMON, None, random() < 0.5, None),
            _ => {}
        }
    }

    // ------------------------------------------------------------ simulation

    fn tick(&mut self) {
        self.tick_n += 1;
        let c = win::cursor();
        self.world.cursor_d = (c.0 - self.world.cursor.0, c.1 - self.world.cursor.1);
        self.world.cursor = c;
        if self.world.monitors.is_empty() || self.tick_n % 200 == 0 { self.refresh_monitors(); }
        if self.world.monitors.is_empty() { return; }
        if self.settings.interact { self.scanner.scan(&mut self.world); } else { self.world.wins.clear(); }
        if self.tick_n % 5 == 0 {
            // only hide after a fullscreen app has stayed in front ~0.4s, so shell popups don't flash pets
            let cand = self.scanner.fullscreen(&self.world);
            if cand == self.fs_pending.0 { self.fs_pending.1 += 1; } else { self.fs_pending = (cand.clone(), 1); }
            if (cand.is_none() || self.fs_pending.1 >= 3) && cand != self.fs_settled {
                self.fs_settled = cand.clone();
                let show = cand.as_ref().map(|(_, exe)| !self.settings.hide_fullscreen || self.settings.fs_show.contains(exe)).unwrap_or(false);
                self.fullscreen = if show { None } else { cand.as_ref().map(|c| c.0) };
                let vis = if show { cand.map(|c| c.0) } else { None };
                if vis != self.fs_visible {
                    self.fs_visible = vis;
                    self.refresh_monitors();
                    self.restack();
                }
            }
        }
        self.update_shakes();
        if !self.paused { self.pranks_before(); }
        if self.settings.debug && self.tick_n % 200 == 0 {
            let mut d = format!("[debug t={}] {} windows:", self.tick_n, self.world.wins.len());
            for w in &self.world.wins { d += &format!(" ({},{} {}x{})", w.rect.l, w.rect.t, w.rect.w(), w.rect.h()); }
            for p in &self.pets { d += &format!(" | {} {} at {:?} ie={:?}", self.packs[p.m.pack].name, p.m.beh, p.m.st.anchor, p.m.st.ie.is_some()); }
            self.log.push(d);
        }
        if self.settings.watch_downloads && self.tick_n % 120 == 60 {
            for (p, names) in self.downloads.poll() {
                if self.pending.iter().any(|(q, _)| *q == p) { continue; }
                win::tray_balloon(self.tray, "Popeto", &tf("New pet in Downloads: {}. Click here to add it.", &[&names.join(", ")]));
                self.pending.push((p, names));
            }
        }
        if self.settings.debug {
            if let Some(e) = self.online.take_error() { self.log.push(format!("[online t={}] {e}", self.tick_n)); }
        }
        if self.paused { return; }

        self.life_before();
        let mut outs: Vec<(u32, Out)> = Vec::new();
        for i in 0..self.pets.len() {
            let mon = self.pets[i].m.st.monitor;
            let dragging = self.pets[i].m.st.dragging;
            let w = &mut self.pets[i].w;
            if self.fullscreen == Some(mon) && !dragging {
                if !w.hidden { win::show(w.hwnd, false); w.shown = false; w.hidden = true; }
                continue;
            }
            w.hidden = false;
            if self.pets[i].m.holding {
                let (c, pos) = (self.world.cursor, self.pets[i].w.pos);
                if let Some(Some(b)) = self.hotspot_at(i, c.0 - pos.0, c.1 - pos.1) {
                    if b != self.pets[i].m.beh && self.packs[self.pets[i].m.pack].behavior(&b).is_some() {
                        self.set_behavior(i, &b);
                        self.pets[i].m.hotspot_beh = true;
                    }
                }
            }
            self.with_pet(i, |m, pack, env, dis, log| m.tick(pack, env, dis, log));
            let m = &mut self.pets[i].m;
            outs.extend(m.st.out.drain(..).map(|o| (m.id, o)));
        }
        self.apply(outs);
        self.pranks_after();
        self.life_after();
        self.render();
        // fullscreen apps like RDP sit in the topmost band too, so re-assert often while one is up
        if self.tick_n % if self.fs_visible.is_some() { 8 } else { 80 } == 0 { self.restack(); }
        if !self.log.is_empty() { self.flush_log(); }
    }

    /// Re-read monitors; under a fullscreen app the taskbar is covered, so pets get the whole screen.
    fn refresh_monitors(&mut self) {
        self.world.monitors = win::monitors();
        if let Some(m) = self.fs_visible.and_then(|i| self.world.monitors.get_mut(i)) { m.work = m.screen; }
    }

    /// Import pets from a Shimeji download, then drop one of each new pet on screen.
    fn import_from(&mut self, src: &std::path::Path) {
        self.pending.retain(|(p, _)| p != src);
        let rep = importer::import(src, &self.root);
        if !rep.added.is_empty() {
            let want = self.settings.start_pets.trim().to_string();
            if !(want.is_empty() || want.eq_ignore_ascii_case("all")) {
                self.settings.start_pets = format!("{want},{}", rep.added.join(","));
                self.settings.save();
            }
            self.reload_keep();
            for n in &rep.added {
                if let Some(i) = self.pack_index(n) { self.spawn(i, None, random() < 0.5, None); }
            }
        }
        let mut msg = String::new();
        if !rep.added.is_empty() { msg += &tf("Pets added: {}", &[&rep.added.join(", ")]); }
        if !rep.failed.is_empty() {
            if !msg.is_empty() { msg += "\n\n"; }
            msg += &tf("Could not add:\n{}", &[&rep.failed.join("\n")]);
        }
        win::message(&msg);
    }

    /// Take over settings from the settings window and apply what changed.
    fn apply_settings(&mut self, new: Settings) {
        let old = std::mem::replace(&mut self.settings, new);
        let s = &self.settings;
        s.save();
        if old.language != s.language {
            i18n::set(&s.language);
            // rebuild the settings window in the new language
            if settings_ui::close() { settings_ui::open(s, self.icon); }
        }
        self.scanner.blacklist = s.blacklist.clone();
        if old.panic_key != s.panic_key { win::register_hotkey(self.tray, &s.panic_key); }
        let p = &s.pranks;
        if p.notes || p.memes || p.footprints || p.cursor || p.talk || p.honk || p.windows { pranks::ensure_files(&self.root); }
        // force the fullscreen check to re-decide with the new lists
        self.fs_settled = Some((usize::MAX, String::new()));
        if old.scaling != s.scaling || old.nearest != s.nearest {
            self.reload_keep();
        } else if old.opacity != s.opacity {
            for p in &mut self.pets { p.w.last = None; }
        }
        self.trim_to_max();
        if self.settings.unique_pets { self.dedupe(); }
        self.configure_online();
    }

    /// Start or stop fetching internet pictures; sources follow the current language.
    fn configure_online(&mut self) {
        let p = &self.settings.pranks;
        let on = p.online && p.memes;
        if on { pranks::ensure_files(&self.root); }
        self.online.configure(on, if on { self.pranks.online_sources() } else { vec![] });
    }

    /// Unique pets mode: keep the oldest pet of each kind, dismiss the rest.
    fn dedupe(&mut self) {
        let mut seen = HashSet::new();
        let mut k = 0;
        while k < self.pets.len() {
            if seen.insert(self.pets[k].m.pack) { k += 1; } else { self.remove(k); }
        }
    }

    /// Dismiss pets above MaxPets: newest first, and clones before the last pet of a kind.
    fn trim_to_max(&mut self) {
        while self.pets.len() > self.settings.max_pets.max(1) {
            let counts = self.counts();
            let i = (0..self.pets.len()).rev().find(|&i| counts[self.pets[i].m.pack] > 1).unwrap_or(self.pets.len() - 1);
            self.remove(i);
        }
    }

    /// Keep every Popeto window above other apps in a fixed order: bubbles/ball, pets (newest on top),
    /// notes and footprints. Done atomically so overlapping pets never flicker between layers.
    fn restack(&mut self) {
        let (top, bottom) = self.pranks.layers();
        let (ltop, lbottom) = self.life.layers();
        let mut all = top;
        all.extend(ltop);
        all.extend(self.pets.iter().rev().filter(|p| !self.life.hidden.contains_key(&p.m.id)).map(|p| p.w.hwnd));
        all.extend(bottom);
        all.extend(lbottom);
        win::restack(&all);
    }

    /// Reload packs (e.g. after a size change) and put the same pets back where they were.
    fn reload_keep(&mut self) {
        let keep: Vec<(String, (i32, i32), bool)> = self.pets.iter().map(|p| (self.packs[p.m.pack].name.clone(), p.m.st.anchor, p.m.st.look_right)).collect();
        self.pranks.clear();
        self.unload();
        self.scale = scale_for(&self.settings);
        self.load_packs();
        for (name, a, lr) in keep {
            if let Some(i) = self.pack_index(&name) { self.spawn(i, Some((a.0, a.1 - 4)), lr, None); }
        }
    }

    fn screen_union(&self) -> Rect {
        self.world.monitors.iter().fold(self.world.monitors[0].work, |a, m| Rect { l: a.l.min(m.work.l), t: a.t.min(m.work.t), r: a.r.max(m.work.r), b: a.b.max(m.work.b) })
    }

    fn apply(&mut self, outs: Vec<(u32, Out)>) {
        for (id, o) in outs {
            let idx = self.pets.iter().position(|p| p.m.id == id);
            match o {
                Out::Spawn { pack, anchor, look_right, behavior } => {
                    if !self.settings.cloning { continue; }
                    let Some(i) = idx else { continue };
                    let pk = pack.and_then(|n| self.pack_index(&n)).unwrap_or(self.pets[i].m.pack);
                    self.spawn(pk, Some(anchor), look_right, Some(&behavior));
                }
                Out::Transform { pack, behavior } => {
                    let Some(i) = idx else { continue };
                    if let Some(pk) = pack.and_then(|n| self.pack_index(&n)) { self.pets[i].m.pack = pk; }
                    self.set_behavior(i, &behavior);
                }
                Out::Remove => if let Some(i) = idx { self.remove(i); },
                Out::Sound(p) => if self.settings.sounds { win::play_sound(&p); },
                Out::SetBehavior { pet, behavior, face_x } => {
                    let Some(i) = pet.map(|p| self.pets.iter().position(|x| x.m.id == p)).unwrap_or(idx) else { continue };
                    if let Some(fx) = face_x { self.pets[i].m.st.look_right = self.pets[i].m.st.anchor.0 < fx; }
                    if self.packs[self.pets[i].m.pack].behavior(&behavior).is_some() { self.set_behavior(i, &behavior); }
                }
                Out::MoveWindow { hwnd, l, t } => {
                    // negative handles are pets' heads (stacking), not windows
                    if !self.settings.throw_windows || hwnd < 0 { continue; }
                    let (mut l, mut t) = (l, t);
                    if self.settings.keep_on_screen {
                        let u = self.screen_union();
                        let r = self.world.win(hwnd).map(|z| self.world.wins[z].rect).unwrap_or_default();
                        l = l.clamp(u.l - r.w() + 150, (u.r - 150).max(u.l - r.w() + 150));
                        t = t.clamp(u.t, (u.b - 60).max(u.t));
                    }
                    if self.shakes.iter().any(|s| s.hwnd == hwnd) { continue; }
                    if let Some(orig) = win::move_app_window(hwnd, l, t) { self.originals.entry(hwnd).or_insert(orig); }
                }
                Out::Shake(h) => {
                    if self.settings.shake && !self.shakes.iter().any(|s| s.hwnd == h) {
                        if let Some(r) = self.world.win(h).map(|z| self.world.wins[z].rect) {
                            self.shakes.push(Shake { hwnd: h, t: 0, base: (r.l, r.t), delay: 0 });
                        }
                    }
                }
            }
        }
    }

    fn update_shakes(&mut self) {
        const OFF: [(i32, i32); 12] = [(0, 6), (7, 3), (-7, 2), (6, 1), (-6, 0), (4, 0), (-4, 0), (3, 0), (-3, 0), (1, 0), (-1, 0), (0, 0)];
        self.shakes.retain_mut(|s| {
            if s.delay > 0 { s.delay -= 1; return true; }
            let (dx, dy) = OFF[s.t];
            win::move_app_window(s.hwnd, s.base.0 + dx, s.base.1 + dy);
            s.t += 1;
            s.t < OFF.len()
        });
    }

    fn render(&mut self) {
        let alpha = (self.settings.opacity * 255.0) as u8;
        for i in 0..self.pets.len() {
            let (pack, frame, anchor) = {
                let m = &self.pets[i].m;
                (m.pack, m.st.frame.as_ref().map(|f| (f.img, f.flip, f.mirror_anchor, f.ax, f.ay)), m.st.anchor)
            };
            let Some((img, flip, mirror, ax, ay)) = frame else {
                let w = &mut self.pets[i].w;
                if w.shown { win::show(w.hwnd, false); w.shown = false; }
                continue;
            };
            if self.pets[i].w.hidden { continue; }
            let Some(b) = self.bmp(pack, img) else { continue };
            let (bw, bh, bm) = (b.w, b.h, if flip { b.flipped } else { b.normal });
            let x = if mirror { anchor.0 - (bw - ax) } else { anchor.0 - ax };
            let y = anchor.1 - ay;
            let w = &mut self.pets[i].w;
            if w.last != Some((img, flip)) || !w.shown || w.size != (bw, bh) {
                self.painter.update(w.hwnd, bm, x, y, bw, bh, alpha);
                w.last = Some((img, flip));
                w.size = (bw, bh);
                if !w.shown { win::show(w.hwnd, true); w.shown = true; }
            } else if w.pos != (x, y) {
                win::move_window(w.hwnd, x, y);
            }
            w.pos = (x, y);
        }
    }
}

fn root_dir() -> PathBuf {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf())).unwrap_or_else(|| PathBuf::from("."));
    if exe_dir.join("img").is_dir() { return exe_dir; }
    let cwd = std::env::current_dir().unwrap_or_else(|_| exe_dir.clone());
    if cwd.join("img").is_dir() { cwd } else { exe_dir }
}

fn main() {
    unsafe {
        windows_sys::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(windows_sys::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let name = win::wide("Local\\PopetoDesktopPets");
        CreateMutexW(null(), 0, name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // dragged a zip onto Popeto.exe while it runs: hand it over to the running one
            if let Some(p) = std::env::args().nth(1).filter(|a| !a.starts_with("--")) {
                if win::send_to_running(&p) { return; }
            }
            win::message(t("Popeto is already running - look for its icon in the tray."));
            return;
        }
    }
    win::register_classes();
    let root = root_dir();
    let mut app = App::new(root.clone());
    i18n::set(&app.settings.language);
    app.load_packs();
    app.flush_log();
    if app.packs.is_empty() {
        let _ = std::fs::create_dir_all(root.join("img"));
        win::message(&tf(
            "No pets found.\n\nPut Shimeji pets into:\n{}\n\nEach pet is a folder with shime*.png images (and usually a conf folder with actions.xml and behaviors.xml).\nDetails are in popeto.log.",
            &[&root.join("img").display()],
        ));
        return;
    }
    win::tray(app.tray, app.icon, win::TRAY_ADD);
    win::register_hotkey(app.tray, &app.settings.panic_key);
    app.configure_online();
    let pr = &app.settings.pranks;
    if pr.notes || pr.memes || pr.footprints || pr.cursor || pr.talk || pr.honk || pr.windows { pranks::ensure_files(&root); }
    app.world.monitors = win::monitors();
    app.world.cursor = win::cursor();
    if app.packs.len() > 1 && (app.settings.show_chooser || app.settings.fresh) { app.choose_pets(true); }
    app.spawn_start();
    if std::env::args().any(|a| a == "--settings") { settings_ui::open(&app.settings, app.icon); }
    if let Some(p) = std::env::args().nth(1).filter(|a| !a.starts_with("--")) { app.import_from(std::path::Path::new(&p)); }

    unsafe {
        // high resolution timer so 40 fps really is 40 fps (default Windows timers tick at ~15.6ms)
        let timer = CreateWaitableTimerExW(null(), null(), 2 /* HIGH_RESOLUTION */, 0x1F0003);
        let start = std::time::Instant::now();
        let mut next = 0.0f64;
        let mut msg: MSG = std::mem::zeroed();
        'main: loop {
            while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT { break 'main; }
                if settings_ui::pre_translate(&msg) { continue; }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let evs: Vec<Ev> = win::EVENTS.with(|q| std::mem::take(&mut *q.borrow_mut()));
            for e in evs { app.on_event(e); }
            if app.quit { break; }
            let now = start.elapsed().as_secs_f64() * 1000.0;
            if now >= next {
                app.tick();
                next += TICK_MS;
                if now - next > 200.0 { next = now + TICK_MS; }
            }
            let wait = (next - start.elapsed().as_secs_f64() * 1000.0).max(0.0);
            if !timer.is_null() {
                let due: i64 = -((wait * 10_000.0) as i64).max(1);
                SetWaitableTimer(timer, &due, 0, None, null(), 0);
                MsgWaitForMultipleObjects(1, &timer, 0, INFINITE, QS_ALLINPUT);
            } else {
                MsgWaitForMultipleObjects(0, null(), 0, wait as u32, QS_ALLINPUT);
            }
        }
    }
    win::tray(app.tray, app.icon, win::TRAY_DEL);
    app.unload();
    let _ = win::TRAY_MOD;
    let _ = win::MSG_LUP;
}
