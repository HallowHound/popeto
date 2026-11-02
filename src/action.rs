//! Runtime action tree, mirroring Shimeji-ee's action classes
//! (Stay, Move, Animate, Sequence, Select and the embedded ones: Fall, Jump, Dragged, ...).

use crate::config::{merge, ActKind, ActionDef, Animation, BorderKind, Pack, Params};
use crate::env::{eval_param, BorderRef, EnvView, Frame, MState, Out, VarMap};
use crate::expr::{random, Val};
use std::rc::Rc;

pub struct Lost;

enum K {
    Stay,
    Move,
    /// Move that first plays its second animation (turning around) when it has to change direction.
    MoveTurn { turn_start: i64, turning: bool },
    Animate,
    Seq { ch: Vec<Act>, idx: usize, looping: bool },
    Select { ch: Vec<Act>, idx: usize },
    Look,
    Offset,
    Instant,
    Fall { vx: f64, vy: f64, mx: f64, my: f64, carry: bool },
    Jump,
    Dragged { foot_x: f64, foot_dx: f64, regist_at: i64 },
    Regist,
    WalkWithIe,
    ThrowIe { pos: (f64, f64), hwnd: Option<isize> },
    Breed,
    Transform,
    SelfDestruct,
    Turn,
    /// Shimeji-ee ScanMove: walk to a pet offering `Affordance`, then start behaviours on both.
    ScanMove { target: Option<u32> },
    /// Shimeji-ee Interact: play the animation, then switch to `Behavior`.
    Interact,
}

pub struct Act {
    def: Rc<ActionDef>,
    pub vars: VarMap,
    start: i64,
    border: BorderRef,
    scale: f64,
    k: K,
}

const SHAKE_SPEED: f64 = 18.0;

impl Act {
    pub fn build(def: &Rc<ActionDef>, params: &Params, pack: &Pack, log: &mut Vec<String>) -> Act {
        Self::build_d(def, params, pack, log, 0)
    }

    fn build_d(def: &Rc<ActionDef>, params: &Params, pack: &Pack, log: &mut Vec<String>, depth: u32) -> Act {
        if let ActKind::Reference(name) = &def.kind {
            let merged = merge(params, &def.params);
            return match pack.actions.get(name) {
                Some(t) if depth < 48 => Self::build_d(t, &merged, pack, log, depth + 1),
                _ => {
                    log.push(format!("{}: unknown action '{name}'", pack.name));
                    Self::stub(def, pack)
                }
            };
        }
        let vars = VarMap::new(&merge(&def.params, params));
        let kids = |log: &mut Vec<String>| def.children.iter().map(|c| Self::build_d(c, params, pack, log, depth + 1)).collect();
        let k = match &def.kind {
            ActKind::Stay => K::Stay,
            ActKind::Move => K::Move,
            ActKind::Animate => K::Animate,
            ActKind::Sequence => K::Seq { ch: kids(log), idx: 0, looping: false },
            ActKind::Select => K::Select { ch: kids(log), idx: 0 },
            ActKind::Embedded(c) => match c.as_str() {
                "look" => K::Look,
                "offset" => K::Offset,
                "fall" => K::Fall { vx: 0.0, vy: 0.0, mx: 0.0, my: 0.0, carry: false },
                "fallwithie" => K::Fall { vx: 0.0, vy: 0.0, mx: 0.0, my: 0.0, carry: true },
                "jump" | "scanjump" => K::Jump,
                "dragged" => K::Dragged { foot_x: 0.0, foot_dx: 0.0, regist_at: 250 },
                "regist" => K::Regist,
                "walkwithie" => K::WalkWithIe,
                "movewithturn" => K::MoveTurn { turn_start: 0, turning: false },
                "throwie" => K::ThrowIe { pos: (0.0, 0.0), hwnd: None },
                "breed" | "breedmove" | "breedjump" => K::Breed,
                "transform" => K::Transform,
                "selfdestruct" => K::SelfDestruct,
                "turn" => K::Turn,
                "mute" | "setvariable" | "scaninteract" => K::Instant,
                "scanmove" => K::ScanMove { target: None },
                "interact" => K::Interact,
                "broadcast" | "stay" => K::Stay,
                _ => if def.anims.is_empty() { K::Instant } else { K::Animate },
            },
            ActKind::Reference(_) => unreachable!(),
        };
        Act { def: def.clone(), vars, start: 0, border: BorderRef::None, scale: pack.scale, k }
    }

    fn stub(def: &Rc<ActionDef>, pack: &Pack) -> Act {
        Act { def: def.clone(), vars: VarMap::new(&vec![]), start: 0, border: BorderRef::None, scale: pack.scale, k: K::Instant }
    }

    // ------------------------------------------------------------ helpers

    fn time(&self, m: &MState) -> i64 { m.time - self.start }

    fn var(&self, name: &str, m: &MState, env: &EnvView) -> Option<Val> { self.vars.get(name, m, env) }

    fn num(&self, name: &str, default: f64, m: &MState, env: &EnvView) -> f64 {
        self.var(name, m, env).map(|v| v.num()).unwrap_or(default)
    }

    fn opt_int(&self, name: &str, m: &MState, env: &EnvView) -> Option<i32> {
        self.var(name, m, env).map(|v| v.int())
    }

    fn base_has_next(&self, m: &MState, env: &EnvView) -> bool {
        let effective = self.var("Condition", m, env).map(|v| v.truthy()).unwrap_or(true);
        let dur = self.var("Duration", m, env).map(|v| v.num()).unwrap_or(f64::INFINITY);
        effective && (self.time(m) as f64) < dur
    }

    pub fn anim(&self, m: &MState, env: &EnvView) -> Option<Rc<Animation>> {
        self.def.anims.iter().find(|a| a.cond.as_ref().map(|c| eval_param(c, m, env, Some(&self.vars)).truthy()).unwrap_or(true)).cloned()
    }

    fn anim_total(&self, m: &MState, env: &EnvView) -> i64 { self.anim(m, env).map(|a| a.total).unwrap_or(0) }

    fn play(&self, m: &mut MState, env: &EnvView) {
        let Some(a) = self.anim(m, env) else { return };
        self.play_anim(&a, self.time(m), m);
    }

    fn play_anim(&self, a: &Rc<Animation>, time: i64, m: &mut MState) {
        let (i, p) = a.pose_at(time);
        m.anchor.0 += if m.look_right { -p.vx } else { p.vx };
        m.anchor.1 += p.vy;
        m.frame = if m.look_right {
            match (p.img_right, p.img) {
                (Some(r), _) => Some(Frame { img: r, flip: false, mirror_anchor: true, ax: p.ax, ay: p.ay }),
                (None, Some(l)) => Some(Frame { img: l, flip: true, mirror_anchor: true, ax: p.ax, ay: p.ay }),
                _ => None,
            }
        } else {
            p.img.map(|l| Frame { img: l, flip: false, mirror_anchor: false, ax: p.ax, ay: p.ay })
        };
        let key = (Rc::as_ptr(a) as usize).wrapping_add(i * 0x9E37_79B9);
        if key != m.pose_key {
            m.pose_key = key;
            if let Some(s) = &p.sound { m.out.push(Out::Sound(s.clone())); }
        }
    }

    fn bordered(&self, m: &mut MState, env: &EnvView) -> Result<(), Lost> {
        if self.border != BorderRef::None {
            m.anchor = env.border_move(self.border, m.anchor);
            if !env.is_on(self.border, m.anchor) { return Err(Lost); }
        }
        Ok(())
    }

    fn ie_offsets(&self, m: &MState, env: &EnvView) -> (i32, i32) {
        let s = self.scale;
        ((self.num("IeOffsetX", 0.0, m, env) * s) as i32, (self.num("IeOffsetY", 0.0, m, env) * s) as i32)
    }

    /// Hold the interactive window by its bottom corner (Shimeji's "carry IE").
    fn carry(&self, m: &mut MState, env: &EnvView) -> Result<(), Lost> {
        let (Some(h), true) = (env.ie_hwnd(), env.ie.visible) else { return Err(Lost) };
        let (ox, oy) = self.ie_offsets(m, env);
        let r = env.ie.r;
        let l = if m.look_right { m.anchor.0 + ox } else { m.anchor.0 - ox - r.w() };
        m.out.push(Out::MoveWindow { hwnd: h, l, t: m.anchor.1 + oy - r.h() });
        Ok(())
    }

    fn targets(&self, m: &MState, env: &EnvView) -> (Option<i32>, Option<i32>) {
        (self.opt_int("TargetX", m, env), self.opt_int("TargetY", m, env))
    }

    /// One step of a Move: face the target, animate, and stop exactly on it.
    fn move_step(&self, m: &mut MState, env: &EnvView) {
        let (tx, ty) = self.targets(m, env);
        if let Some(tx) = tx { if m.anchor.0 != tx { m.look_right = m.anchor.0 < tx; } }
        let down = ty.map(|ty| m.anchor.1 < ty).unwrap_or(false);
        self.play(m, env);
        if let Some(tx) = tx {
            if (m.look_right && m.anchor.0 >= tx) || (!m.look_right && m.anchor.0 <= tx) { m.anchor.0 = tx; }
        }
        if let Some(ty) = ty {
            if (down && m.anchor.1 >= ty) || (!down && m.anchor.1 <= ty) { m.anchor.1 = ty; }
        }
    }

    /// What this action offers to other pets' ScanMove (Shimeji-ee `Affordance`), if anything.
    pub fn affordance(&self, m: &MState, env: &EnvView) -> Option<String> {
        if matches!(self.k, K::ScanMove { .. }) { return None; }
        self.var("Affordance", m, env).map(|v| v.as_str()).filter(|s| !s.is_empty())
    }

    pub fn leaf(&self) -> &Act {
        match &self.k {
            K::Seq { ch, idx, .. } | K::Select { ch, idx } if *idx < ch.len() => ch[*idx].leaf(),
            _ => self,
        }
    }

    // ------------------------------------------------------------ lifecycle

    pub fn init(&mut self, m: &mut MState, env: &mut EnvView) {
        self.start = m.time;
        self.vars.init();
        self.border = match self.def.border {
            Some(BorderKind::Floor) => env.floor(m.anchor),
            Some(BorderKind::Wall) => env.wall(m.anchor, m.look_right),
            Some(BorderKind::Ceiling) => env.ceiling(m.anchor),
            None => BorderRef::None,
        };
        let ok = self.base_has_next(m, env);
        match &mut self.k {
            K::Seq { looping, .. } => {
                *looping = self.vars.get("Loop", m, env).map(|v| v.truthy()).unwrap_or(false);
                if ok { self.enter(0, m, env); self.seek(m, env); }
            }
            K::Select { .. } => {
                if ok { self.enter(0, m, env); self.seek(m, env); }
            }
            K::Look => {
                if ok {
                    let lr = self.vars.get("LookRight", m, env).map(|v| v.truthy()).unwrap_or(!m.look_right);
                    m.look_right = lr;
                }
            }
            K::Offset => {
                if ok {
                    let x = self.num("X", 0.0, m, env) as i32;
                    let y = self.num("Y", 0.0, m, env) as i32;
                    m.anchor = (m.anchor.0 + x, m.anchor.1 + y);
                }
            }
            K::Fall { .. } => {
                let vx = self.num("InitialVX", 0.0, m, env);
                let vy = self.num("InitialVY", 0.0, m, env);
                if let K::Fall { vx: a, vy: b, mx, my, .. } = &mut self.k { *a = if vx.is_nan() { 0.0 } else { vx }; *b = if vy.is_nan() { 0.0 } else { vy }; *mx = 0.0; *my = 0.0; }
            }
            K::Dragged { foot_x, foot_dx, regist_at } => {
                *foot_x = env.world.cursor.0 as f64;
                *foot_dx = 0.0;
                *regist_at = 250;
            }
            K::Turn => m.look_right = !m.look_right,
            K::ScanMove { .. } => {
                let want = self.vars.get("Affordance", m, env).map(|v| v.as_str()).unwrap_or_default();
                let me = env.self_id;
                let best = env.world.pets.iter()
                    .filter(|p| p.id != me && !want.is_empty() && p.affordance.as_deref() == Some(want.as_str()) && (p.anchor.1 - m.anchor.1).abs() < 8)
                    .min_by_key(|p| (p.anchor.0 - m.anchor.0).abs())
                    .map(|p| p.id);
                self.k = K::ScanMove { target: best };
            }
            K::ThrowIe { pos, hwnd } => {
                *pos = (env.ie.r.l as f64, env.ie.r.t as f64);
                *hwnd = env.ie_hwnd();
            }
            _ => {}
        }
    }

    fn enter(&mut self, i: usize, m: &mut MState, env: &mut EnvView) {
        if let K::Seq { ch, idx, .. } | K::Select { ch, idx } = &mut self.k {
            *idx = i;
            if i < ch.len() { ch[i].init(m, env); }
        }
    }

    fn seek(&mut self, m: &mut MState, env: &mut EnvView) {
        if !self.base_has_next(m, env) { return; }
        let is_select = matches!(self.k, K::Select { .. });
        let mut wraps = 0;
        loop {
            let (len, i, looping) = match &mut self.k {
                K::Seq { ch, idx, looping } => {
                    if *idx < ch.len() && ch[*idx].has_next(m, env) { return; }
                    (ch.len(), *idx, *looping)
                }
                K::Select { ch, idx } => {
                    if *idx < ch.len() && ch[*idx].has_next(m, env) { return; }
                    (ch.len(), *idx, false)
                }
                _ => return,
            };
            if i >= len { return; }
            let mut n = i + 1;
            if n >= len && looping && !is_select {
                wraps += 1;
                if wraps > 2 { n = len; } else { n = 0; }
            }
            self.enter(n, m, env);
        }
    }

    pub fn has_next(&mut self, m: &mut MState, env: &mut EnvView) -> bool {
        match &self.k {
            K::Seq { .. } => self.seek(m, env),
            _ => {}
        }
        let base = self.base_has_next(m, env);
        if !base { return false; }
        let t = self.time(m);
        match &mut self.k {
            K::Seq { ch, idx, .. } | K::Select { ch, idx } => *idx < ch.len() && ch[*idx].has_next(m, env),
            K::Stay => self.anim(m, env).is_some(),
            K::Animate | K::Breed | K::Transform | K::SelfDestruct | K::Turn | K::Regist | K::Interact => t < self.anim_total(m, env),
            K::ScanMove { target } => {
                let tid = *target;
                self.anim(m, env).is_some() && tid.map(|id| env.world.pets.iter().any(|p| p.id == id)).unwrap_or(false)
            }
            K::ThrowIe { hwnd, .. } => {
                let alive = hwnd.map(|h| env.world.win(h).is_some()).unwrap_or(false);
                alive && t < self.anim_total(m, env)
            }
            K::Move | K::WalkWithIe | K::MoveTurn { .. } => {
                if self.anim(m, env).is_none() { return false; }
                let (tx, ty) = self.targets(m, env);
                !(tx == Some(m.anchor.0) || ty == Some(m.anchor.1))
            }
            K::Look | K::Offset | K::Instant => false,
            K::Fall { .. } => {
                let on = env.land(m.anchor) || env.wall(m.anchor, m.look_right) != BorderRef::NotOn;
                m.ie = env.ie_hwnd().or(m.ie);
                !on
            }
            K::Jump => {
                let (tx, ty) = self.targets(m, env);
                let dx = tx.unwrap_or(m.anchor.0) - m.anchor.0;
                let dy = ty.unwrap_or(m.anchor.1) - m.anchor.1 - dx.abs() / 2;
                dx != 0 || dy != 0
            }
            K::Dragged { regist_at, .. } => t < *regist_at,
        }
    }

    pub fn next(&mut self, m: &mut MState, env: &mut EnvView) -> Result<(), Lost> {
        self.vars.init_frame();
        let t = self.time(m);
        match &self.k {
            K::Seq { .. } | K::Select { .. } => {
                if let K::Seq { ch, idx, .. } | K::Select { ch, idx } = &mut self.k {
                    if *idx < ch.len() && ch[*idx].has_next(m, env) { ch[*idx].next(m, env)?; }
                }
            }
            K::Stay | K::Animate | K::Turn => { self.bordered(m, env)?; self.play(m, env); }
            K::MoveTurn { .. } => {
                self.bordered(m, env)?;
                let K::MoveTurn { mut turn_start, mut turning } = self.k else { unreachable!() };
                let tx = self.targets(m, env).0;
                if let (Some(tx), Some(ta)) = (tx, self.def.anims.get(1).cloned()) {
                    let want_right = m.anchor.0 < tx;
                    if !turning && m.anchor.0 != tx && want_right != m.look_right {
                        m.look_right = want_right;
                        turning = true;
                        turn_start = t;
                    }
                    if turning && t - turn_start < ta.total {
                        self.play_anim(&ta, t - turn_start, m);
                        self.k = K::MoveTurn { turn_start, turning };
                        return Ok(());
                    }
                    turning = false;
                }
                self.k = K::MoveTurn { turn_start, turning };
                self.move_step(m, env);
            }
            K::Move | K::WalkWithIe => {
                self.bordered(m, env)?;
                self.move_step(m, env);
                if matches!(self.k, K::WalkWithIe) { self.carry(m, env)?; }
            }
            K::Look | K::Offset | K::Instant => {}
            K::ScanMove { target } => {
                let Some(tid) = *target else { return Ok(()) };
                self.bordered(m, env)?;
                let Some(tx) = env.world.pets.iter().find(|p| p.id == tid).map(|p| p.anchor.0) else { return Ok(()) };
                if m.anchor.0 != tx { m.look_right = m.anchor.0 < tx; }
                self.play(m, env);
                if (m.look_right && m.anchor.0 >= tx) || (!m.look_right && m.anchor.0 <= tx) || (m.anchor.0 - tx).abs() <= 2 {
                    m.anchor.0 = tx;
                    let s = |n: &str, m: &MState, env: &EnvView| self.var(n, m, env).map(|v| v.as_str()).filter(|s| !s.is_empty());
                    if let Some(b) = s("Behavior", m, env) { m.out.push(Out::SetBehavior { pet: None, behavior: b, face_x: None }); }
                    if let Some(b) = s("TargetBehavior", m, env) { m.out.push(Out::SetBehavior { pet: Some(tid), behavior: b, face_x: Some(m.anchor.0) }); }
                    self.k = K::ScanMove { target: None };
                }
            }
            K::Interact => {
                self.bordered(m, env)?;
                self.play(m, env);
                if t == self.anim_total(m, env) - 1 {
                    if let Some(b) = self.var("Behavior", m, env).map(|v| v.as_str()).filter(|s| !s.is_empty()) {
                        m.out.push(Out::SetBehavior { pet: None, behavior: b, face_x: None });
                    }
                }
            }
            K::Fall { .. } => self.fall(m, env)?,
            K::Jump => {
                let (tx, ty) = self.targets(m, env);
                let (tx, ty) = (tx.unwrap_or(m.anchor.0), ty.unwrap_or(m.anchor.1));
                let v = self.num("VelocityParam", 20.0, m, env);
                m.look_right = m.anchor.0 < tx;
                let dx = tx - m.anchor.0;
                let dy = ty - m.anchor.1 - dx.abs() / 2;
                let dist = ((dx as f64).powi(2) + (dy as f64).powi(2)).sqrt();
                if dist != 0.0 {
                    m.anchor.0 += (v * dx as f64 / dist) as i32;
                    m.anchor.1 += (v * dy as f64 / dist) as i32;
                    self.play(m, env);
                }
                if dist <= v { m.anchor = (tx, ty); }
            }
            K::Dragged { .. } => {
                m.look_right = false;
                m.dragging = true;
                let s = self.scale;
                let ox = (self.num("OffsetX", 0.0, m, env) * s) as i32;
                let oy = (self.num("OffsetY", 0.0, m, env) * s) as i32;
                let c = env.world.cursor;
                let (fx, fdx) = if let K::Dragged { foot_x, foot_dx, regist_at } = &mut self.k {
                    if (c.0 - m.anchor.0 + ox).abs() >= 5 { *regist_at = t + 250; }
                    *foot_dx = (*foot_dx + (c.0 as f64 - *foot_x) * 0.1) * 0.8;
                    *foot_x += *foot_dx;
                    (*foot_x, *foot_dx)
                } else { unreachable!() };
                self.vars.set("FootX", Val::Num(fx));
                self.vars.set("FootDX", Val::Num(fdx));
                self.play(m, env);
                m.anchor = (c.0 + ox, c.1 + oy);
            }
            K::Regist => {
                m.look_right = false;
                self.play(m, env);
                if t + 1 >= self.anim_total(m, env) {
                    m.dragging = false;
                    m.look_right = random() < 0.5;
                    return Err(Lost);
                }
            }
            K::ThrowIe { .. } => {
                self.play(m, env);
                let vx = self.num("InitialVX", 32.0, m, env) * self.scale;
                let vy = self.num("InitialVY", -10.0, m, env) * self.scale;
                let g = self.num("Gravity", 0.5, m, env) * self.scale;
                let lr = m.look_right;
                if let K::ThrowIe { pos, hwnd: Some(h) } = &mut self.k {
                    pos.0 += if lr { vx } else { -vx };
                    pos.1 += vy + t as f64 * g;
                    m.out.push(Out::MoveWindow { hwnd: *h, l: pos.0 as i32, t: pos.1 as i32 });
                }
            }
            K::Breed | K::Transform | K::SelfDestruct => {
                self.bordered(m, env)?;
                self.play(m, env);
                if t == self.anim_total(m, env) - 1 {
                    let str_var = |n: &str, m: &MState, env: &EnvView| self.var(n, m, env).map(|v| v.as_str()).filter(|s| !s.is_empty());
                    match self.k {
                        K::Breed => {
                            let s = self.scale;
                            let bx = (self.num("BornX", 0.0, m, env) * s) as i32;
                            let by = (self.num("BornY", 0.0, m, env) * s) as i32;
                            m.out.push(Out::Spawn {
                                pack: str_var("BornMascot", m, env),
                                anchor: (m.anchor.0 + if m.look_right { -bx } else { bx }, m.anchor.1 + by),
                                look_right: m.look_right,
                                behavior: str_var("BornBehavior", m, env).unwrap_or_else(|| "Fall".into()),
                            });
                        }
                        K::Transform => {
                            if let Some(b) = str_var("TransformBehavior", m, env) {
                                m.out.push(Out::Transform { pack: str_var("TransformMascot", m, env), behavior: b });
                            }
                        }
                        _ => m.out.push(Out::Remove),
                    }
                }
            }
        }
        Ok(())
    }

    fn fall(&mut self, m: &mut MState, env: &mut EnvView) -> Result<(), Lost> {
        let g = self.num("Gravity", 2.0, m, env);
        let rx = self.var("RegistanceX", m, env).or_else(|| self.var("ResistanceX", m, env)).map(|v| v.num()).unwrap_or(0.05);
        let ry = self.var("RegistanceY", m, env).or_else(|| self.var("ResistanceY", m, env)).map(|v| v.num()).unwrap_or(0.1);
        let K::Fall { vx, vy, mx, my, carry } = &mut self.k else { return Ok(()) };
        let carry = *carry;
        if *vx != 0.0 { m.look_right = *vx > 0.0; }
        *vx -= *vx * rx;
        *vy = *vy - *vy * ry + g;
        *mx += *vx % 1.0;
        *my += *vy % 1.0;
        let dx = *vx as i32 + *mx as i32;
        let dy = *vy as i32 + *my as i32;
        *mx %= 1.0;
        *my %= 1.0;
        let speed = *vy;
        let dev = dx.abs().max(dy.abs()).max(1);
        let start = m.anchor;
        let mut landed = false;
        'outer: for i in 0..=dev {
            let x = start.0 + dx * i / dev;
            let y = start.1 + dy * i / dev;
            m.anchor = (x, y);
            if dy > 0 && !carry {
                for j in -80..=0 {
                    m.anchor = (x, y + j);
                    if env.land(m.anchor) { landed = true; break 'outer; }
                }
                m.anchor = (x, y);
            } else if dy > 0 && env.floor(m.anchor) != BorderRef::NotOn {
                break;
            }
            if env.wall(m.anchor, m.look_right) != BorderRef::NotOn { break; }
        }
        if landed {
            m.ie = env.ie_hwnd().or(m.ie);
            if speed > SHAKE_SPEED {
                if let (BorderRef::Ie(_), Some(h)) = (env.floor(m.anchor), env.ie_hwnd()) { m.out.push(Out::Shake(h)); }
            }
        }
        self.play(m, env);
        if carry { self.carry(m, env)?; }
        Ok(())
    }
}
