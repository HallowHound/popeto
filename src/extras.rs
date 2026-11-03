//! Extras on top of life: break / water reminders and a kitchen timer, messages from the command line
//! (`Popeto.exe --say "Build done"`), dancing to music, the laser pointer and chalk doodles, pet name tags.

use crate::env::{BorderRef, Rect};
use crate::expr::random;
use crate::life::Emote;
use crate::pranks::{cover, pick, rrect, Canvas, Gfx, Overlay, ACT_RUN};
use crate::win;
use crate::App;
use std::collections::HashMap;
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{DeleteObject, DT_CENTER, DT_SINGLELINE, DT_VCENTER};

/// Kitchen timer lengths offered in the tray menu, minutes.
pub const TIMERS: [u32; 7] = [5, 10, 15, 25, 30, 45, 60];

struct Doodle { ov: Overlay, strokes: Vec<Vec<(f32, f32)>>, color: (u8, u8, u8), x: i32, y: i32, age: u32, shown: f32 }

#[derive(Default)]
pub struct Extras {
    /// Seconds at the computer since the last break / water reminder.
    active_s: u32,
    water_s: u32,
    /// Kitchen timer: fires at this tick.
    pub timer_at: Option<u64>,
    /// Command-line messages waiting until pets are on screen: (kind, text).
    pub remote: Vec<(String, String)>,
    meter: crate::audio::Meter,
    fast: f32,
    slow: f32,
    loud: u32,
    pub music: bool,
    last_beat: u64,
    beats: u32,
    bob: f64,
    /// Pets dancing right now.
    dancers: Vec<u32>,
    pub laser: Option<Overlay>,
    laser_still: u32,
    laser_cool: HashMap<u32, u64>,
    pub chalk: bool,
    next_chalk: u64,
    doodles: Vec<Doodle>,
    tag: Option<(u32, Overlay)>,
    hover: (u32, u32),
    /// Desktop icon platforms, refreshed every few seconds.
    pub icons: Vec<Rect>,
}

impl Extras {
    pub fn layers(&self) -> (Vec<HWND>, Vec<HWND>) {
        let mut top: Vec<HWND> = self.laser.iter().map(|o| o.hwnd).collect();
        top.extend(self.tag.as_ref().map(|t| t.1.hwnd));
        (top, self.doodles.iter().map(|d| d.ov.hwnd).collect())
    }
    pub fn clear(&mut self) {
        if let Some(l) = self.laser.take() { l.close(); }
        for d in self.doodles.drain(..) { d.ov.close(); }
        if let Some((_, t)) = self.tag.take() { t.close(); }
        self.dancers.clear();
    }
    pub fn clear_doodles(&mut self) { for d in self.doodles.drain(..) { d.ov.close(); } }
    pub fn doodle_count(&self) -> usize { self.doodles.len() }
    /// Upward offset of a dancing pet, pixels.
    pub fn bob(&self, id: u32, s: f64) -> i32 { if self.dancers.contains(&id) { (self.bob * 7.0 * s) as i32 } else { 0 } }
}

impl Drop for Extras { fn drop(&mut self) { self.clear(); } }

// ---------------------------------------------------------------- drawing

fn dot_gfx(s: f64) -> Gfx {
    let d = (16.0 * s) as i32 | 1;
    let mut c = Canvas::new(d, d);
    c.fill((255, 30, 30));
    let r = d as f32 / 2.0;
    let bmp = c.finish(move |x, y| {
        let q = (x - r).hypot(y - r);
        // bright core and a soft red glow
        cover(q - r * 0.35).max(((1.0 - q / r) * 0.6).clamp(0.0, 1.0))
    });
    Gfx { bmp, w: d, h: d }
}

fn tag_gfx(text: &str, s: f64) -> Gfx {
    let sc = |v: f64| (v * s).round() as i32;
    let mut probe = Canvas::new(8, 8);
    let (tw, _) = probe.measure(text, sc(300.0), sc(13.0), true);
    probe.finish(|_, _| 0.0);
    let (w, h) = (tw + sc(16.0), sc(22.0));
    let mut c = Canvas::new(w, h);
    c.fill((45, 45, 55));
    c.text(text, RECT { left: 0, top: 0, right: w, bottom: h }, sc(13.0), true, (255, 255, 255), DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    let (wf, hf) = (w as f32, h as f32);
    let bmp = c.finish(move |x, y| cover(rrect(x, y, 0.5, 0.5, wf - 0.5, hf - 0.5, hf / 2.0)) * 0.85);
    Gfx { bmp, w, h }
}

/// Doodles as strokes in a 0..1 box.
fn doodle_shape() -> Vec<Vec<(f32, f32)>> {
    use std::f32::consts::TAU;
    let circle = |cx: f32, cy: f32, r: f32, n: usize| -> Vec<(f32, f32)> { (0..=n).map(|k| { let a = k as f32 / n as f32 * TAU; (cx + r * a.cos(), cy + r * a.sin()) }).collect() };
    match (random() * 7.0) as u32 {
        0 => vec![(0..=48).map(|k| {
            let t = k as f32 / 48.0 * TAU;
            let x = 16.0 * t.sin().powi(3);
            let y = 13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos();
            (0.5 + x / 36.0, 0.47 - y / 36.0)
        }).collect()],
        1 => vec![(0..=10).map(|k| { let a = k as f32 / 10.0 * TAU - TAU / 4.0; let r = if k % 2 == 0 { 0.46 } else { 0.19 }; (0.5 + r * a.cos(), 0.53 + r * a.sin()) }).collect()],
        2 => vec![circle(0.5, 0.5, 0.42, 36), circle(0.35, 0.4, 0.04, 8), circle(0.65, 0.4, 0.04, 8),
                  (0..=16).map(|k| { let a = 0.15 * TAU + k as f32 / 16.0 * 0.2 * TAU; (0.5 + 0.25 * a.cos(), 0.5 + 0.25 * a.sin()) }).collect()],
        3 => {
            let mut v = vec![circle(0.5, 0.5, 0.2, 24)];
            for k in 0..8 { let a = k as f32 / 8.0 * TAU; v.push(vec![(0.5 + 0.28 * a.cos(), 0.5 + 0.28 * a.sin()), (0.5 + 0.45 * a.cos(), 0.5 + 0.45 * a.sin())]); }
            v
        }
        4 => {
            let mut v = vec![vec![(0.5, 0.45), (0.52, 0.98)], vec![(0.51, 0.75), (0.68, 0.65)], circle(0.5, 0.3, 0.07, 12)];
            for k in 0..5 { let a = k as f32 / 5.0 * TAU; v.push(circle(0.5 + 0.16 * a.cos(), 0.3 + 0.16 * a.sin(), 0.09, 14)); }
            v
        }
        5 => vec![vec![(0.15, 0.95), (0.15, 0.5), (0.85, 0.5), (0.85, 0.95), (0.15, 0.95)], vec![(0.08, 0.55), (0.5, 0.12), (0.92, 0.55)],
                  vec![(0.42, 0.95), (0.42, 0.72), (0.58, 0.72), (0.58, 0.95)]],
        _ => vec![circle(0.5, 0.58, 0.32, 32), vec![(0.24, 0.42), (0.22, 0.1), (0.42, 0.28)], vec![(0.58, 0.28), (0.78, 0.1), (0.76, 0.42)],
                  circle(0.39, 0.55, 0.035, 8), circle(0.61, 0.55, 0.035, 8),
                  vec![(0.05, 0.62), (0.32, 0.66)], vec![(0.05, 0.74), (0.32, 0.7)], vec![(0.95, 0.62), (0.68, 0.66)], vec![(0.95, 0.74), (0.68, 0.7)]],
    }
}

/// The first `part` (0..1) of the doodle in chalk.
fn doodle_gfx(strokes: &[Vec<(f32, f32)>], color: (u8, u8, u8), part: f32, d: i32, s: f64) -> Gfx {
    let mut c = Canvas::new(d, d);
    c.fill(color);
    let pad = 4.0 * s as f32;
    let k = d as f32 - 2.0 * pad;
    let mut segs: Vec<((f32, f32), (f32, f32))> = Vec::new();
    for st in strokes { for w in st.windows(2) { segs.push(((pad + w[0].0 * k, pad + w[0].1 * k), (pad + w[1].0 * k, pad + w[1].1 * k))); } }
    let n = ((segs.len() as f32 * part).ceil() as usize).min(segs.len());
    segs.truncate(n);
    let half = 2.2 * s as f32;
    let bmp = c.finish(move |x, y| {
        let mut best = f32::MAX;
        for ((ax, ay), (bx, by)) in &segs {
            let (dx, dy) = (bx - ax, by - ay);
            let l2 = dx * dx + dy * dy;
            let t = if l2 > 0.0 { (((x - ax) * dx + (y - ay) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
            best = best.min((x - ax - t * dx).hypot(y - ay - t * dy));
        }
        // dusty chalk: a little grain
        let h = ((x as u32).wrapping_mul(73856093) ^ (y as u32).wrapping_mul(19349663)) % 100;
        cover(best - half) * (0.6 + h as f32 / 250.0)
    });
    Gfx { bmp, w: d, h: d }
}

// ---------------------------------------------------------------- app side

impl App {
    pub fn extras_tick(&mut self) {
        let t = self.tick_n;
        if t % 40 == 0 { self.step_reminders(); }
        if let Some(at) = self.extras.timer_at {
            if t >= at {
                self.extras.timer_at = None;
                let line = pick(&self.pranks.texts("talk.txt", Some("timer"))).unwrap_or_else(|| "!".into());
                self.bring_note(line);
                self.honk_always();
            }
        }
        if t > 80 && !self.extras.remote.is_empty() {
            for (kind, text) in std::mem::take(&mut self.extras.remote) { self.remote(&kind, &text); }
        }
        if self.settings.life.icons && self.settings.interact {
            if t % 120 == 1 { self.extras.icons = crate::icons::desktop_icons(); if self.settings.debug { self.dbg(&format!("desktop icons: {:?}", self.extras.icons.iter().take(4).map(|r| (r.l, r.t, r.w())).collect::<Vec<_>>())); } }
        } else { self.extras.icons.clear(); }
        if self.paused { return; }
        self.step_dance();
        self.step_laser();
        self.step_chalk();
        self.step_tag();
    }

    pub fn honk_always(&mut self) {
        if let Some(f) = pick(&self.pranks.pool("sounds", "wav")) { win::play_sound(&f); }
    }

    // ---------------------------------------------------------------- reminders and messages

    fn step_reminders(&mut self) {
        let l = &self.settings.life;
        let (brk, water) = (l.break_minutes, l.water_minutes);
        let idle = win::idle_ms();
        if idle > 5 * 60_000 {
            // you were away: that was a break
            self.extras.active_s = 0;
            return;
        }
        if idle > 60_000 { return; }
        self.extras.active_s += 1;
        self.extras.water_s += 1;
        if brk > 0 && self.extras.active_s >= brk * 60 {
            self.extras.active_s = 0;
            self.remind("break");
        } else if water > 0 && self.extras.water_s >= water * 60 {
            self.extras.water_s = 0;
            self.remind("water");
        }
        if brk == 0 { self.extras.active_s = 0; }
        if water == 0 { self.extras.water_s = 0; }
    }

    fn remind(&mut self, section: &str) {
        let Some(line) = pick(&self.pranks.texts("talk.txt", Some(section))) else { return };
        self.dbg(&format!("reminder: {line}"));
        self.bring_note(line);
    }

    /// `Popeto.exe --say text` / `--note text` (from a script, a build, ...).
    pub fn remote(&mut self, kind: &str, text: &str) {
        self.dbg(&format!("remote {kind}: {text}"));
        if kind == "note" { self.bring_note(text.to_string()); return; }
        let shown: Vec<usize> = (0..self.pets.len()).filter(|&i| self.pets[i].w.shown).collect();
        let free: Vec<usize> = shown.iter().copied().filter(|&i| !self.pets[i].m.st.dragging && !self.pranks.busy(self.pets[i].m.id)).collect();
        let Some(i) = pick(&free).or_else(|| pick(&shown)) else { self.show_note(text); return };
        let id = self.pets[i].m.id;
        self.say_text(id, text, 200 + text.chars().count() as u32 * 4);
        self.emote(id, Emote::Bang);
    }

    // ---------------------------------------------------------------- dancing

    fn step_dance(&mut self) {
        let x = &mut self.extras;
        x.bob *= 0.8;
        if !self.settings.life.dance || self.life.away {
            if x.music { x.music = false; x.dancers.clear(); }
            return;
        }
        let p = x.meter.peak();
        x.fast += (p - x.fast) * 0.35;
        x.slow += (p - x.slow) * 0.03;
        x.loud = if p > 0.02 { (x.loud + 1).min(200) } else { x.loud.saturating_sub(3) };
        // a few seconds of sound in a row is music (a notification ding isn't)
        if !x.music && x.loud >= 140 { x.music = true; self.dbg("music: on"); }
        let x = &mut self.extras;
        if x.music && x.loud == 0 { x.music = false; x.dancers.clear(); self.dbg("music: off"); return; }
        let x = &mut self.extras;
        if !x.music { return; }
        let t = self.tick_n;
        let beat = x.fast > x.slow * 1.3 && x.fast > 0.04 && t >= x.last_beat + 10;
        let s = self.scale;
        // keep dancers dancing, invite free pets that stand on something
        if t % 80 == 0 {
            let dancers = std::mem::take(&mut self.extras.dancers);
            let mut keep = Vec::new();
            for i in 0..self.pets.len() {
                let id = self.pets[i].m.id;
                let was = dancers.contains(&id) && self.pets[i].m.beh == "#life-dance";
                if !was && !(self.pet_free(i) && self.floor_of(i) != BorderRef::NotOn && random() < 0.7) { continue; }
                let pose: &[&str] = if !was && random() < 0.2 { &["Sit", "Stand"] } else { &["Stand"] };
                if self.run_on(i, pose, &[("Duration", "100")], "#life-dance") { keep.push(id); }
            }
            self.extras.dancers = keep;
        }
        self.extras.dancers.retain(|id| self.pets.iter().any(|p| p.m.id == *id && p.m.beh == "#life-dance" && !p.m.st.dragging));
        if beat {
            let x = &mut self.extras;
            x.last_beat = t;
            x.beats += 1;
            x.bob = 1.0;
            let flip = x.beats % 2 == 0;
            let note = x.beats % 4 == 0;
            let dancers = x.dancers.clone();
            for id in &dancers {
                if let Some(i) = self.pet_index(*id) { if flip { self.pets[i].m.st.look_right = !self.pets[i].m.st.look_right; } }
            }
            if note { if let Some(id) = pick(&dancers) { self.emote(id, Emote::Note); } }
            let _ = s;
        }
    }

    // ---------------------------------------------------------------- laser pointer

    pub fn toggle_laser(&mut self) {
        if let Some(l) = self.extras.laser.take() { l.close(); return; }
        self.extras.laser = Some(Overlay::new(dot_gfx(self.ui_scale()), true, 0));
        self.extras.laser_still = 0;
        self.pranks.restack = true;
    }

    fn step_laser(&mut self) {
        let Some(l) = &self.extras.laser else { return };
        let (c, d) = (self.world.cursor, self.world.cursor_d);
        l.show(&self.painter, c.0 - l.gfx.w / 2, c.1 - l.gfx.h / 2, 235);
        self.extras.laser_still = if d == (0, 0) { self.extras.laser_still + 1 } else { 0 };
        // switch off by itself after two minutes without moving the mouse
        if self.extras.laser_still > 40 * 120 { self.toggle_laser(); return; }
        if self.tick_n % 6 != 0 { return; }
        let s = self.scale;
        let t = self.tick_n;
        for i in 0..self.pets.len() {
            let id = self.pets[i].m.id;
            let chasing = self.pets[i].m.beh == "#life-laser";
            if !(chasing || self.pet_free(i)) || self.pets[i].m.st.dragging { continue; }
            if self.floor_of(i) == BorderRef::NotOn { continue; }
            let (a, w) = (self.pets[i].m.st.anchor, &self.pets[i].w);
            let r = Rect { l: w.pos.0, t: w.pos.1, r: w.pos.0 + w.size.0, b: w.pos.1 + w.size.1 };
            if r.contains(c.0, c.1) {
                if self.extras.laser_cool.get(&id).map(|&k| t >= k).unwrap_or(true) {
                    self.extras.laser_cool.insert(id, t + 80);
                    self.emote(id, Emote::Star);
                    if random() < 0.3 { self.say_always(id, "laser"); }
                }
                continue;
            }
            let (dx, dy) = (c.0 - a.0, a.1 - c.1);
            if dx.abs() > (700.0 * s) as i32 || dy < -(20.0 * s) as i32 || dy > (420.0 * s) as i32 { continue; }
            let cool = self.extras.laser_cool.get(&id).map(|&k| t < k).unwrap_or(false);
            if dx.abs() < (60.0 * s) as i32 && dy > (40.0 * s) as i32 && !cool {
                // pounce
                self.extras.laser_cool.insert(id, t + 60);
                let (tx, ty) = (c.0.to_string(), (c.1 + (30.0 * s) as i32).to_string());
                self.run_on(i, &["Jumping"], &[("TargetX", &tx), ("TargetY", &ty)], "#life-laser");
            } else if !chasing || t % 18 == 0 {
                let tx = c.0.to_string();
                self.run_on(i, &ACT_RUN, &[("TargetX", &tx)], "#life-laser");
            }
        }
    }

    // ---------------------------------------------------------------- chalk

    pub fn toggle_chalk(&mut self) {
        self.extras.chalk = !self.extras.chalk;
        if self.extras.chalk { self.draw_doodle(); }
    }

    /// A free pet draws a little picture in chalk next to itself.
    pub fn draw_doodle(&mut self) {
        let free: Vec<usize> = (0..self.pets.len()).filter(|&i| self.pet_free(i)).collect();
        let free: Vec<usize> = free.into_iter().filter(|&i| self.floor_of(i) != BorderRef::NotOn).collect();
        let Some(i) = pick(&free) else { return };
        let s = self.ui_scale();
        let d = (110.0 * s) as i32;
        let (a, lr) = (self.pets[i].m.st.anchor, self.pets[i].m.st.look_right);
        let half = self.pets[i].w.size.0 / 2;
        let mon = &self.world.monitors[self.pets[i].m.st.monitor.min(self.world.monitors.len() - 1)];
        let mut x = if lr { a.0 + half } else { a.0 - half - d };
        x = x.clamp(mon.screen.l, mon.screen.r - d);
        let y = (a.1 - d - (4.0 * s) as i32).max(mon.screen.t);
        if !self.run_on(i, &["Stand", "Sit"], &[("Duration", "170")], "#life-chalk") { return; }
        let colors = [(255, 255, 255), (255, 214, 102), (130, 210, 255), (255, 150, 200), (170, 240, 150)];
        let color = pick(&colors).unwrap();
        let strokes = doodle_shape();
        let ov = Overlay::new(doodle_gfx(&strokes, color, 0.0, d, s), true, 0);
        if self.extras.doodles.len() >= 12 { self.extras.doodles.remove(0).ov.close(); }
        let id = self.pets[i].m.id;
        self.dbg(&format!("pet {id} draws in chalk"));
        if random() < 0.5 { self.say_always(id, "chalk"); }
        self.extras.doodles.push(Doodle { ov, strokes, color, x, y, age: 0, shown: 0.0 });
        self.pranks.restack = true;
    }

    fn step_chalk(&mut self) {
        let t = self.tick_n;
        if self.extras.chalk && t >= self.extras.next_chalk {
            if self.extras.next_chalk != 0 { self.draw_doodle(); }
            self.extras.next_chalk = t + 40 * (40 + (random() * 60.0) as u64);
        }
        const LIFE: u32 = 40 * 45;
        let s = self.ui_scale();
        let painter = &self.painter;
        self.extras.doodles.retain_mut(|d| {
            d.age += 1;
            if d.age >= LIFE { unsafe { windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(d.ov.hwnd); DeleteObject(d.ov.gfx.bmp); } return false; }
            // drawn stroke by stroke over ~4 seconds
            if d.shown < 1.0 && d.age % 4 == 0 {
                d.shown = (d.age as f32 / 160.0).min(1.0);
                let g = doodle_gfx(&d.strokes, d.color, d.shown, d.ov.gfx.w, s);
                unsafe { DeleteObject(d.ov.gfx.bmp); }
                d.ov.gfx = g;
            } else if d.age < LIFE - 120 && d.age % 4 != 1 {
                return true;
            }
            let fade = if d.age > LIFE - 120 { (LIFE - d.age) as f64 / 120.0 } else { 1.0 };
            d.ov.show(painter, d.x, d.y, (fade * 230.0) as u8);
            true
        });
    }

    // ---------------------------------------------------------------- names

    pub fn pet_name(&self, i: usize) -> Option<String> {
        let pack = &self.packs[self.pets[i].m.pack].name;
        if let Some(v) = self.lan.as_ref().and_then(|l| l.visitor_label(self.pets[i].m.id, pack)) { return Some(v); }
        self.settings.names.get(pack).filter(|s| !s.is_empty()).cloned()
    }

    pub fn is_visitor_pack(&self, pack: usize) -> bool {
        self.lan.as_ref().map(|l| l.visitor_packs.contains(&self.packs[pack].name)).unwrap_or(false)
    }

    fn step_tag(&mut self) {
        if !self.settings.life.show_names {
            if let Some((_, o)) = self.extras.tag.take() { o.close(); }
            return;
        }
        let c = self.world.cursor;
        let under = win::window_at(c.0, c.1);
        let hit = self.pets.iter().position(|p| p.w.shown && p.w.hwnd as isize == under);
        let id = hit.map(|i| self.pets[i].m.id).unwrap_or(0);
        if id != self.extras.hover.0 { self.extras.hover = (id, 0); } else { self.extras.hover.1 += 1; }
        let want = hit.filter(|_| self.extras.hover.1 >= 12).and_then(|i| self.pet_name(i).map(|n| (i, n)));
        match want {
            None => if let Some((_, o)) = self.extras.tag.take() { o.close(); },
            Some((i, name)) => {
                if self.extras.tag.as_ref().map(|t| t.0 != id).unwrap_or(true) {
                    if let Some((_, o)) = self.extras.tag.take() { o.close(); }
                    self.extras.tag = Some((id, Overlay::new(tag_gfx(&name, self.ui_scale()), true, 0)));
                    self.pranks.restack = true;
                }
                let w = &self.pets[i].w;
                let o = &self.extras.tag.as_ref().unwrap().1;
                o.show(&self.painter, w.pos.0 + w.size.0 / 2 - o.gfx.w / 2, w.pos.1 + w.size.1 + 2, 255);
            }
        }
    }
}
