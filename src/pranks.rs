//! Goose-style mischief on top of the Shimeji engine: pets bring notes and pictures, leave muddy
//! footprints, grab the cursor, talk, honk, bother windows, and play with a ball.
//! Every prank is off until switched on; no keyboard/mouse hooks, nothing injected anywhere.

use crate::env::{BorderRef, Side};
use crate::expr::random;
use crate::win;
use crate::App;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow;

// ---------------------------------------------------------------- drawing

/// 32bpp top-down DIB we can draw into with GDI and then fix up per pixel.
struct Canvas { dc: HDC, bmp: HBITMAP, old: HGDIOBJ, bits: *mut u8, w: i32, h: i32 }

impl Canvas {
    fn new(w: i32, h: i32) -> Canvas {
        unsafe {
            let mut bi: BITMAPINFO = std::mem::zeroed();
            bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bi.bmiHeader.biWidth = w;
            bi.bmiHeader.biHeight = -h;
            bi.bmiHeader.biPlanes = 1;
            bi.bmiHeader.biBitCount = 32;
            bi.bmiHeader.biCompression = BI_RGB;
            let mut bits = null_mut();
            let bmp = CreateDIBSection(null_mut(), &bi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            let dc = CreateCompatibleDC(null_mut());
            let old = SelectObject(dc, bmp);
            Canvas { dc, bmp, old, bits: bits as *mut u8, w, h }
        }
    }
    fn px(&mut self) -> &mut [u8] { unsafe { std::slice::from_raw_parts_mut(self.bits, (self.w * self.h * 4) as usize) } }
    fn fill(&mut self, c: (u8, u8, u8)) {
        unsafe { GdiFlush(); }
        for p in self.px().chunks_mut(4) { p[0] = c.2; p[1] = c.1; p[2] = c.0; p[3] = 255; }
    }
    fn text(&mut self, s: &str, r: RECT, size: i32, bold: bool, color: (u8, u8, u8), flags: u32) -> RECT {
        unsafe {
            let face = win::wide("Segoe UI");
            let f = CreateFontW(-size, 0, 0, 0, if bold { 700 } else { 400 }, 0, 0, 0, DEFAULT_CHARSET as _, 0, 0, ANTIALIASED_QUALITY as _, 0, face.as_ptr());
            let of = SelectObject(self.dc, f);
            SetBkMode(self.dc, TRANSPARENT as _);
            SetTextColor(self.dc, color.0 as u32 | (color.1 as u32) << 8 | (color.2 as u32) << 16);
            let mut t = win::wide(s);
            let mut rr = r;
            DrawTextW(self.dc, t.as_mut_ptr(), (t.len() - 1) as i32, &mut rr, flags | DT_NOPREFIX);
            SelectObject(self.dc, of);
            DeleteObject(f);
            GdiFlush();
            rr
        }
    }
    /// Measure wrapped text.
    fn measure(&mut self, s: &str, width: i32, size: i32, bold: bool) -> (i32, i32) {
        let r = self.text(s, RECT { left: 0, top: 0, right: width, bottom: 10000 }, size, bold, (0, 0, 0), DT_WORDBREAK | DT_CALCRECT | DT_CENTER);
        (r.right - r.left, r.bottom - r.top)
    }
    /// Apply per-pixel alpha (0..=1 coverage) and premultiply. Returns the finished bitmap.
    fn finish(mut self, alpha: impl Fn(f32, f32) -> f32) -> HBITMAP {
        let (w, h) = (self.w, self.h);
        let px = self.px();
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let a = alpha(x as f32 + 0.5, y as f32 + 0.5).clamp(0.0, 1.0);
                for k in 0..3 { px[i + k] = (px[i + k] as f32 * a) as u8; }
                px[i + 3] = (a * 255.0) as u8;
            }
        }
        unsafe { SelectObject(self.dc, self.old); DeleteDC(self.dc); }
        self.bmp
    }
}

/// Signed distance to a rounded rectangle (negative inside).
fn rrect(x: f32, y: f32, l: f32, t: f32, r: f32, b: f32, rad: f32) -> f32 {
    let (cx, cy) = ((l + r) / 2.0, (t + b) / 2.0);
    let (hx, hy) = ((r - l) / 2.0 - rad, (b - t) / 2.0 - rad);
    let (qx, qy) = ((x - cx).abs() - hx, (y - cy).abs() - hy);
    let out = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    out + qx.max(qy).min(0.0) - rad
}

fn cover(d: f32) -> f32 { (0.5 - d).clamp(0.0, 1.0) }

fn paint_shadow_card(c: &mut Canvas, card: (f32, f32, f32, f32), rad: f32, fill: (u8, u8, u8)) {
    let (w, h) = (c.w, c.h);
    let px = c.px();
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            if rrect(fx, fy, card.0, card.1, card.2, card.3, rad) <= 0.5 {
                let i = ((y * w + x) * 4) as usize;
                px[i] = fill.2; px[i + 1] = fill.1; px[i + 2] = fill.0;
            }
        }
    }
}

fn card_alpha(card: (f32, f32, f32, f32), rad: f32, shadow: f32) -> impl Fn(f32, f32) -> f32 {
    move |x, y| {
        let d = rrect(x, y, card.0, card.1, card.2, card.3, rad);
        if d <= 0.0 { return cover(d); }
        // soft shadow, offset down a bit
        let ds = rrect(x, y - shadow * 0.4, card.0, card.1, card.2, card.3, rad).max(0.0);
        let sh = (1.0 - ds / shadow).clamp(0.0, 1.0).powi(2) * 0.35;
        cover(d).max(sh)
    }
}

pub struct Gfx { pub bmp: HBITMAP, pub w: i32, pub h: i32 }

fn note_gfx(text: &str, s: f64) -> Gfx {
    let sc = |v: f64| (v * s).round() as i32;
    let mut probe = Canvas::new(8, 8);
    let (_, th) = probe.measure(text, sc(220.0), sc(17.0), false);
    probe.finish(|_, _| 0.0);
    let m = sc(10.0) as f32;
    let (w, h) = (sc(260.0), th + sc(64.0));
    let mut c = Canvas::new(w, h);
    c.fill((0, 0, 0));
    let card = (m, m, w as f32 - m, h as f32 - m);
    paint_shadow_card(&mut c, card, 4.0 * s as f32, (255, 244, 170));
    // tape strip
    let tape = (w as f32 / 2.0 - 30.0 * s as f32, m - 6.0 * s as f32, w as f32 / 2.0 + 30.0 * s as f32, m + 12.0 * s as f32);
    paint_shadow_card(&mut c, tape, 2.0, (225, 225, 210));
    c.text("\u{00d7}", RECT { left: w - sc(38.0), top: m as i32 + sc(2.0), right: w - m as i32 - sc(4.0), bottom: m as i32 + sc(26.0) }, sc(20.0), false, (150, 130, 60), DT_CENTER | DT_SINGLELINE);
    c.text(text, RECT { left: sc(20.0), top: sc(36.0), right: w - sc(20.0), bottom: h - sc(14.0) }, sc(17.0), false, (60, 50, 30), DT_WORDBREAK | DT_CENTER);
    let a = card_alpha(card, 4.0 * s as f32, 9.0 * s as f32);
    let ta = move |x: f32, y: f32| a(x, y).max(cover(rrect(x, y, tape.0, tape.1, tape.2, tape.3, 2.0)) * 0.85);
    Gfx { bmp: c.finish(ta), w, h }
}

fn meme_gfx(img: &Path, s: f64) -> Option<Gfx> {
    let (iw, ih, rgba) = win::load_rgba(img, 1.0, false)?;
    meme_gfx_rgba(iw, ih, &rgba, s)
}

fn meme_gfx_rgba(iw: u32, ih: u32, rgba: &[u8], s: f64) -> Option<Gfx> {
    let max = 380.0 * s;
    let k = (max / iw.max(ih) as f64).min(1.5 * s);
    let (nw, nh) = (((iw as f64 * k) as i32).max(1), ((ih as f64 * k) as i32).max(1));
    let small = win::resize(rgba, iw, ih, nw as u32, nh as u32, false);
    let sc = |v: f64| (v * s).round() as i32;
    let (m, pad, bottom) = (sc(10.0), sc(10.0), sc(34.0));
    let (w, h) = (nw + 2 * (m + pad), nh + m * 2 + pad + bottom);
    let mut c = Canvas::new(w, h);
    c.fill((0, 0, 0));
    let card = (m as f32, m as f32, (w - m) as f32, (h - m) as f32);
    paint_shadow_card(&mut c, card, 3.0, (252, 252, 250));
    {
        let px = c.px();
        for y in 0..nh {
            for x in 0..nw {
                let si = ((y * nw + x) * 4) as usize;
                if si + 3 >= small.len() { continue; }
                let a = small[si + 3] as u32;
                let di = (((y + m + pad) * w + x + m + pad) * 4) as usize;
                for (k, ch) in [2usize, 1, 0].iter().enumerate() {
                    px[di + k] = ((small[si + ch] as u32 * a + px[di + k] as u32 * (255 - a)) / 255) as u8;
                }
            }
        }
    }
    c.text("\u{00d7}", RECT { left: w - m - sc(30.0), top: h - m - sc(30.0), right: w - m - sc(4.0), bottom: h - m - sc(4.0) }, sc(20.0), false, (150, 150, 150), DT_CENTER | DT_SINGLELINE);
    Some(Gfx { bmp: c.finish(card_alpha(card, 3.0, 9.0 * s as f32)), w, h })
}

fn bubble_gfx(text: &str, s: f64) -> Gfx {
    let sc = |v: f64| (v * s).round() as i32;
    let mut probe = Canvas::new(8, 8);
    let (tw, th) = probe.measure(text, sc(200.0), sc(15.0), true);
    probe.finish(|_, _| 0.0);
    let (pad, tail) = (sc(10.0), sc(12.0));
    let (w, h) = ((tw + pad * 2 + 4).max(sc(44.0)), th + pad * 2 + tail + 2);
    let mut c = Canvas::new(w, h);
    c.fill((255, 255, 255));
    c.text(text, RECT { left: pad, top: pad, right: w - pad, bottom: h - tail - pad }, sc(15.0), true, (40, 40, 40), DT_WORDBREAK | DT_CENTER);
    let body = (1.0, 1.0, w as f32 - 1.0, (h - tail) as f32 - 1.0);
    let rad = 10.0 * s as f32;
    let (tx, tb, tt) = (w as f32 / 2.0, h as f32 - 1.0, body.3 - 2.0);
    let tl = tail as f32;
    let shape = move |x: f32, y: f32| {
        let d = rrect(x, y, body.0, body.1, body.2, body.3, rad);
        // tail: triangle pointing down at the pet
        let in_tail = y >= tt && y <= tb && (x - tx).abs() <= (tb - y) / tl * tl * 0.6;
        if in_tail { -1.0 } else { d }
    };
    // dark outline
    {
        let (w, h) = (c.w, c.h);
        let px = c.px();
        for y in 0..h {
            for x in 0..w {
                let d = shape(x as f32 + 0.5, y as f32 + 0.5);
                if d > -1.6 && d <= 0.5 && !(y as f32 > body.3 - 1.0) {
                    let i = ((y * w + x) * 4) as usize;
                    px[i] = 70; px[i + 1] = 70; px[i + 2] = 70;
                }
            }
        }
    }
    Gfx { bmp: c.finish(move |x, y| cover(shape(x, y))), w, h }
}

fn print_gfx(s: f64) -> Gfx {
    let w = (36.0 * s) as i32;
    let h = (15.0 * s) as i32;
    let mut c = Canvas::new(w.max(4), h.max(4));
    c.fill((92, 64, 38));
    let (fw, fh) = (w as f32, h as f32);
    let blob = move |x: f32, y: f32, cx: f32, cy: f32, rx: f32, ry: f32| 1.0 - (((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2)).sqrt();
    Gfx {
        bmp: c.finish(move |x, y| {
            let a = blob(x, y, fw * 0.27, fh * 0.55, fw * 0.2, fh * 0.42).max(blob(x, y, fw * 0.73, fh * 0.55, fw * 0.2, fh * 0.42));
            (a * 3.0).clamp(0.0, 1.0) * 0.9
        }),
        w,
        h,
    }
}

fn ball_gfx(r: i32) -> Gfx {
    let d = r * 2 + 2;
    let mut c = Canvas::new(d, d);
    {
        let px = c.px();
        for y in 0..d {
            for x in 0..d {
                let (fx, fy) = (x as f32 + 0.5 - d as f32 / 2.0, y as f32 + 0.5 - d as f32 / 2.0);
                let ang = fy.atan2(fx);
                let seg = (((ang + std::f32::consts::PI) / (std::f32::consts::PI / 3.0)) as i32) % 6;
                let col = [(230, 57, 70), (255, 255, 255), (69, 123, 230), (255, 255, 255), (255, 200, 50), (255, 255, 255)][seg as usize];
                let shade = 1.0 - ((fx + r as f32 * 0.4).powi(2) + (fy + r as f32 * 0.4).powi(2)).sqrt() / (r as f32 * 2.4);
                let i = ((y * d + x) * 4) as usize;
                px[i] = (col.2 as f32 * shade) as u8; px[i + 1] = (col.1 as f32 * shade) as u8; px[i + 2] = (col.0 as f32 * shade) as u8;
            }
        }
    }
    let rf = r as f32;
    Gfx { bmp: c.finish(move |x, y| cover(((x - rf - 1.0).powi(2) + (y - rf - 1.0).powi(2)).sqrt() - rf)), w: d, h: d }
}

// ---------------------------------------------------------------- prank files

const README: &str = "Popeto prank content\r\n\
\r\n\
universal    used whatever the interface language is: pictures without text, sounds,\r\n\
             notes and speech bubbles made of symbols.\r\n\
en, ru       used only when Popeto runs in that language: notes, speech bubbles and\r\n\
             pictures with text in that language.\r\n\
\r\n\
Each of these folders can have:\r\n\
  notes.txt   one note per line (pets drag them onto the screen)\r\n\
  talk.txt    speech bubbles; lines under [random], [petted], [thrown], [grab], [free], [note], [window]\r\n\
  pictures    folder with .png pictures pets bring in\r\n\
  sounds      folder with .wav sounds for honking\r\n\
Pets pick from universal + the current language together.\r\n";

// Default texts and internet sources ship inside the exe, taken from assets/pranks in the repo.
const UNIVERSAL_NOTES: &str = include_str!("../assets/pranks/universal/notes.txt");
const UNIVERSAL_TALK: &str = include_str!("../assets/pranks/universal/talk.txt");
const UNIVERSAL_ONLINE: &str = include_str!("../assets/pranks/universal/online.txt");

/// (notes, talk, online) defaults for a language folder.
fn default_texts(lang: &str) -> (&'static str, &'static str, &'static str) {
    match lang {
        "ru" => (include_str!("../assets/pranks/ru/notes.txt"), include_str!("../assets/pranks/ru/talk.txt"), include_str!("../assets/pranks/ru/online.txt")),
        _ => (include_str!("../assets/pranks/en/notes.txt"), include_str!("../assets/pranks/en/talk.txt"), include_str!("../assets/pranks/en/online.txt")),
    }
}

/// A short synthesized honk (two nasal hoots), so honking works without any downloaded files.
fn honk_wav() -> Vec<u8> {
    let rate = 22050u32;
    let mut samples: Vec<i16> = Vec::new();
    for (start, len) in [(0.0f32, 0.16f32), (0.22, 0.2)] {
        while (samples.len() as f32) < start * rate as f32 { samples.push(0); }
        let n = (len * rate as f32) as usize;
        let mut phase = 0f32;
        for i in 0..n {
            let t = i as f32 / rate as f32;
            let f = 420.0 - 140.0 * (t / len);
            phase += f / rate as f32;
            let saw = 2.0 * (phase - phase.floor()) - 1.0;
            let sq = if (phase * 2.0).fract() < 0.5 { 1.0 } else { -1.0 };
            let env = (t / 0.015).min(1.0) * ((len - t) / 0.04).min(1.0);
            let nasal = 0.75 + 0.25 * (t * 2.0 * std::f32::consts::PI * 33.0).sin();
            samples.push(((saw * 0.6 + sq * 0.4) * env * nasal * 0.45 * 32767.0) as i16);
        }
    }
    let data = samples.len() as u32 * 2;
    let mut v = Vec::with_capacity(44 + data as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&rate.to_le_bytes());
    v.extend_from_slice(&(rate * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data.to_le_bytes());
    for s in samples { v.extend_from_slice(&s.to_le_bytes()); }
    v
}

/// Create the prank folders (universal + one per language) and move files from the old flat layout.
pub fn ensure_files(root: &Path) -> PathBuf {
    let dir = root.join("pranks");
    let uni = dir.join("universal");
    for sub in ["pictures", "sounds"] { let _ = std::fs::create_dir_all(uni.join(sub)); }
    for (code, _) in crate::i18n::LANGUAGES { let _ = std::fs::create_dir_all(dir.join(code).join("pictures")); }
    // old layout: pranks\notes.txt, talk.txt (English), pictures\, sounds\ -> en\ and universal\
    let mv = |from: PathBuf, to: PathBuf| { if from.exists() && !to.exists() { let _ = std::fs::rename(&from, &to); } };
    mv(dir.join("notes.txt"), dir.join("en").join("notes.txt"));
    mv(dir.join("talk.txt"), dir.join("en").join("talk.txt"));
    for (old, new) in [("pictures", uni.join("pictures")), ("sounds", uni.join("sounds"))] {
        let od = dir.join(old);
        for f in std::fs::read_dir(&od).into_iter().flatten().flatten() {
            if f.file_name() == "put PNG pictures here.txt" { let _ = std::fs::remove_file(f.path()); continue; }
            mv(f.path(), new.join(f.file_name()));
        }
        let _ = std::fs::remove_dir(&od);
    }
    let put = |p: PathBuf, data: &[u8]| { if !p.exists() { let _ = std::fs::write(p, data); } };
    put(dir.join("README.txt"), README.as_bytes());
    put(uni.join("notes.txt"), UNIVERSAL_NOTES.as_bytes());
    put(uni.join("talk.txt"), UNIVERSAL_TALK.as_bytes());
    put(uni.join("sounds").join("honk.wav"), &honk_wav());
    put(uni.join("online.txt"), UNIVERSAL_ONLINE.as_bytes());
    for (code, _) in crate::i18n::LANGUAGES {
        let (notes, talk, online) = default_texts(code);
        put(dir.join(code).join("online.txt"), online.as_bytes());
        put(dir.join(code).join("notes.txt"), notes.as_bytes());
        put(dir.join(code).join("talk.txt"), talk.as_bytes());
    }
    dir
}

fn lines(path: &Path, section: Option<&str>) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut cur: Option<String> = None;
    text.lines()
        .map(|l| l.trim().trim_start_matches('\u{feff}'))
        .filter(|l| {
            if l.starts_with('[') && l.ends_with(']') { cur = Some(l[1..l.len() - 1].to_ascii_lowercase()); return false; }
            !l.is_empty() && !l.starts_with('#') && section.map(|s| cur.as_deref() == Some(s)).unwrap_or(true)
        })
        .map(|l| l.to_string())
        .collect()
}

fn files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.extension().map(|x| x.eq_ignore_ascii_case(ext)).unwrap_or(false)).collect()
}

fn pick<T: Clone>(v: &[T]) -> Option<T> {
    if v.is_empty() { None } else { Some(v[(random() * v.len() as f64) as usize % v.len()].clone()) }
}

// ---------------------------------------------------------------- state

pub struct Overlay { pub hwnd: HWND, pub gfx: Gfx }

impl Overlay {
    fn new(gfx: Gfx, click_through: bool, close_px: i32) -> Overlay { Overlay { hwnd: win::create_overlay(click_through, close_px), gfx } }
    fn show(&self, p: &win::Painter, x: i32, y: i32, alpha: u8) {
        p.update(self.hwnd, self.gfx.bmp, x, y, self.gfx.w, self.gfx.h, alpha);
        win::show(self.hwnd, true);
    }
    fn close(self) { unsafe { DestroyWindow(self.hwnd); DeleteObject(self.gfx.bmp); } }
}

struct Bubble { pet: u32, ov: Overlay, ticks: u32 }
struct Print { ov: Overlay, age: u32 }

enum Job {
    /// Walk to the screen edge, then come back dragging a note.
    ToEdge { pet: u32, edge: i32, meme: bool, ticks: u32 },
    Bring { pet: u32, ov: Overlay, dest: i32 },
    /// Jump at the cursor, then hang on it and drag it around.
    Leap { pet: u32, ticks: u32 },
    Grab { pet: u32, ticks: u32, shakes: u32, last: (i32, i32), dest: (i32, i32) },
}

pub struct Ball { ov: Overlay, x: f64, y: f64, vx: f64, vy: f64, r: f64, pub dragging: bool }

#[derive(Default)]
pub struct Pranks {
    dir: PathBuf,
    jobs: Vec<Job>,
    notes: Vec<Overlay>,
    bubbles: Vec<Bubble>,
    prints: Vec<Print>,
    pub ball: Option<Ball>,
    /// Next tick each timed prank may fire: notes, pictures, cursor, talk, honk, windows.
    next: [u64; 6],
    last_print: Vec<(u32, i32, i32)>,
    pub panic_hidden: bool,
    /// Prank windows were added; re-stack all Popeto windows at the end of the tick.
    pub restack: bool,
}

const ACT_RUN: [&str; 3] = ["Run", "Dash", "Walk"];
const ACT_WALK: [&str; 2] = ["Walk", "Run"];

impl Pranks {
    /// Content folders in use: universal + the current language.
    fn dirs(&self) -> [PathBuf; 2] { [self.dir.join("universal"), self.dir.join(crate::i18n::code())] }
    fn texts(&self, file: &str, section: Option<&str>) -> Vec<String> {
        self.dirs().iter().flat_map(|d| lines(&d.join(file), section)).collect()
    }
    /// Internet picture sources from `online.txt` in universal + the current language.
    pub fn online_sources(&self) -> Vec<crate::online::Source> {
        self.dirs().iter().flat_map(|d| crate::online::parse_sources(&std::fs::read_to_string(d.join("online.txt")).unwrap_or_default())).collect()
    }
    fn pool(&self, sub: &str, ext: &str) -> Vec<PathBuf> {
        self.dirs().iter().flat_map(|d| files(&d.join(sub), ext)).collect()
    }
    pub fn new(root: &Path) -> Pranks { let mut p = Pranks::default(); p.dir = root.join("pranks"); p }
    fn busy(&self, pet: u32) -> bool {
        self.jobs.iter().any(|j| match j { Job::ToEdge { pet: p, .. } | Job::Bring { pet: p, .. } | Job::Leap { pet: p, .. } | Job::Grab { pet: p, .. } => *p == pet })
    }
    pub fn holds_cursor(&self) -> bool { self.jobs.iter().any(|j| matches!(j, Job::Grab { .. })) }
    pub fn is_ball(&self, h: isize) -> bool { self.ball.as_ref().map(|b| b.ov.hwnd as isize == h).unwrap_or(false) }
    pub fn close_note(&mut self, h: isize) {
        if let Some(i) = self.notes.iter().position(|n| n.hwnd as isize == h) { self.notes.remove(i).close(); }
    }
    pub fn clear(&mut self) {
        for j in self.jobs.drain(..) { if let Job::Bring { ov, .. } = j { ov.close(); } }
        for n in self.notes.drain(..) { n.close(); }
        for b in self.bubbles.drain(..) { b.ov.close(); }
        for p in self.prints.drain(..) { p.ov.close(); }
        if let Some(b) = self.ball.take() { b.ov.close(); }
    }
    pub fn counts(&self) -> (usize, usize) { (self.notes.len(), self.prints.len()) }
    /// Prank windows that go above pets (bubbles, ball) and below them (notes, footprints).
    pub fn layers(&self) -> (Vec<HWND>, Vec<HWND>) {
        let mut top: Vec<HWND> = self.bubbles.iter().map(|b| b.ov.hwnd).collect();
        top.extend(self.ball.as_ref().map(|b| b.ov.hwnd));
        let mut bottom: Vec<HWND> = self.jobs.iter().filter_map(|j| if let Job::Bring { ov, .. } = j { Some(ov.hwnd) } else { None }).collect();
        bottom.extend(self.notes.iter().rev().map(|n| n.hwnd));
        bottom.extend(self.prints.iter().rev().map(|p| p.ov.hwnd));
        (top, bottom)
    }
}

impl Drop for Pranks { fn drop(&mut self) { self.clear(); } }

/// Ticks until a prank fires again; level 3 is the hidden test speed (Debug=true).
fn interval(level: u8, base_s: f64) -> u64 {
    let mult = [3.0, 1.0, 0.3, 0.04][level.min(3) as usize];
    ((base_s * mult * (0.5 + random())) * 40.0) as u64
}

impl App {
    fn ui_scale(&self) -> f64 { self.scale.max(0.5) }

    fn dbg(&mut self, s: &str) { if self.settings.debug { self.log.push(format!("[prank t={}] {s}", self.tick_n)); } }

    fn pet_index(&self, id: u32) -> Option<usize> { self.pets.iter().position(|p| p.m.id == id) }

    fn floor_of(&mut self, i: usize) -> BorderRef { self.with_pet(i, |m, _, env, _, _| env.floor(m.st.anchor)) }

    fn run_on(&mut self, i: usize, names: &[&str], params: &[(&str, &str)], tag: &str) -> bool {
        let names: Vec<String> = names.iter().map(|s| s.to_string()).collect();
        let params: Vec<(String, String)> = params.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let tag = tag.to_string();
        self.with_pet(i, |m, pack, env, _, log| {
            let ps: Vec<(&str, &str)> = params.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            names.iter().any(|n| m.run_action(pack, n, &ps, &tag, env, log))
        })
    }

    /// A random pet that is free, visible and standing on the bottom of the screen (or anything, if `any_place`).
    fn free_pet(&mut self, floor_only: bool) -> Option<usize> {
        let mut c: Vec<usize> = (0..self.pets.len())
            .filter(|&i| { let p = &self.pets[i]; p.w.shown && !p.m.st.dragging && !p.m.holding && !self.pranks.busy(p.m.id) })
            .collect();
        if floor_only { c.retain(|&i| self.floor_of(i) == BorderRef::Work(Side::Bottom)); }
        pick(&c)
    }

    pub fn say(&mut self, pet: u32, section: &str) {
        if !self.settings.pranks.talk { return; }
        let Some(line) = pick(&self.pranks.texts("talk.txt", Some(section))) else { return };
        if let Some(i) = self.pranks.bubbles.iter().position(|b| b.pet == pet) { self.pranks.bubbles.remove(i).ov.close(); }
        let ov = Overlay::new(bubble_gfx(&line, self.ui_scale()), true, 0);
        self.pranks.bubbles.push(Bubble { pet, ov, ticks: 0 });
    }

    pub fn honk(&mut self) {
        if !self.settings.pranks.honk { return; }
        if let Some(f) = pick(&self.pranks.pool("sounds", "wav")) { win::play_sound(&f); }
    }

    /// Start one prank right now (tray "Do a prank now", or the timers).
    pub fn prank_now(&mut self, kind: Option<usize>) {
        let p = self.settings.pranks.clone();
        let enabled: Vec<usize> = [p.notes, p.memes, p.cursor, p.talk, p.honk, p.windows].iter().enumerate().filter(|(_, e)| **e).map(|(i, _)| i).collect();
        let Some(k) = kind.or_else(|| pick(&enabled)) else { return };
        match k {
            0 | 1 => {
                if k == 1 && self.pranks.pool("pictures", "png").is_empty() && !self.online.ready() { self.dbg("pictures: none found"); return; }
                let Some(i) = self.free_pet(true) else { self.dbg("notes/pictures: no free pet on the floor"); return };
                let work = self.world.monitors[self.pets[i].m.st.monitor].work;
                let x = self.pets[i].m.st.anchor.0;
                let edge = if x - work.l < work.r - x { work.l } else { work.r };
                let es = edge.to_string();
                if self.run_on(i, &ACT_RUN, &[("TargetX", &es)], "#prank-edge") {
                    self.pranks.jobs.push(Job::ToEdge { pet: self.pets[i].m.id, edge, meme: k == 1, ticks: 0 });
                    self.dbg(&format!("notes/pictures: pet {} heads to edge {edge}", self.pets[i].m.id));
                } else { self.dbg("notes/pictures: pet has no Run/Walk action"); }
            }
            2 => {
                if self.pranks.holds_cursor() { return; }
                let Some(i) = self.free_pet(false) else { return };
                let pack = &self.packs[self.pets[i].m.pack];
                let off = |n: &str, d: f64| pack.actions.get("Pinched").and_then(|a| a.params.iter().find(|(k, _)| &**k == n).map(|(_, v)| v.clone()))
                    .and_then(|p| if let crate::config::ParamSrc::Lit(v) = &p.src { Some(v.num()) } else { None }).unwrap_or(d) * pack.scale;
                let (ox, oy) = (off("OffsetX", 0.0), off("OffsetY", 90.0));
                let tx = format!("#{{mascot.environment.cursor.x+{ox}}}");
                let ty = format!("#{{mascot.environment.cursor.y+{oy}}}");
                if self.run_on(i, &["Jumping"], &[("TargetX", &tx), ("TargetY", &ty), ("VelocityParam", "24")], "#prank-leap") {
                    self.pranks.jobs.push(Job::Leap { pet: self.pets[i].m.id, ticks: 0 });
                }
            }
            3 => { if let Some(i) = self.free_pet(false) { let id = self.pets[i].m.id; self.say(id, "random"); } }
            4 => self.honk(),
            _ => {
                // bother the window a pet is standing on: stomp-shake it or knock it down (minimize)
                let c: Vec<usize> = (0..self.pets.len()).filter(|&i| !self.pets[i].m.st.dragging).collect();
                for i in c {
                    if let BorderRef::Ie(Side::Top) = self.floor_of(i) {
                        let Some(h) = self.pets[i].m.st.ie else { continue };
                        let id = self.pets[i].m.id;
                        self.say(id, "window");
                        if random() < 0.5 { win::minimize_app_window(h); } else {
                            for k in 0..3 { self.shake_later(h, k * 14); }
                        }
                        break;
                    }
                }
            }
        }
    }

    fn shake_later(&mut self, h: isize, delay: usize) {
        if let Some(r) = self.world.win(h).map(|z| self.world.wins[z].rect) {
            self.shakes.push(crate::Shake { hwnd: h, t: 0, base: (r.l, r.t), delay });
        }
    }

    pub fn throw_ball(&mut self) {
        if let Some(b) = self.pranks.ball.take() { b.ov.close(); }
        let Some(m) = self.world.monitors.first() else { return };
        let r = (16.0 * self.ui_scale()).round();
        let ov = Overlay::new(ball_gfx(r as i32), false, 0);
        let x = m.work.l as f64 + 100.0 + random() * (m.work.w() as f64 - 200.0);
        self.pranks.ball = Some(Ball { ov, x, y: m.work.t as f64 + 40.0, vx: (random() - 0.5) * 16.0, vy: 0.0, r, dragging: false });
    }

    pub fn remove_ball(&mut self) { if let Some(b) = self.pranks.ball.take() { b.ov.close(); } }

    pub fn panic(&mut self) {
        self.release_cursor(false);
        self.pranks.clear();
        self.pranks.panic_hidden = !self.pranks.panic_hidden;
        let hide = self.pranks.panic_hidden;
        self.paused = hide;
        for p in &mut self.pets {
            win::show(p.w.hwnd, !hide);
            p.w.shown = !hide;
            p.w.last = None;
        }
    }

    fn release_cursor(&mut self, say: bool) {
        let mut freed = None;
        self.pranks.jobs.retain(|j| if let Job::Grab { pet, .. } = j { freed = Some(*pet); false } else { true });
        if let Some(i) = freed.and_then(|id| self.pet_index(id)) {
            self.pets[i].m.st.dragging = false;
            self.set_behavior(i, "Thrown");
            if say { let id = self.pets[i].m.id; self.say(id, "free"); }
        }
    }

    /// Called every tick before pets move.
    pub fn pranks_before(&mut self) {
        // the pet hanging on the cursor drags it along; shaking the mouse breaks free
        let grab = self.pranks.jobs.iter().position(|j| matches!(j, Job::Grab { .. }));
        if let Some(gi) = grab {
            let Job::Grab { pet, .. } = self.pranks.jobs[gi] else { unreachable!() };
            let alive = self.pet_index(pet).map(|i| self.pets[i].m.beh == self.packs[self.pets[i].m.pack].required("Dragged")).unwrap_or(false);
            let (cur, scale) = (self.world.cursor, self.scale);
            let Job::Grab { ticks, shakes, last, dest, .. } = &mut self.pranks.jobs[gi] else { unreachable!() };
            *ticks += 1;
            let user = ((cur.0 - last.0) as f64).hypot((cur.1 - last.1) as f64);
            if user > 45.0 * scale { *shakes += 1; }
            if !alive || *shakes >= 3 || *ticks > 200 {
                self.release_cursor(alive);
            } else {
                let (dx, dy) = ((dest.0 - cur.0) as f64, (dest.1 - cur.1) as f64);
                let d = dx.hypot(dy).max(1.0);
                let sp = (5.0 * scale).min(d);
                let wob = ((*ticks as f64) * 0.3).sin() * 2.0;
                let n = (cur.0 + (dx / d * sp) as i32, cur.1 + (dy / d * sp + wob) as i32);
                *last = n;
                win::set_cursor(n.0, n.1);
                self.world.cursor_d = (n.0 - self.world.cursor.0, n.1 - self.world.cursor.1);
                self.world.cursor = n;
            }
        }
    }

    /// Called every tick after pets moved and before rendering.
    pub fn pranks_after(&mut self) {
        let lvl = if self.settings.debug { 3 } else { self.settings.pranks.level };
        let p = self.settings.pranks.clone();
        let t = self.tick_n;
        let enabled = [p.notes, p.memes, p.cursor, p.talk, p.honk, p.windows];
        let base = [240.0, 300.0, 180.0, 45.0, 120.0, 150.0];
        if !self.pets.is_empty() && !self.pranks.panic_hidden {
            for k in 0..6 {
                if !enabled[k] { self.pranks.next[k] = 0; continue; }
                if self.pranks.next[k] == 0 { self.pranks.next[k] = t + interval(lvl, base[k]); }
                if t >= self.pranks.next[k] {
                    self.pranks.next[k] = t + interval(lvl, base[k]);
                    self.prank_now(Some(k));
                }
            }
        }
        if self.settings.debug && t == 120 && self.pranks.ball.is_none() { self.throw_ball(); }
        self.step_jobs();
        if p.footprints { self.step_prints(); }
        self.age_prints();
        self.step_ball();
        self.place_bubbles();
        if std::mem::take(&mut self.pranks.restack) { self.restack(); }
    }

    fn step_jobs(&mut self) {
        let jobs = std::mem::take(&mut self.pranks.jobs);
        let s = self.ui_scale();
        for job in jobs {
            match job {
                Job::ToEdge { pet, edge, meme, ticks } => {
                    let Some(i) = self.pet_index(pet) else { continue };
                    let m = &mut self.pets[i].m;
                    let fin = m.prank_finished.take().is_some();
                    let done = fin && m.st.anchor.0 == edge;
                    if fin && !done { let a = m.st.anchor.0; self.dbg(&format!("edge run ended at x={a}, wanted {edge}")); }
                    if done {
                        let gfx = if meme {
                            // fresh internet pictures when ready, mixed with the local ones
                            let local = self.pranks.pool("pictures", "png");
                            if self.online.ready() && (local.is_empty() || random() < 0.6) {
                                let pic = self.online.take();
                                self.dbg(&format!("picture from the internet: {:?}", pic.as_ref().map(|p| (p.w, p.h))));
                                pic.and_then(|p| meme_gfx_rgba(p.w, p.h, &p.rgba, s))
                            } else {
                                self.dbg("picture from disk");
                                pick(&local).and_then(|f| meme_gfx(&f, s))
                            }
                        } else {
                            pick(&self.pranks.texts("notes.txt", None)).map(|l| note_gfx(&l, s))
                        };
                        let Some(gfx) = gfx else { continue };
                        let work = self.world.monitors[self.pets[i].m.st.monitor].work;
                        let dest = work.l + gfx.w + 60 + (random() * (work.w() - 2 * gfx.w - 120).max(1) as f64) as i32;
                        let ds = dest.to_string();
                        if self.run_on(i, &ACT_WALK, &[("TargetX", &ds)], "#prank-bring") {
                            let ov = Overlay::new(gfx, false, (34.0 * s) as i32);
                            self.pranks.jobs.push(Job::Bring { pet, ov, dest });
                            self.say(pet, "note");
                        } else { unsafe { DeleteObject(gfx.bmp); } }
                    } else if self.pets[i].m.beh == "#prank-edge" && ticks < 1200 {
                        self.pranks.jobs.push(Job::ToEdge { pet, edge, meme, ticks: ticks + 1 });
                    }
                }
                Job::Bring { pet, ov, dest } => {
                    let Some(i) = self.pet_index(pet) else { ov.close(); continue };
                    let finished = self.pets[i].m.prank_finished.take().is_some();
                    let m = &self.pets[i].m;
                    let (a, lr) = (m.st.anchor, m.st.look_right);
                    let gap = (16.0 * s) as i32;
                    let x = if lr { a.0 - gap - ov.gfx.w } else { a.0 + gap };
                    ov.show(&self.painter, x, a.1 - ov.gfx.h + (6.0 * s) as i32, 255);
                    if finished || m.beh != "#prank-bring" || a.0 == dest {
                        self.pranks.notes.push(ov);
                        self.pranks.restack = true;
                    } else {
                        self.pranks.jobs.push(Job::Bring { pet, ov, dest });
                    }
                }
                Job::Leap { pet, ticks } => {
                    let Some(i) = self.pet_index(pet) else { continue };
                    let finished = self.pets[i].m.prank_finished.take().is_some();
                    if finished {
                        // landed on the cursor: hang on and take it for a walk
                        self.pets[i].m.st.dragging = true;
                        self.set_behavior(i, "Dragged");
                        let m = self.world.monitors[self.pets[i].m.st.monitor].work;
                        let dest = (m.l + 80 + (random() * (m.w() - 160).max(1) as f64) as i32, m.t + m.h() / 2 + (random() * (m.h() / 3) as f64) as i32);
                        self.pranks.jobs.push(Job::Grab { pet, ticks: 0, shakes: 0, last: self.world.cursor, dest });
                        self.say(pet, "grab");
                        self.honk();
                    } else if self.pets[i].m.beh == "#prank-leap" && ticks < 160 {
                        self.pranks.jobs.push(Job::Leap { pet, ticks: ticks + 1 });
                    }
                }
                g @ Job::Grab { .. } => self.pranks.jobs.push(g),
            }
        }
        for p in &mut self.pets { p.m.prank_finished = None; }
    }

    fn step_prints(&mut self) {
        let s = self.ui_scale();
        let stride = (30.0 * s) as i32;
        for i in 0..self.pets.len() {
            let (id, a, shown) = (self.pets[i].m.id, self.pets[i].m.st.anchor, self.pets[i].w.shown);
            if !shown || self.pets[i].m.st.dragging { continue; }
            let last = self.pranks.last_print.iter().position(|l| l.0 == id);
            let moved = match last { Some(k) => { let l = self.pranks.last_print[k]; l.2 == a.1 && (a.0 - l.1).abs() >= stride } None => true };
            if !moved {
                if let Some(k) = last { if self.pranks.last_print[k].2 != a.1 { self.pranks.last_print[k] = (id, a.0, a.1); } }
                continue;
            }
            if let Some(k) = last { self.pranks.last_print[k] = (id, a.0, a.1); } else { self.pranks.last_print.push((id, a.0, a.1)); continue; }
            if self.floor_of(i) == BorderRef::NotOn { continue; }
            if self.pranks.prints.len() >= 80 { self.pranks.prints.remove(0).ov.close(); }
            let ov = Overlay::new(print_gfx(s), true, 0);
            let (w, h) = (ov.gfx.w, ov.gfx.h);
            ov.show(&self.painter, a.0 - w / 2, a.1 - h + (2.0 * s) as i32, 190);
            self.pranks.prints.push(Print { ov, age: 0 });
            self.pranks.restack = true;
        }
        let ids: Vec<u32> = self.pets.iter().map(|p| p.m.id).collect();
        self.pranks.last_print.retain(|l| ids.contains(&l.0));
    }

    fn age_prints(&mut self) {
        let painter = &self.painter;
        self.pranks.prints.retain_mut(|p| {
            p.age += 1;
            const LIFE: u32 = 40 * 12;
            if p.age >= LIFE { unsafe { DestroyWindow(p.ov.hwnd); DeleteObject(p.ov.gfx.bmp); } return false; }
            if p.age > LIFE - 80 && p.age % 4 == 0 {
                let (x, y) = win::window_pos(p.ov.hwnd);
                painter.update(p.ov.hwnd, p.ov.gfx.bmp, x, y, p.ov.gfx.w, p.ov.gfx.h, (190 * (LIFE - p.age) / 80) as u8);
            }
            true
        });
    }

    fn place_bubbles(&mut self) {
        let s = self.ui_scale();
        let mut keep = Vec::new();
        for mut b in std::mem::take(&mut self.pranks.bubbles) {
            b.ticks += 1;
            let Some(i) = self.pet_index(b.pet) else { b.ov.close(); continue };
            if b.ticks > 130 || !self.pets[i].w.shown { b.ov.close(); continue; }
            let w = &self.pets[i].w;
            let mon = &self.world.monitors[self.pets[i].m.st.monitor.min(self.world.monitors.len() - 1)];
            let x = (w.pos.0 + w.size.0 / 2 - b.ov.gfx.w / 2).clamp(mon.screen.l, mon.screen.r - b.ov.gfx.w);
            let y = (w.pos.1 + (10.0 * s) as i32 - b.ov.gfx.h).max(mon.screen.t);
            b.ov.show(&self.painter, x, y, 255);
            if b.ticks == 1 { self.pranks.restack = true; }
            keep.push(b);
        }
        self.pranks.bubbles = keep;
    }

    pub fn grab_ball(&mut self, on: bool) {
        if let Some(b) = &mut self.pranks.ball {
            b.dragging = on;
            if on { win::capture(b.ov.hwnd); } else {
                win::release_capture();
                b.vx = self.world.cursor_d.0 as f64;
                b.vy = self.world.cursor_d.1 as f64;
            }
        }
    }

    fn step_ball(&mut self) {
        let Some(mut b) = self.pranks.ball.take() else { return };
        let s = self.ui_scale();
        let mon = self.world.monitor_for(0, b.x as i32, b.y as i32);
        let work = self.world.monitors[mon].work;
        if b.dragging {
            b.x = self.world.cursor.0 as f64;
            b.y = self.world.cursor.1 as f64;
        } else {
            b.vy += 0.9 * s;
            b.x += b.vx;
            b.y += b.vy;
            let floor = work.b as f64 - b.r;
            if b.y >= floor {
                b.y = floor;
                b.vy = if b.vy > 3.0 { -b.vy * 0.62 } else { 0.0 };
                b.vx *= 0.96;
                if b.vx.abs() < 0.2 { b.vx = 0.0; }
            }
            if b.x < work.l as f64 + b.r { b.x = work.l as f64 + b.r; b.vx = b.vx.abs() * 0.7; }
            if b.x > work.r as f64 - b.r { b.x = work.r as f64 - b.r; b.vx = -b.vx.abs() * 0.7; }
            if b.y < work.t as f64 + b.r { b.y = work.t as f64 + b.r; b.vy = b.vy.abs(); }
        }
        // pets on the floor chase the ball and kick it
        let on_floor = b.y >= work.b as f64 - b.r - 4.0;
        for i in 0..self.pets.len() {
            let (id, a) = (self.pets[i].m.id, self.pets[i].m.st.anchor);
            if self.pets[i].m.st.dragging || self.pranks.busy(id) { continue; }
            if a.1 != work.b { continue; }
            let dx = b.x - a.0 as f64;
            if dx.abs() < b.r + 18.0 * s && on_floor && !b.dragging {
                let dir = if dx >= 0.0 { 1.0 } else { -1.0 };
                b.vx = dir * (9.0 + random() * 9.0) * s;
                b.vy = -(7.0 + random() * 9.0) * s;
                if random() < 0.3 { self.honk(); }
            } else if self.tick_n % 20 == (id as u64 % 20) && random() < 0.6 {
                let chasing = self.pets[i].m.beh == "#prank-ball";
                if chasing || random() < 0.5 {
                    let tx = (b.x as i32).to_string();
                    self.run_on(i, &ACT_RUN, &[("TargetX", &tx)], "#prank-ball");
                }
            }
        }
        b.ov.show(&self.painter, (b.x - b.r) as i32 - 1, (b.y - b.r) as i32 - 1, 255);
        self.pranks.ball = Some(b);
    }
}

