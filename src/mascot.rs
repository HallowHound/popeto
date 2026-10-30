//! One pet: behaviour selection (weighted, conditional, NextBehaviorList) on top of the action tree.

use crate::action::{Act, Lost};
use crate::config::{Animation, Pack};
use crate::env::{eval_param, EnvView, MState, Side};
use crate::expr::random;
use std::collections::HashSet;
use std::rc::Rc;

pub struct Mascot {
    pub id: u32,
    pub pack: usize,
    pub st: MState,
    pub beh: String,
    act: Option<Act>,
    /// Current behaviour was entered through a hotspot (petting etc).
    pub hotspot_beh: bool,
    /// Mouse is held down on a hotspot.
    pub holding: bool,
    /// Set when a prank-driven action (`run_action`) ran to its end this tick.
    pub prank_finished: Option<String>,
}

impl Mascot {
    pub fn new(id: u32, pack: usize, anchor: (i32, i32), look_right: bool) -> Self {
        Mascot {
            id,
            pack,
            st: MState { anchor, look_right, time: 0, dragging: false, ie: None, monitor: 0, frame: None, pose_key: 0, out: vec![] },
            beh: String::new(),
            act: None,
            hotspot_beh: false,
            holding: false,
            prank_finished: None,
        }
    }

    fn on_ie(&self, env: &EnvView) -> bool {
        let p = self.st.anchor;
        [Side::Top, Side::Bottom, Side::Left, Side::Right].iter().any(|s| env.on(&env.ie, *s, p))
    }

    pub fn set_behavior(&mut self, pack: &Pack, name: &str, env: &mut EnvView, disabled: &HashSet<String>, log: &mut Vec<String>) {
        if !self.on_ie(env) {
            self.st.ie = env.world.active_for(self.st.monitor);
            env.bind_ie(self.st.ie);
        }
        self.hotspot_beh = false;
        let fall = pack.required("Fall");
        let mut name = name.to_string();
        for attempt in 0..24 {
            let built = pack.behavior(&name).and_then(|b| pack.actions.get(&b.action).map(|a| (b, a)));
            let Some((b, a)) = built else {
                log.push(format!("{}: unknown behaviour '{name}'", pack.name));
                if name == fall || attempt > 20 { self.act = None; return; }
                name = fall.clone();
                continue;
            };
            let mut act = Act::build(a, &b.params, pack, log);
            act.init(&mut self.st, env);
            self.beh = name.clone();
            if act.has_next(&mut self.st, env) {
                self.act = Some(act);
                return;
            }
            name = self.next_name(pack, env, &name, disabled);
        }
        self.act = None;
    }

    fn conds_ok(&self, conds: &[Rc<crate::config::Param>], env: &EnvView) -> bool {
        conds.iter().all(|c| eval_param(c, &self.st, env, None).truthy())
    }

    fn next_name(&mut self, pack: &Pack, env: &mut EnvView, prev: &str, disabled: &HashSet<String>) -> String {
        let mut cands: Vec<(&str, f64)> = Vec::new();
        let mut add_all = true;
        if let Some(nl) = pack.behavior(prev).and_then(|b| b.next.as_ref()) {
            add_all = nl.add;
            for r in &nl.refs {
                if r.freq > 0.0 && !disabled.contains(&r.name) && self.conds_ok(&r.conds, env) { cands.push((&r.name, r.freq)); }
            }
        }
        if add_all {
            for b in &pack.behaviors {
                if b.freq > 0.0 && !disabled.contains(&b.name) && self.conds_ok(&b.conds, env) { cands.push((&b.name, b.freq)); }
            }
        }
        let total: f64 = cands.iter().map(|c| c.1).sum();
        if total <= 0.0 {
            let w = env.work.r;
            self.st.anchor = (w.l + (random() * w.w() as f64) as i32, w.t - 256);
            return pack.required("Fall");
        }
        let mut r = random() * total;
        for (n, f) in &cands {
            r -= f;
            if r < 0.0 { return n.to_string(); }
        }
        cands.last().unwrap().0.to_string()
    }

    pub fn tick(&mut self, pack: &Pack, env: &mut EnvView, disabled: &HashSet<String>, log: &mut Vec<String>) {
        let Some(mut act) = self.act.take() else {
            let f = pack.required("Fall");
            self.set_behavior(pack, &f, env, disabled, log);
            self.st.time += 1;
            return;
        };
        let st = &mut self.st;
        let res: Result<bool, Lost> = (|| {
            if act.has_next(st, env) { act.next(st, env)?; }
            Ok(act.has_next(st, env))
        })();
        match res {
            Ok(true) => {
                let w = env.screen.r;
                let a = self.st.anchor;
                if !self.st.dragging && (a.1 > w.b + 64 || a.0 < w.l - 128 || a.0 > w.r + 128) {
                    let wa = env.work.r;
                    self.st.anchor = (wa.l + 64 + (random() * (wa.w() - 128).max(1) as f64) as i32, wa.t - 256);
                    let f = pack.required("Fall");
                    self.set_behavior(pack, &f, env, disabled, log);
                } else {
                    self.act = Some(act);
                }
            }
            Ok(false) => {
                if self.beh.starts_with("#prank") { self.prank_finished = Some(self.beh.clone()); }
                let prev = self.beh.clone();
                let n = self.next_name(pack, env, &prev, disabled);
                self.set_behavior(pack, &n, env, disabled, log);
            }
            Err(Lost) => {
                let f = pack.required("Fall");
                self.set_behavior(pack, &f, env, disabled, log);
            }
        }
        self.st.time += 1;
    }

    /// Play a pack action directly (used by pranks). `tag` becomes the behaviour name, `#prank...`.
    pub fn run_action(&mut self, pack: &Pack, action: &str, params: &[(&str, &str)], tag: &str, env: &mut EnvView, log: &mut Vec<String>) -> bool {
        let Some(def) = pack.actions.get(action) else { return false };
        let ps: crate::config::Params = params.iter().map(|(k, v)| (Rc::from(*k), crate::config::parse_param(v, log))).collect();
        let mut act = Act::build(def, &ps, pack, log);
        act.init(&mut self.st, env);
        if !act.has_next(&mut self.st, env) { return false; }
        self.act = Some(act);
        self.beh = tag.to_string();
        self.hotspot_beh = false;
        true
    }

    /// Animation + draggable flag of the action currently playing.
    pub fn current(&self, env: &EnvView) -> Option<(Rc<Animation>, bool)> {
        let leaf = self.act.as_ref()?.leaf();
        let drag = leaf.vars.get("Draggable", &self.st, env).map(|v| v.truthy()).unwrap_or(true);
        Some((leaf.anim(&self.st, env)?, drag))
    }

    pub fn next_after_hotspot(&mut self, pack: &Pack, env: &mut EnvView, disabled: &HashSet<String>, log: &mut Vec<String>) {
        let prev = self.beh.clone();
        let n = self.next_name(pack, env, &prev, disabled);
        self.set_behavior(pack, &n, env, disabled, log);
    }
}
