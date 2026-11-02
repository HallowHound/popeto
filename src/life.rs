//! Pets' own life on top of their Shimeji behaviours: they stand on each other, greet each other,
//! play tag and follow-the-leader, hide behind and push windows, sleep while you're away, react to
//! the time of day, show little emotes, and play with toys (cookie, balloon, bed; ball fetch).

use crate::env::{BorderRef, Out, PetInfo, Rect, Side, Win};
use crate::expr::random;
use crate::pranks::{cover, pick, rrect, Canvas, Gfx, Overlay, ACT_RUN, ACT_WALK};
use crate::win;
use crate::App;
use std::collections::{HashMap, HashSet};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{DT_CENTER, DT_SINGLELINE, DT_VCENTER};

#[derive(Clone, Copy, PartialEq)]
pub enum Emote { Heart, Bang, Zzz, Star, Crumb, Note }

struct Particle { ov: Overlay, x: f64, y: f64, vx: f64, vy: f64, age: u32, life: u32 }

enum Game {
    Tag { chaser: u32, runner: u32, ticks: u32, swaps: u32 },
    Follow { leader: u32, dest: i32, followers: Vec<u32>, ticks: u32 },
}

#[derive(Clone, Copy, PartialEq)]
pub enum ToyKind { Cookie, Balloon, Bed }

pub struct Toy {
    pub kind: ToyKind,
    ov: Overlay,
    /// Cookie/bed: bottom centre. Balloon: centre.
    x: f64,
    y: f64,
    vy: f64,
    base_x: f64,
    dragging: bool,
    bites: u32,
    eater: Option<u32>,
    timer: u32,
}

struct Push { pet: u32, hwnd: isize, dir: i32, ticks: u32 }

#[derive(Default)]
pub struct Life {
    particles: Vec<Particle>,
    game: Option<Game>,
    pub toys: Vec<Toy>,
    meet_cool: HashMap<(u32, u32), u64>,
    meeting: HashMap<u32, u32>,
    /// Pets hiding behind a window: pet id -> window.
    pub hidden: HashMap<u32, isize>,
    push: Option<Push>,
    asleep: HashSet<u32>,
    /// You haven't touched keyboard or mouse for a while: pets are asleep.
    pub away: bool,
    said: HashSet<String>,
    heads_prev: HashMap<u32, Rect>,
    /// Pets walking to the bed.
    to_bed: HashSet<u32>,
    /// No new meeting before this tick (one chat at a time across all pets).
    next_meet: u64,
}

const SLEEP_ACTS: [&str; 6] = ["Sprawl", "LayDownAction", "Sleep", "SitDownPose", "Sit", "Stand"];

impl Life {
    /// Busy with something from this module, so pranks and other events leave it alone.
    pub fn busy(&self, id: u32) -> bool {
        self.meeting.contains_key(&id)
            || self.asleep.contains(&id)
            || self.to_bed.contains(&id)
            || self.push.as_ref().map(|p| p.pet == id).unwrap_or(false)
            || self.toys.iter().any(|t| t.eater == Some(id))
            || match &self.game {
                Some(Game::Tag { chaser, runner, .. }) => *chaser == id || *runner == id,
                Some(Game::Follow { leader, followers, .. }) => *leader == id || followers.contains(&id),
                None => false,
            }
    }
    /// Windows above pets (particles, balloons) and below them (cookies, beds).
    pub fn layers(&self) -> (Vec<HWND>, Vec<HWND>) {
        let mut top: Vec<HWND> = self.particles.iter().map(|p| p.ov.hwnd).collect();
        let mut bottom = Vec::new();
        for t in &self.toys { if t.kind == ToyKind::Balloon { top.push(t.ov.hwnd) } else { bottom.push(t.ov.hwnd) } }
        (top, bottom)
    }
    pub fn clear(&mut self) {
        for t in self.toys.drain(..) { t.ov.close(); }
        for p in self.particles.drain(..) { p.ov.close(); }
        self.game = None;
        self.meeting.clear();
        self.push = None;
    }
    pub fn has_bed(&self) -> bool { self.toys.iter().any(|t| t.kind == ToyKind::Bed) }
}

impl Drop for Life { fn drop(&mut self) { self.clear(); } }

// ---------------------------------------------------------------- drawing

/// A glyph (♥ ! z ★ ...) in one colour with soft edges.
fn glyph_gfx(text: &str, color: (u8, u8, u8), size: i32) -> Gfx {
    let d = size * 2;
    let mut c = Canvas::new(d, d);
    c.fill((0, 0, 0));
    c.text(text, RECT { left: 0, top: 0, right: d, bottom: d }, size, true, (255, 255, 255), DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    let px = c.px();
    let mut alpha = vec![0f32; (d * d) as usize];
    for (i, p) in px.chunks_mut(4).enumerate() {
        alpha[i] = p[0].max(p[1]).max(p[2]) as f32 / 255.0;
        p[0] = color.2; p[1] = color.1; p[2] = color.0;
    }
    let bmp = c.finish(move |x, y| alpha[(y as i32 * d + x as i32) as usize]);
    Gfx { bmp, w: d, h: d }
}

fn emote_gfx(e: Emote, s: f64) -> Gfx {
    let sz = (18.0 * s) as i32;
    match e {
        Emote::Heart => glyph_gfx("\u{2665}", (235, 64, 96), sz),
        Emote::Bang => glyph_gfx("!", (255, 140, 0), sz),
        Emote::Zzz => glyph_gfx("z", (90, 130, 230), sz),
        Emote::Star => glyph_gfx("\u{2605}", (255, 196, 40), sz),
        Emote::Crumb => glyph_gfx("\u{2022}", (150, 95, 40), sz / 2 + 4),
        Emote::Note => glyph_gfx("\u{266A}", (130, 80, 210), sz),
    }
}

fn cookie_gfx(s: f64, bites: u32) -> Gfx {
    let r = 15.0 * s as f32;
    let d = (r * 2.0) as i32 + 4;
    let mut c = Canvas::new(d, d);
    let cx = d as f32 / 2.0;
    let bite = |x: f32, y: f32| -> bool {
        let spots = [(cx + r * 0.8, cx - r * 0.6), (cx - r * 0.9, cx - r * 0.2), (cx + r * 0.2, cx + r * 0.95)];
        spots.iter().take(bites as usize).any(|(bx, by)| (x - bx).hypot(y - by) < r * 0.5)
    };
    {
        let px = c.px();
        for y in 0..d { for x in 0..d {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let chip = [(0.3f32, -0.3f32), (-0.35, 0.1), (0.1, 0.4), (-0.1, -0.45), (0.45, 0.25)]
                .iter().any(|(ox, oy)| (fx - cx - ox * r).hypot(fy - cx - oy * r) < r * 0.14);
            let (cr, cg, cb) = if chip { (80, 45, 25) } else { (222, 170, 100) };
            let i = ((y * d + x) * 4) as usize;
            px[i] = cb; px[i + 1] = cg; px[i + 2] = cr;
        } }
    }
    let bmp = c.finish(move |x, y| if bite(x, y) { 0.0 } else { cover((x - cx).hypot(y - cx) - r) });
    Gfx { bmp, w: d, h: d }
}

fn balloon_gfx(s: f64) -> Gfx {
    let (rw, rh) = (18.0 * s as f32, 23.0 * s as f32);
    let (w, h) = ((rw * 2.0) as i32 + 4, (rh * 2.0 + 22.0 * s as f32) as i32);
    let mut c = Canvas::new(w, h);
    let (cx, cy) = (w as f32 / 2.0, rh + 2.0);
    {
        let px = c.px();
        for y in 0..h { for x in 0..w {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let hl = (fx - cx + rw * 0.35).hypot(fy - cy + rh * 0.4) < rw * 0.25;
            let (r, g, b) = if fy > cy + rh { (120, 120, 120) } else if hl { (255, 170, 170) } else { (225, 45, 60) };
            let i = ((y * w + x) * 4) as usize;
            px[i] = b; px[i + 1] = g; px[i + 2] = r;
        } }
    }
    let bmp = c.finish(move |x, y| {
        let e = ((x - cx) / rw).powi(2) + ((y - cy) / rh).powi(2);
        let body = ((1.0 - e) * rw).clamp(0.0, 1.0);
        let string = if y > cy + rh && (x - cx - ((y - cy - rh) * 0.4).sin() * 2.0).abs() < 0.8 { 1.0 } else { 0.0 };
        body.max(string)
    });
    Gfx { bmp, w, h }
}

fn bed_gfx(s: f64) -> Gfx {
    let (w, h) = ((120.0 * s) as i32, (30.0 * s) as i32);
    let mut c = Canvas::new(w, h);
    {
        let px = c.px();
        for y in 0..h { for x in 0..w {
            let top = (y as f32) < h as f32 * 0.45;
            let (r, g, b) = if top { (205, 175, 235) } else { (150, 110, 200) };
            let i = ((y * w + x) * 4) as usize;
            px[i] = b; px[i + 1] = g; px[i + 2] = r;
        } }
    }
    let (wf, hf) = (w as f32, h as f32);
    let bmp = c.finish(move |x, y| cover(rrect(x, y, 1.0, 1.0, wf - 1.0, hf - 1.0, hf * 0.45)));
    Gfx { bmp, w, h }
}

// ---------------------------------------------------------------- app side

impl App {
    fn say_life(&mut self, pet: u32, section: &str) { if self.settings.life.talk { self.say_always(pet, section); } }

    /// Little floating symbol above a pet.
    pub fn emote(&mut self, pet: u32, e: Emote) {
        if !self.settings.life.emotes || self.life.particles.len() > 40 { return; }
        let Some(i) = self.pet_index(pet) else { return };
        let w = &self.pets[i].w;
        if !w.shown { return; }
        let x = (w.pos.0 + w.size.0 / 2) as f64 + (random() - 0.5) * 20.0;
        let y = w.pos.1 as f64 + w.size.1 as f64 * 0.15;
        self.particle(x, y, e);
    }

    fn particle(&mut self, x: f64, y: f64, e: Emote) {
        let s = self.ui_scale();
        let ov = Overlay::new(emote_gfx(e, s), true, 0);
        let (vx, vy, life) = match e {
            Emote::Crumb => ((random() - 0.5) * 3.0, -2.0 - random() * 2.0, 30),
            Emote::Zzz => (0.4 + random() * 0.3, -0.7, 70),
            _ => ((random() - 0.5) * 0.8, -1.0, 50),
        };
        self.life.particles.push(Particle { ov, x, y, vx: vx * s, vy: vy * s, age: 0, life });
        self.pranks.restack = true;
    }

    fn pet_free(&self, i: usize) -> bool {
        let p = &self.pets[i];
        p.w.shown && !p.m.st.dragging && !p.m.holding && !self.pranks.busy(p.m.id) && !self.life.busy(p.m.id)
    }

    fn work_of(&self, i: usize) -> Rect { self.world.monitors[self.pets[i].m.st.monitor.min(self.world.monitors.len() - 1)].work }

    fn on_work_floor(&self, i: usize) -> bool { self.pets[i].m.st.anchor.1 == self.work_of(i).b }

    /// Every tick before pets move: heads as platforms and what each pet offers others.
    pub fn life_before(&mut self) {
        let mut heads = Vec::new();
        let mut infos = Vec::new();
        for i in 0..self.pets.len() {
            let (id, pack, frame, pos, dragging, shown) = {
                let p = &self.pets[i];
                (p.m.id, p.m.pack, p.m.st.frame.as_ref().map(|f| (f.img, f.flip)), p.w.pos, p.m.st.dragging, p.w.shown)
            };
            let aff = self.with_pet(i, |m, _, env, _, _| m.affordance(env));
            infos.push(PetInfo { id, anchor: self.pets[i].m.st.anchor, affordance: aff });
            if !self.settings.life.stacking || dragging || !shown || self.life.hidden.contains_key(&id) { continue; }
            let Some((img, flip)) = frame else { continue };
            let Some(b) = self.bmp(pack, img) else { continue };
            let (bw, (l, t, r, _)) = (b.w, b.opaque);
            let (l, r) = if flip { (bw - r, bw - l) } else { (l, r) };
            let cx = pos.0 + (l + r) / 2;
            let half = ((r - l) / 4).max(6);
            let top = pos.1 + t + (2.0 * self.scale) as i32;
            let rect = Rect { l: cx - half, t: top, r: cx + half, b: top + 3 };
            let prev = self.life.heads_prev.get(&id).copied().unwrap_or(rect);
            heads.push(Win { hwnd: -(id as isize), rect, prev });
        }
        self.life.heads_prev = heads.iter().map(|w| ((-w.hwnd) as u32, w.rect)).collect();
        self.world.heads = heads;
        self.world.pets = infos;
    }

    /// Every tick after pets moved.
    pub fn life_after(&mut self) {
        if self.settings.debug {
            let t = self.tick_n;
            if t == 200 { self.drop_toy(ToyKind::Cookie); }
            if t == 700 { self.drop_toy(ToyKind::Balloon); }
            if t % 400 == 0 { let st: Vec<String> = self.world.heads.iter().map(|w| format!("{}@{},{}", -w.hwnd, w.rect.l, w.rect.t)).collect(); self.dbg(&format!("heads {st:?} stood-on {:?}", self.pets.iter().filter(|p| p.m.st.ie.map(|h| h < 0).unwrap_or(false)).map(|p| p.m.id).collect::<Vec<_>>())); }
        }
        self.step_particles();
        self.step_toys();
        self.step_to_bed();
        let t = self.tick_n;
        let l = self.settings.life.clone();
        if l.sleep && t % 40 == 7 { self.step_sleep(); }
        if self.life.away { return; }
        if l.daytime && t % 400 == 13 { self.step_daytime(); }
        if l.meet { self.step_meet(); }
        if l.games { self.step_game(); }
        if l.hide && t % 10 == 3 { self.step_hide(); }
        if l.push && self.settings.throw_windows { self.step_push(); }
        if t % 900 == 450 && self.life.has_bed() && random() < 0.4 { self.nap_in_bed(); }
        if l.stacking && t % 500 == 250 && random() < 0.5 { self.piggyback(); }
    }

    fn step_particles(&mut self) {
        let painter = &self.painter;
        self.life.particles.retain_mut(|p| {
            p.age += 1;
            if p.age >= p.life { unsafe { windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(p.ov.hwnd); windows_sys::Win32::Graphics::Gdi::DeleteObject(p.ov.gfx.bmp); } return false; }
            p.x += p.vx;
            p.y += p.vy;
            p.vx *= 0.98;
            let fade = (1.0 - (p.age as f64 / p.life as f64).powi(2)).clamp(0.0, 1.0);
            p.ov.show(painter, p.x as i32 - p.ov.gfx.w / 2, p.y as i32 - p.ov.gfx.h / 2, (fade * 255.0) as u8);
            true
        });
        let m: Vec<u32> = self.life.meeting.keys().copied().collect();
        for id in m {
            let left = self.life.meeting.get_mut(&id).unwrap();
            *left = left.saturating_sub(1);
            if *left == 0 { self.life.meeting.remove(&id); }
        }
    }

    // ---------------------------------------------------------------- sleep / time of day

    fn go_sleep(&mut self, i: usize, ticks: u32, tag: &str) -> bool {
        let d = ticks.to_string();
        self.run_on(i, &SLEEP_ACTS, &[("Duration", &d)], tag)
    }

    fn step_sleep(&mut self) {
        let mins = self.settings.life.sleep_minutes.max(1) as u64;
        let idle = win::idle_ms() as u64;
        let away = idle > mins * 60_000;
        if away {
            if !self.life.away { self.life.away = true; self.life.game = None; }
            for i in 0..self.pets.len() {
                let id = self.pets[i].m.id;
                let p = &self.pets[i];
                if p.m.st.dragging || !p.w.shown || !self.on_work_floor(i) { continue; }
                if self.life.asleep.contains(&id) && p.m.beh == "#life-sleep" {
                    if random() < 0.5 { self.emote(id, Emote::Zzz); }
                    continue;
                }
                // walk to the bed first if there is one
                if let Some(bx) = self.life.toys.iter().find(|t| t.kind == ToyKind::Bed && !t.dragging).map(|t| t.x as i32) {
                    if (self.pets[i].m.st.anchor.0 - bx).abs() > 30 {
                        if !self.life.to_bed.contains(&id) {
                            let tx = bx.to_string();
                            if self.run_on(i, &ACT_WALK, &[("TargetX", &tx)], "#life-tobed") { self.life.to_bed.insert(id); }
                        }
                        continue;
                    }
                }
                if self.go_sleep(i, 10_000_000, "#life-sleep") {
                    if self.life.asleep.insert(id) { self.say_life(id, "sleep"); }
                }
            }
        } else if self.life.away && idle < 3_000 {
            self.life.away = false;
            let sleepers: Vec<u32> = self.life.asleep.drain().collect();
            for id in sleepers {
                let Some(i) = self.pet_index(id) else { continue };
                self.emote(id, Emote::Bang);
                self.say_life(id, "wake");
                let pack = &self.packs[self.pets[i].m.pack];
                let b = if pack.behavior("StandUp").is_some() { "StandUp" } else { "Fall" };
                self.set_behavior(i, b);
            }
        }
    }

    fn step_daytime(&mut self) {
        let (hour, day) = win::local_time();
        let slot = match hour { 6..=10 => "morning", 13 => "lunch", 23 | 0 | 1 => "night", _ => return };
        let key = format!("{day}-{slot}");
        if self.life.said.contains(&key) { return; }
        let free: Vec<usize> = (0..self.pets.len()).filter(|&i| self.pet_free(i)).collect();
        let Some(i) = pick(&free) else { return };
        let id = self.pets[i].m.id;
        self.life.said.insert(key);
        self.say_life(id, slot);
        self.emote(id, match slot { "morning" => Emote::Star, "lunch" => Emote::Note, _ => Emote::Zzz });
        if slot == "night" && self.on_work_floor(i) { self.go_sleep(i, 800, "#life-nap"); }
    }

    // ---------------------------------------------------------------- meeting

    fn step_meet(&mut self) {
        if self.tick_n % 20 != 5 || self.tick_n < self.life.next_meet || !self.life.meeting.is_empty() { return; }
        let s = self.scale;
        let free: Vec<usize> = (0..self.pets.len()).filter(|&i| self.pet_free(i)).collect();
        for (k, &i) in free.iter().enumerate() {
            for &j in &free[k + 1..] {
                let (a, b) = (self.pets[i].m.st.anchor, self.pets[j].m.st.anchor);
                let dx = (a.0 - b.0).abs() as f64;
                if a.1 != b.1 || dx < 30.0 * s || dx > 150.0 * s { continue; }
                let (ia, ib) = (self.pets[i].m.id, self.pets[j].m.id);
                let key = (ia.min(ib), ia.max(ib));
                if self.life.meet_cool.get(&key).map(|t| self.tick_n < *t).unwrap_or(false) || random() > 0.35 { continue; }
                self.life.meet_cool.insert(key, self.tick_n + 2400);
                self.life.next_meet = self.tick_n + 800 + (random() * 600.0) as u64;
                self.dbg(&format!("meet {ia} & {ib}"));
                // face each other and stop for a chat
                self.pets[i].m.st.look_right = a.0 < b.0;
                self.pets[j].m.st.look_right = b.0 < a.0;
                let acts = if random() < 0.5 { ["Sit", "Stand"] } else { ["Stand", "Sit"] };
                self.run_on(i, &acts, &[("Duration", "140")], "#life-meet");
                self.run_on(j, &acts, &[("Duration", "140")], "#life-meet");
                self.life.meeting.insert(ia, 140);
                self.life.meeting.insert(ib, 140);
                self.say_life(ia, "meet");
                let e = if random() < 0.5 { Emote::Heart } else { Emote::Star };
                let mid = ((a.0 + b.0) / 2) as f64;
                let top = self.pets[i].w.pos.1.min(self.pets[j].w.pos.1) as f64 + 10.0;
                self.particle(mid, top, e);
                self.particle(mid + 10.0, top + 8.0, e);
                return;
            }
        }
    }

    // ---------------------------------------------------------------- games

    fn step_game(&mut self) {
        let s = self.scale;
        let Some(game) = self.life.game.take() else {
            if self.tick_n % 300 != 150 || random() > 0.3 { return; }
            // pets standing on the same screen bottom
            let free: Vec<usize> = (0..self.pets.len()).filter(|&i| self.pet_free(i) && self.on_work_floor(i)).collect();
            if free.len() < 2 { return; }
            let i = free[(random() * free.len() as f64) as usize % free.len()];
            let same: Vec<usize> = free.iter().copied().filter(|&j| self.pets[j].m.st.monitor == self.pets[i].m.st.monitor).collect();
            if same.len() < 2 { return; }
            if same.len() >= 3 && random() < 0.5 {
                let leader = self.pets[i].m.id;
                let followers: Vec<u32> = same.iter().filter(|&&j| j != i).map(|&j| self.pets[j].m.id).collect();
                let w = self.work_of(i);
                let dest = w.l + 80 + (random() * (w.w() - 160).max(1) as f64) as i32;
                let ds = dest.to_string();
                self.run_on(i, &ACT_WALK, &[("TargetX", &ds)], "#life-follow");
                self.say_life(leader, "follow");
                self.dbg(&format!("follow the leader {leader} -> {dest}"));
                self.life.game = Some(Game::Follow { leader, dest, followers, ticks: 0 });
            } else {
                let j = *same.iter().find(|&&j| j != i).unwrap();
                let (chaser, runner) = (self.pets[i].m.id, self.pets[j].m.id);
                self.say_life(chaser, "tag");
                self.dbg(&format!("tag {chaser} chases {runner}"));
                self.life.game = Some(Game::Tag { chaser, runner, ticks: 0, swaps: 0 });
            }
            return;
        };
        let alive = |app: &App, id: u32| app.pet_index(id).filter(|&i| !app.pets[i].m.st.dragging && !app.pets[i].m.holding);
        match game {
            Game::Tag { chaser, runner, ticks, swaps } => {
                let (Some(c), Some(r)) = (alive(self, chaser), alive(self, runner)) else { return };
                if ticks > 1000 || swaps >= 4 { return; }
                let (cx, rx) = (self.pets[c].m.st.anchor.0, self.pets[r].m.st.anchor.0);
                if (cx - rx).abs() < (30.0 * s) as i32 {
                    // tagged: swap roles
                    self.emote(runner, Emote::Star);
                    self.say_life(chaser, "tag");
                    self.life.game = Some(Game::Tag { chaser: runner, runner: chaser, ticks: ticks + 1, swaps: swaps + 1 });
                    return;
                }
                if ticks % 10 == 0 {
                    let w = self.work_of(r);
                    let away = if cx < rx { w.r - 60 } else { w.l + 60 };
                    let (rs, cs) = (away.to_string(), rx.to_string());
                    if self.pets[r].m.beh != "#life-tag" || ticks % 60 == 0 { self.run_on(r, &ACT_RUN, &[("TargetX", &rs)], "#life-tag"); }
                    self.run_on(c, &ACT_RUN, &[("TargetX", &cs)], "#life-tag");
                }
                self.life.game = Some(Game::Tag { chaser, runner, ticks: ticks + 1, swaps });
            }
            Game::Follow { leader, dest, followers, ticks } => {
                let Some(li) = alive(self, leader) else { return };
                let lx = self.pets[li].m.st.anchor.0;
                if ticks > 900 || (lx - dest).abs() < 4 {
                    self.emote(leader, Emote::Note);
                    return;
                }
                if self.pets[li].m.beh != "#life-follow" {
                    let ds = dest.to_string();
                    self.run_on(li, &ACT_WALK, &[("TargetX", &ds)], "#life-follow");
                }
                if ticks % 12 == 0 {
                    let dir = if dest > lx { -1 } else { 1 };
                    for (k, &f) in followers.iter().enumerate() {
                        let Some(fi) = alive(self, f) else { continue };
                        let tx = lx + dir * ((k as f64 + 1.0) * 70.0 * s) as i32;
                        let ts = tx.to_string();
                        if (self.pets[fi].m.st.anchor.0 - tx).abs() > 6 { self.run_on(fi, &ACT_WALK, &[("TargetX", &ts)], "#life-follow"); }
                    }
                }
                self.life.game = Some(Game::Follow { leader, dest, followers, ticks: ticks + 1 });
            }
        }
    }

    // ---------------------------------------------------------------- windows

    /// Which side of its window a pet clings to, if any.
    fn window_wall(&mut self, i: usize) -> Option<(isize, Side)> {
        let h = self.pets[i].m.st.ie.filter(|h| *h > 0)?;
        match self.with_pet(i, |m, _, env, _, _| env.wall(m.st.anchor, m.st.look_right)) {
            BorderRef::Ie(s) => Some((h, s)),
            _ => None,
        }
    }

    fn step_hide(&mut self) {
        // pets that left the window come back on top
        let hidden: Vec<(u32, isize)> = self.life.hidden.iter().map(|(a, b)| (*a, *b)).collect();
        for (id, h) in hidden {
            let still = self.pet_index(id).and_then(|i| self.window_wall(i)).map(|(w, _)| w == h).unwrap_or(false);
            if !still { self.life.hidden.remove(&id); self.pranks.restack = true; }
        }
        if self.tick_n % 100 != 3 { return; }
        for i in 0..self.pets.len() {
            let id = self.pets[i].m.id;
            if self.life.hidden.contains_key(&id) || self.pets[i].m.st.dragging { continue; }
            if let Some((h, _)) = self.window_wall(i) {
                if random() < 0.4 {
                    self.life.hidden.insert(id, h);
                    self.dbg(&format!("pet {id} hides behind window {h}"));
                    win::place_behind(self.pets[i].w.hwnd, h);
                }
            }
        }
    }

    fn step_push(&mut self) {
        if let Some(mut p) = self.life.push.take() {
            let Some(i) = self.pet_index(p.pet) else { return };
            let on = self.window_wall(i).map(|(h, _)| h == p.hwnd).unwrap_or(false);
            let Some(r) = self.world.win(p.hwnd).map(|z| self.world.wins[z].rect) else { return };
            if !on || p.ticks > 70 { return; }
            p.ticks += 1;
            let step = (2.0 * self.scale).round() as i32;
            self.apply(vec![(p.pet, Out::MoveWindow { hwnd: p.hwnd, l: r.l + p.dir * step, t: r.t })]);
            self.life.push = Some(p);
            return;
        }
        if self.tick_n % 200 != 77 || random() > 0.35 { return; }
        for i in 0..self.pets.len() {
            if !self.pet_free(i) { continue; }
            if let Some((h, side)) = self.window_wall(i) {
                let id = self.pets[i].m.id;
                // holding the window's left edge pushes it right, and the other way round
                let dir = if side == Side::Left { 1 } else { -1 };
                self.say_life(id, "push");
                self.dbg(&format!("pet {id} pushes window {h}"));
                self.life.push = Some(Push { pet: id, hwnd: h, dir, ticks: 0 });
                return;
            }
        }
    }

    // ---------------------------------------------------------------- toys

    pub fn drop_toy(&mut self, kind: ToyKind) {
        let Some(m) = self.world.monitors.first() else { return };
        let s = self.ui_scale();
        let w = m.work;
        let x = w.l as f64 + 150.0 + random() * (w.w() as f64 - 300.0).max(1.0);
        let (gfx, y) = match kind {
            ToyKind::Cookie => (cookie_gfx(s, 0), w.t as f64 + 60.0),
            ToyKind::Balloon => (balloon_gfx(s), w.b as f64 - 40.0 * s),
            ToyKind::Bed => {
                if let Some(i) = self.life.toys.iter().position(|t| t.kind == ToyKind::Bed) { self.life.toys.remove(i).ov.close(); return; }
                (bed_gfx(s), w.t as f64 + 60.0)
            }
        };
        let ov = Overlay::new(gfx, false, 0);
        self.life.toys.push(Toy { kind, ov, x, y, vy: 0.0, base_x: x, dragging: false, bites: 0, eater: None, timer: 0 });
        self.pranks.restack = true;
    }

    pub fn clear_toys(&mut self) { for t in self.life.toys.drain(..) { t.ov.close(); } }

    /// Mouse down on a toy: pick up a cookie/bed, pop a balloon.
    pub fn toy_press(&mut self, h: isize) -> bool {
        let Some(i) = self.life.toys.iter().position(|t| t.ov.hwnd as isize == h) else { return false };
        if self.life.toys[i].kind == ToyKind::Balloon {
            let t = self.life.toys.remove(i);
            for _ in 0..4 { self.particle(t.x, t.y, Emote::Star); }
            t.ov.close();
            return true;
        }
        let t = &mut self.life.toys[i];
        t.dragging = true;
        t.eater = None;
        win::capture(t.ov.hwnd);
        true
    }

    pub fn toy_release(&mut self, h: isize) -> bool {
        let Some(t) = self.life.toys.iter_mut().find(|t| t.ov.hwnd as isize == h) else { return false };
        if t.dragging { t.dragging = false; t.vy = 0.0; win::release_capture(); }
        true
    }

    pub fn remove_toy(&mut self, h: isize) -> bool {
        let Some(i) = self.life.toys.iter().position(|t| t.ov.hwnd as isize == h) else { return false };
        self.life.toys.remove(i).ov.close();
        true
    }

    fn step_toys(&mut self) {
        let s = self.ui_scale();
        let mut toys = std::mem::take(&mut self.life.toys);
        let mut keep = Vec::new();
        for mut t in toys.drain(..) {
            let mon = self.world.monitor_for(0, t.x as i32, t.y as i32);
            let work = self.world.monitors[mon].work;
            match t.kind {
                ToyKind::Balloon => {
                    t.timer += 1;
                    t.y -= 0.7 * s;
                    t.x = t.base_x + (t.timer as f64 / 25.0).sin() * 12.0 * s;
                    if t.y < work.t as f64 - 60.0 { t.ov.close(); continue; }
                    if self.balloon_step(&t, work) {
                        for _ in 0..5 { self.particle(t.x, t.y, Emote::Star); }
                        t.ov.close();
                        continue;
                    }
                    t.ov.show(&self.painter, t.x as i32 - t.ov.gfx.w / 2, t.y as i32 - (t.ov.gfx.h as f64 * 0.35) as i32, 255);
                }
                ToyKind::Cookie | ToyKind::Bed => {
                    if t.dragging {
                        t.x = self.world.cursor.0 as f64;
                        t.y = self.world.cursor.1 as f64 + t.ov.gfx.h as f64 / 2.0;
                    } else if t.y < work.b as f64 {
                        t.vy += 0.9 * s;
                        t.y = (t.y + t.vy).min(work.b as f64);
                        if t.y >= work.b as f64 { t.vy = 0.0; }
                    }
                    if t.kind == ToyKind::Cookie && !t.dragging && t.y >= work.b as f64 && self.cookie_step(&mut t, work) {
                        t.ov.close();
                        continue;
                    }
                    t.ov.show(&self.painter, t.x as i32 - t.ov.gfx.w / 2, t.y as i32 - t.ov.gfx.h, 255);
                }
            }
            keep.push(t);
        }
        keep.extend(self.life.toys.drain(..));
        self.life.toys = keep;
    }

    /// Pets run to the cookie; the first one there eats it in three bites. True when it's gone.
    fn cookie_step(&mut self, t: &mut Toy, work: Rect) -> bool {
        let s = self.ui_scale();
        if let Some(e) = t.eater {
            let Some(i) = self.pet_index(e).filter(|&i| !self.pets[i].m.st.dragging) else { t.eater = None; return false };
            t.timer += 1;
            if t.timer % 40 == 0 {
                t.bites += 1;
                for _ in 0..3 { self.particle(t.x, t.y - t.ov.gfx.h as f64 / 2.0, Emote::Crumb); }
                if t.bites >= 3 {
                    self.emote(e, Emote::Heart);
                    let _ = i;
                    return true;
                }
                let old = std::mem::replace(&mut t.ov, Overlay::new(cookie_gfx(s, t.bites), false, 0));
                old.close();
                self.pranks.restack = true;
            }
            return false;
        }
        if self.tick_n % 15 != 0 { return false; }
        let mut nearest: Option<(usize, i32)> = None;
        for i in 0..self.pets.len() {
            if !self.pet_free(i) && self.pets[i].m.beh != "#life-food" { continue; }
            if self.pets[i].m.st.anchor.1 != work.b || self.pets[i].m.st.dragging { continue; }
            let d = (self.pets[i].m.st.anchor.0 - t.x as i32).abs();
            if d < (20.0 * s) as i32 {
                let id = self.pets[i].m.id;
                t.eater = Some(id);
                self.dbg(&format!("pet {id} eats the cookie"));
                t.timer = 0;
                self.pets[i].m.st.look_right = self.pets[i].m.st.anchor.0 < t.x as i32;
                self.run_on(i, &["Sit", "Stand"], &[("Duration", "130")], "#life-eat");
                self.say_life(id, "food");
                return false;
            }
            if nearest.map(|n| d < n.1).unwrap_or(true) { nearest = Some((i, d)); }
        }
        if let Some((i, _)) = nearest {
            let tx = (t.x as i32).to_string();
            self.run_on(i, &ACT_RUN, &[("TargetX", &tx)], "#life-food");
        }
        false
    }

    /// Pets under a low balloon jump for it. True when one catches (pops) it.
    fn balloon_step(&mut self, t: &Toy, work: Rect) -> bool {
        let s = self.ui_scale();
        for i in 0..self.pets.len() {
            let w = &self.pets[i].w;
            if !w.shown { continue; }
            let r = Rect { l: w.pos.0, t: w.pos.1, r: w.pos.0 + w.size.0, b: w.pos.1 + w.size.1 };
            if r.contains(t.x as i32, t.y as i32) && !self.pets[i].m.st.dragging {
                let id = self.pets[i].m.id;
                self.dbg(&format!("pet {id} popped the balloon"));
                self.say_life(id, "balloon");
                return true;
            }
        }
        if self.tick_n % 25 == 0 && t.y > work.b as f64 - 330.0 * s {
            let cand: Vec<usize> = (0..self.pets.len())
                .filter(|&i| self.pet_free(i) && self.on_work_floor(i) && (self.pets[i].m.st.anchor.0 as f64 - t.x).abs() < 220.0 * s)
                .collect();
            if let Some(i) = pick(&cand) {
                let (tx, ty) = ((t.x as i32).to_string(), ((t.y + 50.0 * s) as i32).to_string());
                self.run_on(i, &["Jumping"], &[("TargetX", &tx), ("TargetY", &ty)], "#life-balloon");
            }
        }
        false
    }

    /// Ball fetch: a pet runs to the resting ball, carries it to the cursor and drops it there.
    /// Returns true while fetch controls the ball.
    pub fn fetch_step(&mut self, b: &mut crate::pranks::Ball, work: Rect, on_floor: bool) -> bool {
        let s = self.ui_scale();
        match b.carrier {
            None => {
                if !(on_floor && b.vy.abs() < 0.5 && b.vx.abs() < 1.0) { return false; }
                let cand: Vec<usize> = (0..self.pets.len()).filter(|&i| self.pet_free(i) && self.pets[i].m.st.anchor.1 == work.b).collect();
                let Some(&i) = cand.iter().min_by_key(|&&i| (self.pets[i].m.st.anchor.0 - b.x as i32).abs()) else { b.fetch = false; return false };
                b.carrier = Some(self.pets[i].m.id);
                let tx = (b.x as i32).to_string();
                self.run_on(i, &ACT_RUN, &[("TargetX", &tx)], "#life-fetch");
                true
            }
            Some(id) => {
                let Some(i) = self.pet_index(id).filter(|&i| !self.pets[i].m.st.dragging) else { b.fetch = false; b.carrier = None; b.carrying = false; return false };
                let a = self.pets[i].m.st.anchor;
                if !b.carrying {
                    if (a.0 - b.x as i32).abs() < (b.r + 20.0 * s) as i32 {
                        b.carrying = true;
                        self.say_life(id, "fetch");
                    } else if self.pets[i].m.beh != "#life-fetch" {
                        let tx = (b.x as i32).to_string();
                        self.run_on(i, &ACT_RUN, &[("TargetX", &tx)], "#life-fetch");
                    }
                    return true;
                }
                // carry it on the head to below the cursor
                let w = &self.pets[i].w;
                b.x = (w.pos.0 + w.size.0 / 2) as f64;
                b.y = w.pos.1 as f64 + w.size.1 as f64 * 0.2 - b.r;
                b.vx = 0.0;
                b.vy = 0.0;
                let goal = self.world.cursor.0.clamp(work.l + 40, work.r - 40);
                if (a.0 - goal).abs() < (30.0 * s) as i32 {
                    b.fetch = false;
                    b.carrier = None;
                    b.carrying = false;
                    b.vy = -6.0 * s;
                    self.emote(id, Emote::Heart);
                    return true;
                }
                if self.pets[i].m.beh != "#life-fetch" || self.tick_n % 20 == 0 {
                    let tx = goal.to_string();
                    self.run_on(i, &ACT_RUN, &[("TargetX", &tx)], "#life-fetch");
                }
                true
            }
        }
    }

    /// A pet hops onto a nearby friend's head (then stands there until it walks off or the friend moves away).
    fn piggyback(&mut self) {
        let s = self.scale;
        let free: Vec<usize> = (0..self.pets.len()).filter(|&i| self.pet_free(i) && self.on_work_floor(i)).collect();
        for &i in &free {
            let a = self.pets[i].m.st.anchor;
            let me = self.pets[i].m.id;
            let Some(head) = self.world.heads.iter().find(|w| {
                let other = (-w.hwnd) as u32;
                other != me && self.pet_index(other).map(|j| self.pets[j].m.st.anchor.1 == a.1).unwrap_or(false)
                    && ((w.rect.l + w.rect.r) / 2 - a.0).abs() < (220.0 * s) as i32 && ((w.rect.l + w.rect.r) / 2 - a.0).abs() > (30.0 * s) as i32
            }) else { continue };
            let (tx, ty) = (((head.rect.l + head.rect.r) / 2).to_string(), head.rect.t.to_string());
            let who = -head.hwnd;
            if self.run_on(i, &["Jumping"], &[("TargetX", &tx), ("TargetY", &ty)], "#life-hop") {
                self.dbg(&format!("pet {me} hops onto pet {who}"));
                return;
            }
        }
    }

    /// Every now and then a pet takes a nap in the bed.
    fn nap_in_bed(&mut self) {
        let Some(bx) = self.life.toys.iter().find(|t| t.kind == ToyKind::Bed && !t.dragging).map(|t| t.x as i32) else { return };
        let free: Vec<usize> = (0..self.pets.len()).filter(|&i| self.pet_free(i) && self.on_work_floor(i)).collect();
        let Some(i) = pick(&free) else { return };
        if (self.pets[i].m.st.anchor.0 - bx).abs() < 20 {
            self.go_sleep(i, 500, "#life-nap");
            let id = self.pets[i].m.id;
            self.emote(id, Emote::Zzz);
        } else {
            let tx = bx.to_string();
            let id = self.pets[i].m.id;
            if self.run_on(i, &ACT_WALK, &[("TargetX", &tx)], "#life-tobed") { self.life.to_bed.insert(id); }
        }
    }

    /// Pets that reach the bed lie down (for good while you're away, for a nap otherwise).
    fn step_to_bed(&mut self) {
        if self.life.to_bed.is_empty() { return; }
        let bx = self.life.toys.iter().find(|t| t.kind == ToyKind::Bed && !t.dragging).map(|t| t.x as i32);
        let walkers: Vec<u32> = self.life.to_bed.iter().copied().collect();
        for id in walkers {
            let (Some(i), Some(bx)) = (self.pet_index(id), bx) else { self.life.to_bed.remove(&id); continue };
            let at = (self.pets[i].m.st.anchor.0 - bx).abs() <= 6;
            if at {
                self.life.to_bed.remove(&id);
                if self.life.away {
                    if self.go_sleep(i, 10_000_000, "#life-sleep") { self.life.asleep.insert(id); self.say_life(id, "sleep"); }
                } else {
                    self.go_sleep(i, 500, "#life-nap");
                }
                self.emote(id, Emote::Zzz);
            } else if self.pets[i].m.beh != "#life-tobed" || self.pets[i].m.st.dragging {
                self.life.to_bed.remove(&id);
            }
        }
    }
}
