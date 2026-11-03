//! Settings window: tabs for General, Pets, Life, Windows and Pranks. Modeless, so pets keep playing while it's open.
//! The window only edits controls; OK / Apply push an event and the app reads the values back.

use crate::i18n::{self, t, tf};
use crate::settings::Settings;
use crate::win::{self, wide, Ev};
use std::cell::RefCell;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{InitCommonControlsEx, ICC_LINK_CLASS, ICC_TAB_CLASSES, INITCOMMONCONTROLSEX};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub const SIZES: [(f64, &str); 10] = [
    (0.0, "Auto (Windows display scaling)"), (0.5, "50%"), (0.75, "75%"), (1.0, "100%"), (1.25, "125%"),
    (1.5, "150%"), (1.75, "175%"), (2.0, "200%"), (2.5, "250%"), (3.0, "300%"),
];
const OPACITY: [f64; 8] = [1.0, 0.9, 0.8, 0.7, 0.6, 0.5, 0.4, 0.3];
const LEVELS: [&str; 3] = ["Calm - rarely", "Normal", "Chaos - all the time"];

// buttons
const ID_OK: usize = 1;
const ID_CANCEL: usize = 2;
const ID_APPLY: usize = 3;
/// Buttons that run a tray command in the app (same ids as the tray menu).
pub const CMD_PETS_FOLDER: usize = 11;
pub const CMD_CHOOSE: usize = 17;
pub const CMD_ADD_PETS: usize = 19;

/// Where to find pets: (name, address, description). Shown on the Pets tab and in the tray menu.
pub const SITES: [(&str, &str, &str); 5] = [
    ("shimeji.org", "https://shimeji.org/", "Big community library, no account needed. Popeto converts its pets automatically."),
    ("Cachomon", "https://cachomon.com/grid.php?g=1", "Hundreds of Pokemon and other characters. Download via an ad page or Patreon."),
    ("Kilkakon", "https://kilkakon.com/shimeji", "Home of Shimeji-ee and its official pets."),
    ("Reddit r/shimeji", "https://www.reddit.com/r/shimeji/", "Community: new pets, requests and help."),
    ("DeviantArt", "https://www.deviantart.com/search?q=shimeji", "The most pets of all; downloading needs a free account."),
];
pub const CMD_PRANKS_FOLDER: usize = 64;
pub const CMD_BALL: usize = 61;
pub const CMD_COOKIE: usize = 65;
pub const CMD_BALLOON: usize = 66;
pub const CMD_BED: usize = 67;
pub const CMD_TOYS_CLEAR: usize = 68;
pub const CMD_LASER: usize = 69;
pub const CMD_CHALK: usize = 100;

// raw control constants (kept local; they are stable Win32 values)
const BS_AUTOCHECKBOX: u32 = 3;
const BS_DEFPUSHBUTTON: u32 = 1;
const BM_GETCHECK: u32 = 0xF0;
const BM_SETCHECK: u32 = 0xF1;
const CBS_DROPDOWNLIST: u32 = 3;
const CB_ADDSTRING: u32 = 0x143;
const CB_GETCURSEL: u32 = 0x147;
const CB_SETCURSEL: u32 = 0x14E;
const ES_AUTOHSCROLL: u32 = 0x80;
const ES_NUMBER: u32 = 0x2000;
const TCM_GETCURSEL: u32 = 0x130B;
const TCM_INSERTITEMW: u32 = 0x133E;
const TCN_SELCHANGE: u32 = (-551i32) as u32;

#[repr(C)]
struct TcItem { mask: u32, state: u32, state_mask: u32, text: *mut u16, max: i32, image: i32, lparam: isize }
#[repr(C)]
struct NmHdr { from: HWND, id: usize, code: u32 }
/// NMLINK (SysLink click): header + LITEM.
#[repr(C)]
struct NmLink { hdr: NmHdr, mask: u32, link: i32, state: u32, state_mask: u32, id: [u16; 48], url: [u16; 2084] }

#[derive(Clone, Copy, PartialEq)]
enum F {
    Size, Filter, Opacity, MaxPets, Lang, Chooser, Sounds, Cloning, Unique, Watch,
    LStack, LMeet, LGames, LHide, LPush, LSleep, LSleepMin, LDay, LEmotes, LTalk,
    Break, Water, Dance, Icons, Names,
    LanOn, LanAuto, LanMin, LanPeers, LanName,
    Interact, Throw, Keep, Shake, HideFs, FsShow, Ignore,
    Notes, Pictures, Online, Prints, Cursor, Talk, Honk, Wins, Level, Panic,
}

struct Ctl { h: HWND, f: Option<F> }

struct Ui {
    hwnd: HWND,
    pages: Vec<Vec<HWND>>,
    ctls: Vec<Ctl>,
    font: HFONT,
}

thread_local! { static UI: RefCell<Option<Ui>> = const { RefCell::new(None) }; }

const CLASS: &str = "PopetoSettings";

fn px(s: f64, v: i32) -> i32 { (v as f64 * s).round() as i32 }

unsafe extern "system" fn proc_(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = wp & 0xFFFF;
            match id {
                ID_OK => push(Ev::SettingsApply(true)),
                ID_APPLY => push(Ev::SettingsApply(false)),
                ID_CANCEL => { close(); }
                CMD_PETS_FOLDER | CMD_CHOOSE | CMD_ADD_PETS | CMD_PRANKS_FOLDER | CMD_BALL | CMD_COOKIE | CMD_BALLOON | CMD_BED | CMD_TOYS_CLEAR | CMD_LASER | CMD_CHALK => push(Ev::Command(id)),
                _ => {}
            }
            return 0;
        }
        WM_NOTIFY => {
            let n = &*(lp as *const NmHdr);
            if n.code == TCN_SELCHANGE { show_page(SendMessageW(n.from, TCM_GETCURSEL, 0, 0) as usize); }
            // NM_CLICK / NM_RETURN from a site link: open its address in the browser
            if n.code == (-2i32) as u32 || n.code == (-4i32) as u32 {
                let l = &*(lp as *const NmLink);
                let end = l.url.iter().position(|c| *c == 0).unwrap_or(0);
                if end > 0 { win::open_url(&String::from_utf16_lossy(&l.url[..end])); }
            }
            return 0;
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            SetBkColor(wp as HDC, GetSysColor(COLOR_WINDOW));
            return GetSysColorBrush(COLOR_WINDOW) as LRESULT;
        }
        WM_CLOSE => { close(); return 0; }
        WM_DROPFILES => { if let Some(p) = win::dropped_file(wp) { push(Ev::Import(p)); } return 0; }
        _ => {}
    }
    DefWindowProcW(h, msg, wp, lp)
}

fn push(e: Ev) { win::EVENTS.with(|q| q.borrow_mut().push(e)); }

fn show_page(n: usize) {
    let Some(pages) = UI.with(|u| u.borrow().as_ref().map(|u| u.pages.clone())) else { return };
    for (i, p) in pages.iter().enumerate() {
        for h in p { unsafe { ShowWindow(*h, if i == n { SW_SHOW } else { SW_HIDE }); } }
    }
}

/// Give Tab / Enter / Esc dialog behaviour to the settings window. Returns true if the message was handled.
pub fn pre_translate(msg: &MSG) -> bool {
    let Some(h) = UI.with(|u| u.borrow().as_ref().map(|u| u.hwnd)) else { return false };
    unsafe { IsDialogMessageW(h, msg) != 0 }
}

/// Close the window. Returns whether it was open.
pub fn close() -> bool {
    let Some(ui) = UI.with(|u| u.borrow_mut().take()) else { return false };
    unsafe {
        DestroyWindow(ui.hwnd);
        DeleteObject(ui.font);
    }
    true
}

fn register() {
    unsafe {
        let icc = INITCOMMONCONTROLSEX { dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_TAB_CLASSES | ICC_LINK_CLASS };
        InitCommonControlsEx(&icc);
        let cls = wide(CLASS);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(proc_),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: GetModuleHandleW(null()),
            hIcon: null_mut(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: (COLOR_WINDOW + 1) as usize as HBRUSH,
            lpszMenuName: null(),
            lpszClassName: cls.as_ptr(),
            hIconSm: null_mut(),
        };
        RegisterClassExW(&wc);
    }
}

/// Builds controls row by row.
struct Builder { parent: HWND, font: HFONT, s: f64, y: i32, x0: i32, w: i32, page: Vec<HWND>, ctls: Vec<Ctl> }

impl Builder {
    unsafe fn make(&mut self, class: &str, text: &str, style: u32, x: i32, y: i32, w: i32, h: i32, id: usize, f: Option<F>) -> HWND {
        let (c, t) = (wide(class), wide(text));
        let ex = if class == "EDIT" { WS_EX_CLIENTEDGE } else { 0 };
        let hw = CreateWindowExW(ex, c.as_ptr(), t.as_ptr(), WS_CHILD | style, x, y, w, h, self.parent, id as HMENU, GetModuleHandleW(null()), null());
        SendMessageW(hw, WM_SETFONT, self.font as WPARAM, 1);
        self.page.push(hw);
        if f.is_some() { self.ctls.push(Ctl { h: hw, f }); }
        hw
    }
    fn row(&self) -> i32 { px(self.s, 30) }
    unsafe fn check(&mut self, text: &str, f: F, on: bool) {
        let (x, y, w) = (self.x0, self.y, self.w);
        let h = self.make("BUTTON", text, WS_TABSTOP | BS_AUTOCHECKBOX, x, y, w, px(self.s, 24), 0, Some(f));
        SendMessageW(h, BM_SETCHECK, on as usize, 0);
        self.y += self.row();
    }
    unsafe fn label(&mut self, text: &str) -> i32 {
        let (x, y) = (self.x0, self.y + px(self.s, 4));
        self.make("STATIC", text, 0, x, y, px(self.s, 190), px(self.s, 20), 0, None);
        self.x0 + px(self.s, 195)
    }
    unsafe fn combo(&mut self, text: &str, f: F, items: &[String], sel: usize) {
        let x = self.label(text);
        let cw = self.w - (x - self.x0);
        let h = self.make("COMBOBOX", "", WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST, x, self.y, cw, px(self.s, 300), 0, Some(f));
        for it in items { let t = wide(it); SendMessageW(h, CB_ADDSTRING, 0, t.as_ptr() as LPARAM); }
        SendMessageW(h, CB_SETCURSEL, sel, 0);
        self.y += self.row() + px(self.s, 2);
    }
    unsafe fn edit(&mut self, text: &str, f: F, value: &str, number: bool) {
        let x = self.label(text);
        let cw = self.w - (x - self.x0);
        self.make("EDIT", value, WS_TABSTOP | ES_AUTOHSCROLL | if number { ES_NUMBER } else { 0 }, x, self.y, cw, px(self.s, 24), 0, Some(f));
        self.y += self.row() + px(self.s, 2);
    }
    unsafe fn note(&mut self, text: &str, lines: i32) {
        let (x, y, w) = (self.x0, self.y, self.w);
        let h = self.make("STATIC", text, 0, x, y, w, px(self.s, 18) * lines, 0, None);
        let _ = h;
        self.y += px(self.s, 18) * lines + px(self.s, 6);
    }
    unsafe fn button(&mut self, text: &str, id: usize, x: i32, w: i32) {
        let y = self.y;
        self.make("BUTTON", text, WS_TABSTOP, x, y, w, px(self.s, 28), id, None);
    }
    /// SysLink control: `<a href="...">name</a>` markup, opens in the browser when clicked.
    unsafe fn link(&mut self, markup: &str, lines: i32) {
        let (x, y, w) = (self.x0, self.y, self.w);
        self.make("SysLink", markup, WS_TABSTOP, x, y, w, px(self.s, 18) * lines + px(self.s, 2), 0, None);
        self.y += px(self.s, 18) * lines + px(self.s, 8);
    }
    fn gap(&mut self, v: i32) { self.y += px(self.s, v); }
    fn finish_page(&mut self, top: i32) -> Vec<HWND> { self.y = top; std::mem::take(&mut self.page) }
}

fn nearest(list: &[f64], v: f64) -> usize {
    (0..list.len()).min_by(|&a, &b| (list[a] - v).abs().partial_cmp(&(list[b] - v).abs()).unwrap()).unwrap_or(0)
}

pub fn open(s: &Settings, icon: HICON) {
    if let Some(h) = UI.with(|u| u.borrow().as_ref().map(|u| u.hwnd)) {
        unsafe { ShowWindow(h, SW_RESTORE); SetForegroundWindow(h); }
        return;
    }
    register();
    unsafe {
        let sc = windows_sys::Win32::UI::HiDpi::GetDpiForSystem() as f64 / 96.0;
        let (cw, ch) = (px(sc, 560), px(sc, 560));
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let mut r = RECT { left: 0, top: 0, right: cw, bottom: ch };
        AdjustWindowRectEx(&mut r, style, 0, 0);
        let (ww, wh) = (r.right - r.left, r.bottom - r.top);
        let mut work: RECT = std::mem::zeroed();
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut _ as *mut _, 0);
        let (x, y) = (work.left + (work.right - work.left - ww) / 2, work.top + (work.bottom - work.top - wh) / 2);
        let (cls, title) = (wide(CLASS), wide(t("Popeto settings")));
        let hwnd = CreateWindowExW(WS_EX_APPWINDOW | WS_EX_ACCEPTFILES, cls.as_ptr(), title.as_ptr(), style, x, y, ww, wh, null_mut(), null_mut(), GetModuleHandleW(null()), null());
        if !icon.is_null() {
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as WPARAM, icon as LPARAM);
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as WPARAM, icon as LPARAM);
        }
        let face = wide("Segoe UI");
        let font = CreateFontW(-px(sc, 14), 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET as _, 0, 0, CLEARTYPE_QUALITY as _, 0, face.as_ptr());

        let m = px(sc, 12);
        // the tab control is only a header strip; pages sit on the window's own background
        let tab_h = px(sc, 30);
        let tab_cls = wide("SysTabControl32");
        let tab = CreateWindowExW(0, tab_cls.as_ptr(), null(), WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_TABSTOP, m, m, cw - 2 * m, tab_h, hwnd, null_mut(), GetModuleHandleW(null()), null());
        SendMessageW(tab, WM_SETFONT, font as WPARAM, 1);
        for (i, name) in ["General", "Pets", "Life", "Extras", "Network", "Windows", "Pranks"].iter().enumerate() {
            let mut tx = wide(t(name));
            let item = TcItem { mask: 1, state: 0, state_mask: 0, text: tx.as_mut_ptr(), max: 0, image: 0, lparam: 0 };
            SendMessageW(tab, TCM_INSERTITEMW, i, &item as *const _ as LPARAM);
        }

        let top = m + px(sc, 44);
        let mut b = Builder { parent: hwnd, font, s: sc, y: top, x0: m + px(sc, 20), w: cw - 2 * m - px(sc, 40), page: vec![], ctls: vec![] };
        let pct = |v: f64| format!("{}%", (v * 100.0).round() as i32);

        // --- General
        let size_vals: Vec<f64> = SIZES.iter().map(|v| v.0).collect();
        b.combo(t("Pet size"), F::Size, &SIZES.iter().map(|v| t(v.1).to_string()).collect::<Vec<_>>(), if s.scaling <= 0.0 { 0 } else { nearest(&size_vals, s.scaling) });
        b.combo(t("Image smoothing"), F::Filter, &[t("Smooth").into(), t("Sharp pixels (pixel art)").into()], s.nearest as usize);
        b.combo(t("Opacity"), F::Opacity, &OPACITY.iter().map(|v| pct(*v)).collect::<Vec<_>>(), nearest(&OPACITY, s.opacity));
        b.edit(t("Maximum number of pets"), F::MaxPets, &s.max_pets.to_string(), true);
        let langs: Vec<String> = std::iter::once(t("Auto (Windows language)").to_string()).chain(i18n::LANGUAGES.iter().map(|l| l.1.to_string())).collect();
        let lang_sel = i18n::LANGUAGES.iter().position(|l| s.language.eq_ignore_ascii_case(l.0)).map(|i| i + 1).unwrap_or(0);
        b.combo(t("Interface language"), F::Lang, &langs, lang_sel);
        b.gap(6);
        b.check(t("Show the pet chooser every time Popeto starts"), F::Chooser, s.show_chooser);
        b.check(t("Play sounds that come with pets"), F::Sounds, s.sounds);
        b.check(t("Allow pets to clone themselves"), F::Cloning, s.cloning);
        b.check(t("Only one of each pet (no duplicates)"), F::Unique, s.unique_pets);
        b.gap(10);
        b.note(t("Changing size or smoothing reloads the pets (they stay where they are)."), 2);
        let general = b.finish_page(top);

        // --- Pets
        let bw = (b.w - px(sc, 10)) / 2;
        b.button(t("Choose pets..."), CMD_CHOOSE, b.x0, bw);
        b.button(t("Open pets folder"), CMD_PETS_FOLDER, b.x0 + bw + px(sc, 10), bw);
        b.gap(36);
        b.button(t("Add pets from Shimeji zip..."), CMD_ADD_PETS, b.x0, b.w);
        b.gap(36);
        b.check(t("Offer to add pets downloaded to the Downloads folder"), F::Watch, s.watch_downloads);
        b.gap(8);
        b.note(t("Where to find pets (click a link, download in the browser):"), 1);
        for (name, url, desc) in SITES {
            b.link(&format!("<a href=\"{url}\">{name}</a> - {}", t(desc)), 2);
        }
        let pets = b.finish_page(top);

        // --- Life
        let l = &s.life;
        b.note(t("What pets do on their own, with each other, your windows and you."), 1);
        b.gap(2);
        b.check(t("Pets can stand on each other's heads"), F::LStack, l.stacking);
        b.check(t("Pets greet each other when they meet"), F::LMeet, l.meet);
        b.check(t("Pets play tag, follow the leader and fetch the ball"), F::LGames, l.games);
        b.check(t("Pets sometimes hide behind the window they climb"), F::LHide, l.hide);
        b.check(t("Pets push windows (moves your windows a little)"), F::LPush, l.push);
        b.check(t("Pets fall asleep when you're away"), F::LSleep, l.sleep);
        b.edit(t("    ...after this many minutes"), F::LSleepMin, &l.sleep_minutes.to_string(), true);
        b.check(t("Greetings in the morning, lunch reminder, sleepy at night"), F::LDay, l.daytime);
        b.check(t("Little hearts, stars and zzz above pets"), F::LEmotes, l.emotes);
        b.check(t("Pets talk about what they do (speech bubbles)"), F::LTalk, l.talk);
        b.gap(6);
        let tw = (b.w - 3 * px(sc, 8)) / 4;
        b.button(t("Drop a cookie"), CMD_COOKIE, b.x0, tw);
        b.button(t("Throw a ball"), CMD_BALL, b.x0 + tw + px(sc, 8), tw);
        b.button(t("Release a balloon"), CMD_BALLOON, b.x0 + 2 * (tw + px(sc, 8)), tw);
        b.button(t("Bed"), CMD_BED, b.x0 + 3 * (tw + px(sc, 8)), tw);
        let life = b.finish_page(top);

        // --- Extras
        b.note(t("Reminders: a pet brings you a note. Minutes, 0 = off."), 1);
        b.gap(2);
        b.edit(t("Take a break after (minutes at the computer)"), F::Break, &l.break_minutes.to_string(), true);
        b.note(t("The break timer starts over when you are away for 5 minutes or more."), 1);
        b.edit(t("Drink water every (minutes)"), F::Water, &l.water_minutes.to_string(), true);
        b.note(t("Kitchen timer: tray menu > Timer. From scripts: Popeto.exe --say \"Build done\" or --note \"text\"."), 2);
        b.gap(6);
        b.check(t("Pets dance when music plays"), F::Dance, l.dance);
        b.check(t("Pets can stand on desktop icons"), F::Icons, l.icons);
        b.check(t("Show a pet's name when the mouse is over it (right click a pet > Name...)"), F::Names, l.show_names);
        b.gap(6);
        let hw = (b.w - px(sc, 10)) / 2;
        b.button(t("Laser pointer"), CMD_LASER, b.x0, hw);
        b.button(t("Chalk"), CMD_CHALK, b.x0 + hw + px(sc, 10), hw);
        let extras = b.finish_page(top);

        // --- Network
        let n = &s.lan;
        b.note(t("Pets walk off your screen and visit other PCs running Popeto on the same network, then come back."), 2);
        b.gap(2);
        b.check(t("Visit other PCs on the local network"), F::LanOn, n.on);
        b.check(t("Pets go visiting by themselves now and then"), F::LanAuto, n.auto);
        b.edit(t("A visit lasts (minutes)"), F::LanMin, &n.minutes.to_string(), true);
        b.edit(t("This PC's name for others"), F::LanName, &n.name, false);
        b.note(t("Empty = the computer name."), 1);
        b.edit(t("Other PCs (addresses)"), F::LanPeers, &n.peers.join(", "), false);
        b.note(t("Only needed if they aren't found automatically, e.g. 192.168.1.20, office-pc. Comma separated."), 2);
        b.gap(4);
        b.note(t("Visiting pets that you don't have are sent over the network for the visit (kept in the visitors folder). Uses port 47811; Windows may ask to allow Popeto in the firewall. Right click a pet > Go visiting."), 4);
        let network = b.finish_page(top);

        // --- Windows
        b.check(t("Pets play with windows (stand, climb, hang on them)"), F::Interact, s.interact);
        b.check(t("Pets may carry and throw windows"), F::Throw, s.throw_windows);
        b.check(t("Keep thrown windows on screen"), F::Keep, s.keep_on_screen);
        b.check(t("Shake a window when a pet lands on it hard"), F::Shake, s.shake);
        b.check(t("Hide pets while a fullscreen app is in front"), F::HideFs, s.hide_fullscreen);
        b.gap(8);
        b.edit(t("Still show pets over"), F::FsShow, &s.fs_show.join(", "), false);
        b.note(t("Fullscreen apps that keep pets visible (exe names). mstsc = Remote Desktop."), 1);
        b.gap(6);
        b.edit(t("Ignore these windows"), F::Ignore, &s.blacklist.join(", "), false);
        b.note(t("Exe names (without .exe) or parts of window titles, comma separated. Pets won't touch them."), 2);
        let windows = b.finish_page(top);

        // --- Pranks
        let p = &s.pranks;
        b.note(t("Goose-style mischief. Everything here is off until you tick it."), 1);
        b.gap(2);
        b.check(t("Bring notes"), F::Notes, p.notes);
        b.check(t("Bring pictures"), F::Pictures, p.memes);
        b.check(t("    ...also fresh ones from the internet (not saved)"), F::Online, p.online);
        b.check(t("Leave muddy footprints"), F::Prints, p.footprints);
        b.check(t("Grab the mouse cursor (shake the mouse to escape)"), F::Cursor, p.cursor);
        b.check(t("Speech bubbles"), F::Talk, p.talk);
        b.check(t("Honk"), F::Honk, p.honk);
        b.check(t("Stomp on windows and knock them down"), F::Wins, p.windows);
        b.gap(4);
        b.combo(t("How often"), F::Level, &LEVELS.iter().map(|v| t(v).to_string()).collect::<Vec<_>>(), p.level.min(2) as usize);
        b.edit(t("Panic key"), F::Panic, &s.panic_key, false);
        b.note(t("Hides every pet and stops pranks; press again to bring them back. E.g. Ctrl+Shift+F12, Ctrl+Alt+P."), 2);
        b.note(&tf("Texts, pictures, sounds: pranks\\universal (any language) + pranks\\{} (this language).", &[&i18n::code()]), 2);
        b.button(t("Open pranks folder"), CMD_PRANKS_FOLDER, b.x0, bw);
        b.button(t("Throw a ball"), CMD_BALL, b.x0 + bw + px(sc, 10), bw);
        let pranks = b.finish_page(top);

        // bottom buttons
        let (bw2, bh) = (px(sc, 92), px(sc, 28));
        let by = ch - m - bh;
        let mk = |text: &str, id: usize, x: i32, def: bool| {
            let t = wide(text);
            let c = wide("BUTTON");
            let hw = CreateWindowExW(0, c.as_ptr(), t.as_ptr(), WS_CHILD | WS_VISIBLE | WS_TABSTOP | if def { BS_DEFPUSHBUTTON } else { 0 }, x, by, bw2, bh, hwnd, id as HMENU, GetModuleHandleW(null()), null());
            SendMessageW(hw, WM_SETFONT, font as WPARAM, 1);
        };
        mk("OK", ID_OK, cw - m - 3 * bw2 - 2 * px(sc, 8), true);
        mk(t("Cancel"), ID_CANCEL, cw - m - 2 * bw2 - px(sc, 8), false);
        mk(t("Apply"), ID_APPLY, cw - m - bw2, false);

        let ctls = std::mem::take(&mut b.ctls);
        UI.with(|u| *u.borrow_mut() = Some(Ui { hwnd, pages: vec![general, pets, life, extras, network, windows, pranks], ctls, font }));
        show_page(0);
        // tab control must stay below its page controls
        SetWindowPos(tab, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
    }
}

fn text_of(h: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(h, buf.as_mut_ptr(), 512) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Current values of the window, applied over `base`. None if the window is closed.
pub fn read(base: &Settings) -> Option<Settings> {
    let ctls: Vec<(HWND, F)> = UI.with(|u| u.borrow().as_ref().map(|u| u.ctls.iter().filter_map(|c| c.f.map(|f| (c.h, f))).collect()))?;
    let mut s = base.clone();
    let list = |v: &str| v.split(',').map(|x| x.trim().trim_end_matches(".exe").to_lowercase()).filter(|x| !x.is_empty()).collect::<Vec<_>>();
    for (h, f) in ctls {
        let on = unsafe { SendMessageW(h, BM_GETCHECK, 0, 0) } == 1;
        let sel = unsafe { SendMessageW(h, CB_GETCURSEL, 0, 0) }.max(0) as usize;
        match f {
            F::Size => s.scaling = SIZES[sel.min(SIZES.len() - 1)].0,
            F::Filter => s.nearest = sel == 1,
            F::Lang => s.language = if sel == 0 { "auto".into() } else { i18n::LANGUAGES[(sel - 1).min(i18n::LANGUAGES.len() - 1)].0.into() },
            F::Opacity => s.opacity = OPACITY[sel.min(OPACITY.len() - 1)],
            F::MaxPets => s.max_pets = text_of(h).trim().parse::<usize>().unwrap_or(s.max_pets).clamp(1, 200),
            F::Chooser => s.show_chooser = on,
            F::Sounds => s.sounds = on,
            F::Cloning => s.cloning = on,
            F::Unique => s.unique_pets = on,
            F::Watch => s.watch_downloads = on,
            F::LStack => s.life.stacking = on,
            F::LMeet => s.life.meet = on,
            F::LGames => s.life.games = on,
            F::LHide => s.life.hide = on,
            F::LPush => s.life.push = on,
            F::LSleep => s.life.sleep = on,
            F::LSleepMin => s.life.sleep_minutes = text_of(h).trim().parse::<u32>().unwrap_or(s.life.sleep_minutes).clamp(1, 240),
            F::LDay => s.life.daytime = on,
            F::LEmotes => s.life.emotes = on,
            F::LTalk => s.life.talk = on,
            F::Break => s.life.break_minutes = text_of(h).trim().parse::<u32>().unwrap_or(s.life.break_minutes).min(600),
            F::Water => s.life.water_minutes = text_of(h).trim().parse::<u32>().unwrap_or(s.life.water_minutes).min(600),
            F::Dance => s.life.dance = on,
            F::Icons => s.life.icons = on,
            F::Names => s.life.show_names = on,
            F::LanOn => s.lan.on = on,
            F::LanAuto => s.lan.auto = on,
            F::LanMin => s.lan.minutes = text_of(h).trim().parse::<u32>().unwrap_or(s.lan.minutes).clamp(1, 240),
            F::LanName => s.lan.name = text_of(h).trim().replace('|', " "),
            F::LanPeers => s.lan.peers = text_of(h).split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
            F::Interact => s.interact = on,
            F::Throw => s.throw_windows = on,
            F::Keep => s.keep_on_screen = on,
            F::Shake => s.shake = on,
            F::HideFs => s.hide_fullscreen = on,
            F::FsShow => s.fs_show = list(&text_of(h)),
            F::Ignore => s.blacklist = list(&text_of(h)),
            F::Notes => s.pranks.notes = on,
            F::Pictures => s.pranks.memes = on,
            F::Online => s.pranks.online = on,
            F::Prints => s.pranks.footprints = on,
            F::Cursor => s.pranks.cursor = on,
            F::Talk => s.pranks.talk = on,
            F::Honk => s.pranks.honk = on,
            F::Wins => s.pranks.windows = on,
            F::Level => s.pranks.level = sel.min(2) as u8,
            F::Panic => { let t = text_of(h); if !t.trim().is_empty() { s.panic_key = t.trim().to_string(); } }
        }
    }
    Some(s)
}
