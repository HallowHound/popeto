//! Loads Shimeji / Shimeji-ee pet packs: `img/<Name>/*.png` plus `actions.xml` and `behaviors.xml`
//! (English or the original Japanese schema).

use crate::expr::{self, Expr, Val};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

// ---------------------------------------------------------------- parameters

pub enum ParamSrc {
    Lit(Val),
    Script { expr: Option<Expr>, every_frame: bool },
}

pub struct Param {
    pub src: ParamSrc,
}

pub type Params = Vec<(Rc<str>, Rc<Param>)>;

pub fn parse_param(raw: &str, log: &mut Vec<String>) -> Rc<Param> {
    let t = raw.trim();
    let src = if (t.starts_with("${") || t.starts_with("#{")) && t.ends_with('}') {
        let body = &t[2..t.len() - 1];
        let expr = match expr::parse(body) {
            Ok(e) => Some(e),
            Err(e) => { log.push(format!("script error in '{body}': {e}")); None }
        };
        ParamSrc::Script { expr, every_frame: t.starts_with('#') }
    } else if t.eq_ignore_ascii_case("true") {
        ParamSrc::Lit(Val::Bool(true))
    } else if t.eq_ignore_ascii_case("false") {
        ParamSrc::Lit(Val::Bool(false))
    } else if let Ok(n) = t.parse::<f64>() {
        ParamSrc::Lit(Val::Num(n))
    } else {
        ParamSrc::Lit(Val::Str(raw.into()))
    };
    Rc::new(Param { src })
}

/// Overlay `top` on `base` (top wins).
pub fn merge(base: &Params, top: &Params) -> Params {
    let mut out: Params = base.iter().filter(|(k, _)| !top.iter().any(|(t, _)| t == k)).cloned().collect();
    out.extend(top.iter().cloned());
    out
}

// ---------------------------------------------------------------- definitions

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BorderKind { Floor, Wall, Ceiling }

pub struct Pose {
    pub img: Option<usize>,
    pub img_right: Option<usize>,
    pub ax: i32,
    pub ay: i32,
    pub vx: i32,
    pub vy: i32,
    pub dur: i64,
    pub sound: Option<PathBuf>,
}

pub struct Hotspot {
    pub ellipse: bool,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub behavior: Option<String>,
}

pub struct Animation {
    pub cond: Option<Rc<Param>>,
    pub poses: Vec<Pose>,
    pub total: i64,
    pub hotspots: Vec<Hotspot>,
}

impl Animation {
    pub fn pose_at(&self, time: i64) -> (usize, &Pose) {
        let mut t = if self.total > 0 { time.rem_euclid(self.total) } else { 0 };
        for (i, p) in self.poses.iter().enumerate() {
            t -= p.dur;
            if t < 0 { return (i, p); }
        }
        (self.poses.len() - 1, self.poses.last().unwrap())
    }
}

pub enum ActKind {
    Stay,
    Move,
    Animate,
    Sequence,
    Select,
    Embedded(String),
    Reference(String),
}

pub struct ActionDef {
    pub name: String,
    pub kind: ActKind,
    pub border: Option<BorderKind>,
    pub params: Params,
    pub anims: Vec<Rc<Animation>>,
    pub children: Vec<Rc<ActionDef>>,
}

pub struct BehRef {
    pub name: String,
    pub freq: f64,
    pub conds: Vec<Rc<Param>>,
}

pub struct NextList {
    pub add: bool,
    pub refs: Vec<BehRef>,
}

pub struct BehaviorDef {
    pub name: String,
    pub action: String,
    pub freq: f64,
    pub hidden: bool,
    pub toggleable: bool,
    pub conds: Vec<Rc<Param>>,
    pub next: Option<NextList>,
    pub params: Params,
}

pub struct Pack {
    pub name: String,
    #[allow(dead_code)]
    pub dir: PathBuf,
    pub actions: HashMap<String, Rc<ActionDef>>,
    pub behaviors: Vec<BehaviorDef>,
    pub images: Vec<PathBuf>,
    pub icon_img: Option<usize>,
    pub scale: f64,
}

impl Pack {
    pub fn behavior(&self, name: &str) -> Option<&BehaviorDef> {
        self.behaviors.iter().find(|b| b.name == name)
    }
    /// Look up one of the engine-required behaviours, accepting the Japanese names too.
    pub fn required(&self, en: &str) -> String {
        let ja = match en {
            "Fall" => "落下する",
            "Dragged" => "ドラッグされる",
            "Thrown" => "投げられる",
            "ChaseMouse" => "マウスの周りに集まる",
            _ => en,
        };
        if self.behavior(en).is_none() && self.behavior(ja).is_some() { ja.into() } else { en.into() }
    }
}

// ---------------------------------------------------------------- japanese schema

fn ja(s: &str) -> &str {
    match s {
        "動作リスト" => "ActionList",
        "動作" => "Action",
        "動作参照" => "ActionReference",
        "名前" => "Name",
        "種類" => "Type",
        "クラス" => "Class",
        "組み込み" => "Embedded",
        "移動" => "Move",
        "静止" => "Stay",
        "固定" => "Animate",
        "複合" => "Sequence",
        "選択" => "Select",
        "枠" => "BorderType",
        "天井" => "Ceiling",
        "壁" => "Wall",
        "地面" => "Floor",
        "目的地X" => "TargetX",
        "目的地Y" => "TargetY",
        "速度" => "VelocityParam",
        "初速X" => "InitialVX",
        "初速Y" => "InitialVY",
        "重力" => "Gravity",
        "空気抵抗X" => "RegistanceX",
        "空気抵抗Y" => "RegistanceY",
        "右向き" => "LookRight",
        "IEの端X" => "IeOffsetX",
        "IEの端Y" => "IeOffsetY",
        "生まれる場所X" => "BornX",
        "生まれる場所Y" => "BornY",
        "生まれた時の行動" => "BornBehavior",
        "行動" => "Behavior",
        "繰り返し" => "Loop",
        "アニメーション" => "Animation",
        "条件" => "Condition",
        "画像" => "Image",
        "基準座標" => "ImageAnchor",
        "移動速度" => "Velocity",
        "長さ" => "Duration",
        "行動リスト" => "BehaviorList",
        "頻度" => "Frequency",
        "次の行動リスト" => "NextBehaviorList",
        "追加" => "Add",
        "行動参照" => "BehaviorReference",
        "端X" => "OffsetX",
        "端Y" => "OffsetY",
        "ポーズ" => "Pose",
        "定数" => "Constant",
        "値" => "Value",
        "footX" => "FootX",
        "footDX" => "FootDX",
        // British spellings used by Shimeji-ee
        "BehaviourList" => "BehaviorList",
        "Behaviour" => "Behavior",
        "NextBehaviourList" => "NextBehaviorList",
        "BehaviourReference" => "BehaviorReference",
        "BornBehaviour" => "BornBehavior",
        "TransformBehaviour" => "TransformBehavior",
        "TargetBehaviour" => "TargetBehavior",
        _ => s,
    }
}

fn tag<'a>(n: &roxmltree::Node<'a, 'a>) -> &'a str { ja(n.tag_name().name()) }

fn attr(n: &roxmltree::Node, name: &str) -> Option<String> {
    n.attributes().find(|a| ja(a.name()) == name).map(|a| a.value().to_string())
}

fn attr_params(n: &roxmltree::Node, skip: &[&str], log: &mut Vec<String>) -> Params {
    n.attributes()
        .filter(|a| !skip.contains(&ja(a.name())))
        .map(|a| (Rc::from(ja(a.name())), parse_param(ja(a.value()), log)))
        .collect()
}

fn pair(s: &str) -> (f64, f64) {
    let mut it = s.split(',').map(|v| v.trim().parse::<f64>().unwrap_or(0.0));
    (it.next().unwrap_or(0.0), it.next().unwrap_or(0.0))
}

fn elements<'a>(n: roxmltree::Node<'a, 'a>) -> impl Iterator<Item = roxmltree::Node<'a, 'a>> {
    n.children().filter(|c| c.is_element())
}

// ---------------------------------------------------------------- loader

struct Loader<'p> {
    dir: &'p Path,
    sound_dirs: Vec<PathBuf>,
    scale: f64,
    images: Vec<PathBuf>,
    img_index: HashMap<PathBuf, usize>,
    log: Vec<String>,
    anon: usize,
}

impl Loader<'_> {
    fn image(&mut self, raw: &str) -> Option<usize> {
        let rel = raw.trim().trim_start_matches(['/', '\\']);
        if rel.is_empty() { return None; }
        let p = self.dir.join(rel);
        if !p.exists() {
            self.log.push(format!("missing image {}", p.display()));
            return None;
        }
        if let Some(i) = self.img_index.get(&p) { return Some(*i); }
        self.images.push(p.clone());
        self.img_index.insert(p, self.images.len() - 1);
        Some(self.images.len() - 1)
    }

    fn sound(&self, raw: &str) -> Option<PathBuf> {
        let rel = raw.trim().trim_start_matches(['/', '\\']);
        self.sound_dirs.iter().map(|d| d.join(rel)).find(|p| p.exists())
    }

    fn animation(&mut self, n: roxmltree::Node) -> Animation {
        let s = self.scale;
        let mut poses = Vec::new();
        let mut hotspots = Vec::new();
        for c in elements(n) {
            match tag(&c) {
                "Pose" => {
                    let (ax, ay) = pair(&attr(&c, "ImageAnchor").unwrap_or_default());
                    let (vx, vy) = pair(&attr(&c, "Velocity").unwrap_or_default());
                    let img = attr(&c, "Image").and_then(|i| self.image(&i));
                    let img_right = attr(&c, "ImageRight").and_then(|i| self.image(&i));
                    poses.push(Pose {
                        img,
                        img_right,
                        ax: (ax * s).round() as i32,
                        ay: (ay * s).round() as i32,
                        vx: (vx * s).round() as i32,
                        vy: (vy * s).round() as i32,
                        dur: attr(&c, "Duration").and_then(|d| d.trim().parse::<f64>().ok()).unwrap_or(1.0).max(1.0) as i64,
                        sound: attr(&c, "Sound").and_then(|x| self.sound(&x)),
                    });
                }
                "Hotspot" => {
                    let (x, y) = pair(&attr(&c, "Origin").unwrap_or_default());
                    let (w, h) = pair(&attr(&c, "Size").unwrap_or_default());
                    hotspots.push(Hotspot {
                        ellipse: attr(&c, "Shape").map(|v| v.eq_ignore_ascii_case("ellipse")).unwrap_or(false),
                        x: (x * s) as i32,
                        y: (y * s) as i32,
                        w: (w * s) as i32,
                        h: (h * s) as i32,
                        behavior: attr(&c, "Behavior").or_else(|| attr(&c, "Behaviour")),
                    });
                }
                _ => {}
            }
        }
        if poses.is_empty() {
            poses.push(Pose { img: None, img_right: None, ax: 0, ay: 0, vx: 0, vy: 0, dur: 1, sound: None });
        }
        let total = poses.iter().map(|p| p.dur).sum();
        Animation { cond: attr(&n, "Condition").map(|c| parse_param(&c, &mut self.log)), poses, total, hotspots }
    }

    fn action(&mut self, n: roxmltree::Node) -> ActionDef {
        if tag(&n) == "ActionReference" {
            let name = attr(&n, "Name").unwrap_or_default();
            return ActionDef {
                name: String::new(),
                kind: ActKind::Reference(name),
                border: None,
                params: attr_params(&n, &["Name"], &mut self.log),
                anims: vec![],
                children: vec![],
            };
        }
        let name = attr(&n, "Name").unwrap_or_else(|| { self.anon += 1; format!("#anon{}", self.anon) });
        let class = attr(&n, "Class");
        let ty = attr(&n, "Type").map(|t| ja(&t).to_string()).unwrap_or_default();
        let kind = match (ty.as_str(), &class) {
            (_, Some(c)) => ActKind::Embedded(c.rsplit('.').next().unwrap_or(c).to_ascii_lowercase()),
            ("Move", _) => ActKind::Move,
            ("Stay", _) => ActKind::Stay,
            ("Animate", _) => ActKind::Animate,
            ("Sequence", _) => ActKind::Sequence,
            ("Select", _) => ActKind::Select,
            _ => ActKind::Stay,
        };
        let border = attr(&n, "BorderType").and_then(|b| match ja(&b) {
            "Floor" => Some(BorderKind::Floor),
            "Wall" => Some(BorderKind::Wall),
            "Ceiling" => Some(BorderKind::Ceiling),
            _ => None,
        });
        let params = attr_params(&n, &["Name", "Type", "Class", "BorderType"], &mut self.log);
        let mut anims = Vec::new();
        let mut children = Vec::new();
        for c in elements(n) {
            match tag(&c) {
                "Animation" => anims.push(Rc::new(self.animation(c))),
                "Action" | "ActionReference" => children.push(Rc::new(self.action(c))),
                _ => {}
            }
        }
        ActionDef { name, kind, border, params, anims, children }
    }

    fn behavior_list(&mut self, n: roxmltree::Node, conds: &[Rc<Param>], out: &mut Vec<BehaviorDef>) {
        for c in elements(n) {
            match tag(&c) {
                "Condition" => {
                    let mut cc = conds.to_vec();
                    if let Some(x) = attr(&c, "Condition") { cc.push(parse_param(&x, &mut self.log)); }
                    self.behavior_list(c, &cc, out);
                }
                "Behavior" => {
                    let name = attr(&c, "Name").unwrap_or_default();
                    let mut cc = conds.to_vec();
                    if let Some(x) = attr(&c, "Condition") { cc.push(parse_param(&x, &mut self.log)); }
                    let next = elements(c).find(|e| tag(e) == "NextBehaviorList").map(|nl| {
                        let mut refs = Vec::new();
                        self.next_refs(nl, &[], &mut refs);
                        NextList { add: attr(&nl, "Add").map(|a| a.trim() != "false").unwrap_or(true), refs }
                    });
                    out.push(BehaviorDef {
                        action: attr(&c, "Action").unwrap_or_else(|| name.clone()),
                        freq: attr(&c, "Frequency").and_then(|f| f.trim().parse().ok()).unwrap_or(0.0),
                        hidden: attr(&c, "Hidden").map(|h| h.trim() == "true").unwrap_or(false),
                        toggleable: attr(&c, "Toggleable").map(|h| h.trim() == "true").unwrap_or(false),
                        conds: cc,
                        next,
                        params: attr_params(&c, &["Name", "Frequency", "Hidden", "Condition", "Toggleable", "Action"], &mut self.log),
                        name,
                    });
                }
                _ => {}
            }
        }
    }

    fn next_refs(&mut self, n: roxmltree::Node, conds: &[Rc<Param>], out: &mut Vec<BehRef>) {
        for c in elements(n) {
            match tag(&c) {
                "Condition" => {
                    let mut cc = conds.to_vec();
                    if let Some(x) = attr(&c, "Condition") { cc.push(parse_param(&x, &mut self.log)); }
                    self.next_refs(c, &cc, out);
                }
                "BehaviorReference" => {
                    let mut cc = conds.to_vec();
                    if let Some(x) = attr(&c, "Condition") { cc.push(parse_param(&x, &mut self.log)); }
                    out.push(BehRef {
                        name: attr(&c, "Name").unwrap_or_default(),
                        freq: attr(&c, "Frequency").and_then(|f| f.trim().parse().ok()).unwrap_or(0.0),
                        conds: cc,
                    });
                }
                _ => {}
            }
        }
    }
}

pub(crate) fn find_conf(root: &Path, pack_dir: &Path, name: &str, file: &str) -> Option<PathBuf> {
    let jp = if file == "actions.xml" { "動作.xml" } else { "行動.xml" };
    [pack_dir.join("conf"), root.join("conf").join(name), root.join("conf")]
        .iter()
        .flat_map(|d| [d.join(file), d.join(jp)])
        .find(|p| p.exists())
}

fn read_xml(p: &Path) -> Result<String, String> {
    let bytes = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let s = String::from_utf8_lossy(&bytes).into_owned();
    Ok(s.trim_start_matches('\u{feff}').to_string())
}

pub fn load_pack(root: &Path, dir: &Path, scale: f64) -> Result<(Pack, Vec<String>), String> {
    let name = dir.file_name().unwrap().to_string_lossy().to_string();
    let act_path = find_conf(root, dir, &name, "actions.xml").ok_or("no actions.xml found")?;
    let beh_path = find_conf(root, dir, &name, "behaviors.xml").ok_or("no behaviors.xml found")?;
    let mut l = Loader {
        dir,
        sound_dirs: vec![dir.join("sound"), root.join("sound"), dir.to_path_buf()],
        scale,
        images: vec![],
        img_index: HashMap::new(),
        log: vec![],
        anon: 0,
    };
    let mut actions = HashMap::new();
    let mut behaviors = Vec::new();
    let mut seen = HashSet::new();
    for path in [&act_path, &beh_path] {
        if !seen.insert(path.clone()) { continue; }
        let text = read_xml(path)?;
        let doc = roxmltree::Document::parse_with_options(&text, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() })
            .map_err(|e| format!("{}: {e}", path.display()))?;
        for list in doc.root_element().children().filter(|c| c.is_element()) {
            match tag(&list) {
                "ActionList" => {
                    for a in elements(list).filter(|a| tag(a) == "Action") {
                        let def = l.action(a);
                        actions.insert(def.name.clone(), Rc::new(def));
                    }
                }
                "BehaviorList" => l.behavior_list(list, &[], &mut behaviors),
                _ => {}
            }
        }
    }
    if behaviors.is_empty() { return Err("no behaviours defined".into()); }
    let icon_img = ["shime1.png", "icon.png"].iter().find_map(|f| l.img_index.get(&dir.join(f)).copied())
        .or(if l.images.is_empty() { None } else { Some(0) });
    let log = std::mem::take(&mut l.log);
    let pack = Pack { name, dir: dir.to_path_buf(), actions, behaviors, images: l.images, icon_img, scale };
    Ok((pack, log))
}

/// Every sub-folder of `img/` that has pngs is a pet (except `unused`).
pub fn discover(root: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(root.join("img"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && !p.file_name().map(|n| n.eq_ignore_ascii_case("unused")).unwrap_or(false))
        .filter(|p| std::fs::read_dir(p).map(|r| r.flatten().any(|e| e.path().extension().map(|x| x.eq_ignore_ascii_case("png")).unwrap_or(false))).unwrap_or(false))
        .collect();
    v.sort();
    v
}
