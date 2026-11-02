//! The mascot's view of the desktop (work area, interactive window, cursor) and the
//! `mascot.*` script scope that Shimeji configs query.

use crate::config::{Param, ParamSrc, Params};
use crate::expr::{self, Scope, Val};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Rect { pub l: i32, pub t: i32, pub r: i32, pub b: i32 }

impl Rect {
    pub fn w(&self) -> i32 { self.r - self.l }
    pub fn h(&self) -> i32 { self.b - self.t }
    pub fn contains(&self, x: i32, y: i32) -> bool { x >= self.l && x < self.r && y >= self.t && y < self.b }
    pub fn intersects(&self, o: &Rect) -> bool { self.l < o.r && o.l < self.r && self.t < o.b && o.t < self.b }
}

pub struct Monitor {
    pub screen: Rect,
    pub work: Rect,
    /// Edge shared with another monitor (left, top, right, bottom): not a wall/floor.
    pub open: [bool; 4],
}

pub struct Win {
    pub hwnd: isize,
    pub rect: Rect,
    pub prev: Rect,
}

/// What other pets can see of a pet: where it is and what it offers (Shimeji-ee affordances).
pub struct PetInfo {
    pub id: u32,
    pub anchor: (i32, i32),
    pub affordance: Option<String>,
}

#[derive(Default)]
pub struct World {
    pub monitors: Vec<Monitor>,
    /// Interactive windows, topmost first.
    pub wins: Vec<Win>,
    /// Pets' heads as tiny platforms other pets can stand on (hwnd = -pet id).
    pub heads: Vec<Win>,
    pub pets: Vec<PetInfo>,
    pub cursor: (i32, i32),
    pub cursor_d: (i32, i32),
    pub total: usize,
}

impl World {
    pub fn win(&self, hwnd: isize) -> Option<usize> { self.wins.iter().position(|w| w.hwnd == hwnd) }
    pub fn covered(&self, z: usize, x: i32, y: i32) -> bool { self.wins[..z].iter().any(|w| w.rect.contains(x, y)) }

    pub fn monitor_for(&self, cur: usize, x: i32, y: i32) -> usize {
        if let Some(m) = self.monitors.get(cur) {
            if x >= m.screen.l && x <= m.screen.r && y >= m.screen.t - 1024 && y <= m.screen.b { return cur; }
        }
        if let Some(i) = self.monitors.iter().position(|m| m.screen.contains(x, y)) { return i; }
        let dist = |m: &Monitor| {
            let dx = (m.screen.l - x).max(0).max(x - m.screen.r);
            let dy = (m.screen.t - y).max(0).max(y - m.screen.b);
            dx as i64 * dx as i64 + dy as i64 * dy as i64
        };
        (0..self.monitors.len()).min_by_key(|&i| dist(&self.monitors[i])).unwrap_or(0)
    }

    /// Topmost interactive window on the given monitor.
    pub fn active_for(&self, mon: usize) -> Option<isize> {
        let work = self.monitors.get(mon).map(|m| m.work).unwrap_or_default();
        self.wins.iter().find(|w| w.rect.intersects(&work)).or(self.wins.first()).map(|w| w.hwnd)
    }
}

#[derive(Clone, Copy, Default)]
pub struct Area {
    pub r: Rect,
    pub d: Rect,
    pub visible: bool,
    pub open: [bool; 4],
    pub z: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Side { Left, Top, Right, Bottom }

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BorderRef { None, NotOn, Work(Side), Ie(Side) }

pub struct EnvView<'a> {
    pub world: &'a World,
    pub work: Area,
    pub screen: Area,
    pub ie: Area,
    pub count: usize,
    /// The pet this view belongs to (so it doesn't stand on its own head).
    pub self_id: u32,
    ie_h: Option<isize>,
}

impl<'a> EnvView<'a> {
    pub fn new(world: &'a World, mon: usize, ie: Option<isize>, count: usize) -> Self {
        let m = &world.monitors[mon.min(world.monitors.len() - 1)];
        let work = Area { r: m.work, visible: true, open: m.open, ..Default::default() };
        let screen = Area { r: m.screen, visible: true, open: m.open, ..Default::default() };
        let mut v = EnvView { world, work, screen, ie: Area::default(), count, self_id: 0, ie_h: None };
        v.bind_ie(ie);
        v
    }

    pub fn bind_ie(&mut self, ie: Option<isize>) {
        self.ie_h = ie;
        // another pet's head
        if let Some(h) = ie.filter(|h| *h < 0 && *h != -(self.self_id as isize)) {
            if let Some(w) = self.world.heads.iter().find(|w| w.hwnd == h) {
                self.ie = Area {
                    r: w.rect,
                    d: Rect { l: w.rect.l - w.prev.l, t: w.rect.t - w.prev.t, r: w.rect.r - w.prev.r, b: w.rect.b - w.prev.b },
                    visible: true,
                    open: [false; 4],
                    z: None,
                };
                return;
            }
        }
        self.ie = match ie.and_then(|h| self.world.win(h)) {
            Some(z) => {
                let w = &self.world.wins[z];
                Area {
                    r: w.rect,
                    d: Rect { l: w.rect.l - w.prev.l, t: w.rect.t - w.prev.t, r: w.rect.r - w.prev.r, b: w.rect.b - w.prev.b },
                    visible: true,
                    open: [false; 4],
                    z: Some(z),
                }
            }
            None => Area::default(),
        };
    }

    pub fn ie_hwnd(&self) -> Option<isize> { if self.ie.visible { self.ie_h } else { None } }

    pub fn on(&self, a: &Area, s: Side, (x, y): (i32, i32)) -> bool {
        if !a.visible { return false; }
        let r = a.r;
        let hit = match s {
            Side::Top => !a.open[1] && y == r.t && x >= r.l && x <= r.r,
            Side::Bottom => !a.open[3] && y == r.b && x >= r.l && x <= r.r,
            Side::Left => !a.open[0] && x == r.l && y >= r.t && y <= r.b,
            Side::Right => !a.open[2] && x == r.r && y >= r.t && y <= r.b,
        };
        hit && a.z.map(|z| !self.world.covered(z, x, y)).unwrap_or(true)
    }

    fn area(&self, b: BorderRef) -> Option<(&Area, Side)> {
        match b {
            BorderRef::Work(s) => Some((&self.work, s)),
            BorderRef::Ie(s) => Some((&self.ie, s)),
            _ => None,
        }
    }

    pub fn is_on(&self, b: BorderRef, p: (i32, i32)) -> bool {
        self.area(b).map(|(a, s)| self.on(a, s, p)).unwrap_or(false)
    }

    pub fn floor(&self, p: (i32, i32)) -> BorderRef {
        if self.on(&self.ie, Side::Top, p) { BorderRef::Ie(Side::Top) }
        else if self.on(&self.work, Side::Bottom, p) { BorderRef::Work(Side::Bottom) }
        else { BorderRef::NotOn }
    }
    pub fn ceiling(&self, p: (i32, i32)) -> BorderRef {
        if self.on(&self.ie, Side::Bottom, p) { BorderRef::Ie(Side::Bottom) }
        else if self.on(&self.work, Side::Top, p) { BorderRef::Work(Side::Top) }
        else { BorderRef::NotOn }
    }
    pub fn wall(&self, p: (i32, i32), look_right: bool) -> BorderRef {
        let (ws, is) = if look_right { (Side::Right, Side::Left) } else { (Side::Left, Side::Right) };
        if self.on(&self.work, ws, p) { BorderRef::Work(ws) }
        else if self.on(&self.ie, is, p) { BorderRef::Ie(is) }
        else { BorderRef::NotOn }
    }

    /// Floor check used while falling: the bound window, the work area, or any other
    /// visible window top (rebinding the mascot to it).
    pub fn land(&mut self, p: (i32, i32)) -> bool {
        if self.floor(p) != BorderRef::NotOn { return true; }
        let (x, y) = p;
        let world = self.world;
        let hit = world.wins.iter().enumerate().find(|(z, w)| {
            w.rect.t == y && x >= w.rect.l && x <= w.rect.r && !world.covered(*z, x, y)
        });
        if let Some((_, w)) = hit {
            self.bind_ie(Some(w.hwnd));
            return true;
        }
        let me = -(self.self_id as isize);
        if let Some(w) = world.heads.iter().find(|w| w.hwnd != me && w.rect.t == y && x >= w.rect.l && x <= w.rect.r) {
            self.bind_ie(Some(w.hwnd));
            return true;
        }
        false
    }

    /// Carry the point along with a moving border (Shimeji `Border.move`).
    pub fn border_move(&self, b: BorderRef, (x, y): (i32, i32)) -> (i32, i32) {
        let Some((a, s)) = self.area(b) else { return (x, y) };
        if !a.visible { return (x, y); }
        let (r, d) = (a.r, a.d);
        match s {
            Side::Top | Side::Bottom => {
                let mut old_w = (r.r - d.r) - (r.l - d.l);
                if old_w == 0 { old_w = 1; }
                let nx = ((x - (r.l - d.l)) as i64 * r.w() as i64 / old_w as i64) as i32 + r.l;
                let ny = y + if s == Side::Top { d.t } else { d.b };
                let (dx, dy) = (nx - x, ny - y);
                if dx.abs() >= 80 || dy > 20 || dy < -80 { (x, y) } else { (nx, ny) }
            }
            Side::Left | Side::Right => {
                let mut old_h = (r.b - d.b) - (r.t - d.t);
                if old_h == 0 { old_h = 1; }
                let ny = ((y - (r.t - d.t)) as i64 * r.h() as i64 / old_h as i64) as i32 + r.t;
                let nx = x + if s == Side::Left { d.l } else { d.r };
                if (nx - x).abs() >= 80 || (ny - y).abs() >= 80 { (x, y) } else { (nx, ny) }
            }
        }
    }
}

// ---------------------------------------------------------------- mascot state

pub struct Frame {
    pub img: usize,
    pub flip: bool,
    pub mirror_anchor: bool,
    pub ax: i32,
    pub ay: i32,
}

pub enum Out {
    Spawn { pack: Option<String>, anchor: (i32, i32), look_right: bool, behavior: String },
    Transform { pack: Option<String>, behavior: String },
    Remove,
    Sound(std::path::PathBuf),
    MoveWindow { hwnd: isize, l: i32, t: i32 },
    Shake(isize),
    /// Switch a pet (`None` = this one) to a behaviour, optionally turning to face x.
    SetBehavior { pet: Option<u32>, behavior: String, face_x: Option<i32> },
}

pub struct MState {
    pub anchor: (i32, i32),
    pub look_right: bool,
    pub time: i64,
    pub dragging: bool,
    pub ie: Option<isize>,
    pub monitor: usize,
    pub frame: Option<Frame>,
    pub pose_key: usize,
    pub out: Vec<Out>,
}

// ---------------------------------------------------------------- variables

pub struct VarMap {
    items: Vec<(Rc<str>, Rc<Param>, RefCell<Option<Val>>)>,
}

thread_local! { static DEPTH: Cell<u32> = const { Cell::new(0) }; }

impl VarMap {
    pub fn new(p: &Params) -> Self {
        VarMap { items: p.iter().map(|(k, v)| (k.clone(), v.clone(), RefCell::new(None))).collect() }
    }
    pub fn init(&self) { for (_, _, c) in &self.items { *c.borrow_mut() = None; } }
    pub fn init_frame(&self) {
        for (_, p, c) in &self.items {
            if let ParamSrc::Script { every_frame: true, .. } = p.src { *c.borrow_mut() = None; }
        }
    }
    pub fn set(&mut self, name: &str, v: Val) {
        let p = Rc::new(Param { src: ParamSrc::Lit(v) });
        match self.items.iter_mut().find(|(k, ..)| &**k == name) {
            Some(it) => { it.1 = p; *it.2.borrow_mut() = None; }
            None => self.items.push((name.into(), p, RefCell::new(None))),
        }
    }
    pub fn get(&self, name: &str, m: &MState, env: &EnvView) -> Option<Val> {
        let (_, p, cache) = self.items.iter().find(|(k, ..)| &**k == name)?;
        if let Some(v) = cache.borrow().as_ref() { return Some(v.clone()); }
        let v = match &p.src {
            ParamSrc::Lit(v) => v.clone(),
            ParamSrc::Script { expr: None, .. } => Val::Undef,
            ParamSrc::Script { expr: Some(e), .. } => {
                let d = DEPTH.with(|d| { d.set(d.get() + 1); d.get() });
                let v = if d > 32 { Val::Undef } else { expr::eval(e, &Ctx { m, env, vars: Some(self) }) };
                DEPTH.with(|d| d.set(d.get() - 1));
                v
            }
        };
        *cache.borrow_mut() = Some(v.clone());
        Some(v)
    }
}

pub fn eval_param(p: &Param, m: &MState, env: &EnvView, vars: Option<&VarMap>) -> Val {
    match &p.src {
        ParamSrc::Lit(v) => v.clone(),
        ParamSrc::Script { expr: Some(e), .. } => expr::eval(e, &Ctx { m, env, vars }),
        ParamSrc::Script { expr: None, .. } => Val::Undef,
    }
}

// ---------------------------------------------------------------- script scope

pub struct Ctx<'a, 'b> {
    pub m: &'a MState,
    pub env: &'a EnvView<'b>,
    pub vars: Option<&'a VarMap>,
}

fn area_field(env: &EnvView, a: &Area, f: &str) -> Val {
    let r = a.r;
    let _ = env;
    match f {
        "left" | "x" => Val::Num(r.l as f64),
        "top" | "y" => Val::Num(r.t as f64),
        "right" => Val::Num(r.r as f64),
        "bottom" => Val::Num(r.b as f64),
        "width" => Val::Num(r.w() as f64),
        "height" => Val::Num(r.h() as f64),
        "visible" => Val::Bool(a.visible),
        _ => Val::Undef,
    }
}

impl Ctx<'_, '_> {
    fn area(&self, n: &str) -> Option<&Area> {
        match n {
            "workArea" | "complementaryWorkArea" => Some(&self.env.work),
            "screen" => Some(&self.env.screen),
            "activeIE" | "ie" => Some(&self.env.ie),
            _ => None,
        }
    }
}

fn side(n: &str) -> Option<Side> {
    match n {
        "topBorder" => Some(Side::Top),
        "bottomBorder" => Some(Side::Bottom),
        "leftBorder" => Some(Side::Left),
        "rightBorder" => Some(Side::Right),
        _ => None,
    }
}

impl Scope for Ctx<'_, '_> {
    fn get(&self, p: &[String]) -> Val {
        let s: Vec<&str> = p.iter().map(|x| x.as_str()).collect();
        if s.len() == 1 {
            if let Some(v) = self.vars.and_then(|vm| vm.get(s[0], self.m, self.env)) { return v; }
            if s[0] == "mascot" { return Val::Obj(p.into()); }
            return Val::Undef;
        }
        let m = self.m;
        match s.as_slice() {
            ["mascot", "anchor"] => Val::Point(m.anchor.0, m.anchor.1),
            ["mascot", "anchor", "x"] => Val::Num(m.anchor.0 as f64),
            ["mascot", "anchor", "y"] => Val::Num(m.anchor.1 as f64),
            ["mascot", "lookRight"] => Val::Bool(m.look_right),
            ["mascot", "dragging"] => Val::Bool(m.dragging),
            ["mascot", "totalCount"] => Val::Num(self.env.world.total as f64),
            ["mascot", "count"] => Val::Num(self.env.count as f64),
            ["mascot", "environment", "cursor"] => Val::Point(self.env.world.cursor.0, self.env.world.cursor.1),
            ["mascot", "environment", "cursor", f] => {
                let w = self.env.world;
                Val::Num(match *f { "x" => w.cursor.0, "y" => w.cursor.1, "dx" => w.cursor_d.0, "dy" => w.cursor_d.1, _ => return Val::Undef } as f64)
            }
            ["mascot", "environment", a, f] => match (self.area(a), side(f)) {
                (Some(_), Some(_)) => Val::Obj(p.into()),
                (Some(ar), None) => area_field(self.env, ar, f),
                _ => Val::Undef,
            },
            ["mascot", "environment", ..] => Val::Obj(p.into()),
            _ => Val::Undef,
        }
    }

    fn call(&self, p: &[String], args: &[Val]) -> Val {
        let s: Vec<&str> = p.iter().map(|x| x.as_str()).collect();
        let pt = match args.first() {
            Some(Val::Point(x, y)) => (*x, *y),
            _ => self.m.anchor,
        };
        let env = self.env;
        match s.as_slice() {
            ["mascot", "environment", "floor", "isOn"] => Val::Bool(env.floor(pt) != BorderRef::NotOn),
            ["mascot", "environment", "ceiling", "isOn"] => Val::Bool(env.ceiling(pt) != BorderRef::NotOn),
            ["mascot", "environment", "wall", "isOn"] => Val::Bool(env.wall(pt, self.m.look_right) != BorderRef::NotOn),
            ["mascot", "environment", a, b, "isOn"] => match (self.area(a), side(b)) {
                (Some(ar), Some(sd)) => Val::Bool(env.on(ar, sd, pt)),
                _ => Val::Bool(false),
            },
            ["mascot", "environment", "isScreenTopBottom"] => {
                let r = env.screen.r;
                Val::Bool(pt.1 == r.t || pt.1 == r.b)
            }
            ["mascot", "environment", "isScreenLeftRight"] => {
                let r = env.screen.r;
                Val::Bool(pt.0 == r.l || pt.0 == r.r)
            }
            _ => Val::Undef,
        }
    }
}
