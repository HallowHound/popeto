//! Converts pets in the mobile Shimeji app format (`manifest.json` + `animation.json` + WebP sprites,
//! as downloaded from shimeji.org) into a regular Shimeji-ee pet: PNG frames + generated
//! `conf/actions.xml` and `conf/behaviors.xml` built from the template in assets/template.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

// ---------------------------------------------------------------- minimal JSON

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum J { Null, Bool(bool), Num(f64), Str(String), Arr(Vec<J>), Obj(Vec<(String, J)>) }

impl J {
    pub fn get(&self, k: &str) -> Option<&J> {
        if let J::Obj(v) = self { v.iter().find(|(n, _)| n == k).map(|(_, j)| j) } else { None }
    }
    pub fn str(&self) -> Option<&str> { if let J::Str(s) = self { Some(s) } else { None } }
    pub fn num(&self) -> Option<f64> { if let J::Num(n) = self { Some(*n) } else { None } }
    pub fn arr(&self) -> &[J] { if let J::Arr(a) = self { a } else { &[] } }
}

struct P<'a> { s: &'a [u8], i: usize }

impl P<'_> {
    fn ws(&mut self) { while self.i < self.s.len() && (self.s[self.i] as char).is_whitespace() { self.i += 1; } }
    fn val(&mut self) -> Option<J> {
        self.ws();
        match *self.s.get(self.i)? {
            b'{' => {
                self.i += 1;
                let mut v = Vec::new();
                loop {
                    self.ws();
                    if self.s.get(self.i) == Some(&b'}') { self.i += 1; break; }
                    let J::Str(k) = self.val()? else { return None };
                    self.ws();
                    if self.s.get(self.i) != Some(&b':') { return None; }
                    self.i += 1;
                    v.push((k, self.val()?));
                    self.ws();
                    match self.s.get(self.i)? { b',' => self.i += 1, b'}' => { self.i += 1; break; } _ => return None }
                }
                Some(J::Obj(v))
            }
            b'[' => {
                self.i += 1;
                let mut v = Vec::new();
                loop {
                    self.ws();
                    if self.s.get(self.i) == Some(&b']') { self.i += 1; break; }
                    v.push(self.val()?);
                    self.ws();
                    match self.s.get(self.i)? { b',' => self.i += 1, b']' => { self.i += 1; break; } _ => return None }
                }
                Some(J::Arr(v))
            }
            b'"' => {
                self.i += 1;
                let mut out = Vec::new();
                while let Some(&c) = self.s.get(self.i) {
                    self.i += 1;
                    match c {
                        b'"' => return Some(J::Str(String::from_utf8_lossy(&out).into_owned())),
                        b'\\' => {
                            let e = *self.s.get(self.i)?;
                            self.i += 1;
                            match e {
                                b'n' => out.push(b'\n'),
                                b't' => out.push(b'\t'),
                                b'u' => {
                                    let hex = std::str::from_utf8(self.s.get(self.i..self.i + 4)?).ok()?;
                                    self.i += 4;
                                    let ch = char::from_u32(u32::from_str_radix(hex, 16).ok()?).unwrap_or('?');
                                    let mut b = [0u8; 4];
                                    out.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
                                }
                                o => out.push(o),
                            }
                        }
                        c => out.push(c),
                    }
                }
                None
            }
            b't' => { self.i += 4; Some(J::Bool(true)) }
            b'f' => { self.i += 5; Some(J::Bool(false)) }
            b'n' => { self.i += 4; Some(J::Null) }
            _ => {
                let st = self.i;
                while self.i < self.s.len() && matches!(self.s[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') { self.i += 1; }
                std::str::from_utf8(&self.s[st..self.i]).ok()?.parse().ok().map(J::Num)
            }
        }
    }
}

pub fn parse_json(text: &str) -> Option<J> {
    P { s: text.trim_start_matches('\u{feff}').as_bytes(), i: 0 }.val()
}

// ---------------------------------------------------------------- conversion

pub fn is_mobile_pack(dir: &Path) -> bool { dir.join("manifest.json").is_file() && dir.join("animation.json").is_file() }

pub fn manifest_name(text: &str) -> Option<String> {
    parse_json(text)?.get("name")?.str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[derive(Clone, Copy)]
struct Frame { sprite: i64, dx: f64, dy: f64, ticks: f64 }

/// "%04d.webp" style pattern -> file name for sprite `i`.
fn sprite_name(pattern: &str, i: usize) -> String {
    if let Some(p) = pattern.find('%') {
        if let Some(d) = pattern[p..].find('d') {
            let width: usize = pattern[p + 1..p + d].trim_start_matches('0').parse().unwrap_or(0);
            return format!("{}{:0width$}{}", &pattern[..p], i, &pattern[p + d + 1..], width = width);
        }
    }
    pattern.replace("{}", &i.to_string())
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    let f = fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(|e| e.to_string())?;
    wr.write_image_data(rgba).map_err(|e| e.to_string())
}

/// Convert the pet in `src` into a Shimeji-ee pet folder `dest` (created).
pub fn convert(src: &Path, dest: &Path) -> Result<(), String> {
    let manifest = parse_json(&fs::read_to_string(src.join("manifest.json")).map_err(|e| e.to_string())?).ok_or("manifest.json is not valid JSON")?;
    let anim = parse_json(&fs::read_to_string(src.join("animation.json")).map_err(|e| e.to_string())?).ok_or("animation.json is not valid JSON")?;
    let sprites = manifest.get("sprites").ok_or("manifest has no sprites")?;
    let base = sprites.get("basePath").and_then(|v| v.str()).unwrap_or("sprites/");
    let pattern = sprites.get("filePattern").and_then(|v| v.str()).unwrap_or("%04d.webp");
    let count = sprites.get("spriteCount").and_then(|v| v.num()).unwrap_or(0.0) as usize;
    if count == 0 { return Err("no sprites listed".into()); }

    // frames are made for classic 128 px Shimeji art: scale sprites so speeds and anchors match
    fs::create_dir_all(dest.join("conf")).map_err(|e| e.to_string())?;
    let mut out_size = (128u32, 128u32);
    // visible bounds of every frame (left, top, right, bottom) for wall/ceiling anchors
    let mut bounds: HashMap<i64, (i64, i64, i64, i64)> = HashMap::new();
    for i in 0..count {
        let p = src.join(base).join(sprite_name(pattern, i));
        let Ok(bytes) = fs::read(&p) else { continue };
        let Some((w, h, rgba)) = crate::win::decode_rgba(&bytes) else { return Err(format!("cannot read {}", p.display())) };
        let k = 128.0 / h as f64;
        let (nw, nh) = (((w as f64 * k).round() as u32).max(1), 128u32);
        out_size = (nw, nh);
        let small = if (w, h) == (nw, nh) { rgba } else { crate::win::resize(&rgba, w, h, nw, nh, false) };
        write_png(&dest.join(format!("s{i}.png")), nw, nh, &small)?;
        let (mut l, mut t, mut r, mut b) = (nw as i64, nh as i64, -1i64, -1i64);
        for y in 0..nh as i64 { for x in 0..nw as i64 {
            if small[((y * nw as i64 + x) * 4 + 3) as usize] > 40 { l = l.min(x); t = t.min(y); r = r.max(x); b = b.max(y); }
        } }
        if r >= 0 { bounds.insert(i as i64, (l, t, r + 1, b + 1)); }
    }

    let mut anims: HashMap<String, Vec<Frame>> = HashMap::new();
    for a in anim.get("animations").map(|v| v.arr()).unwrap_or(&[]) {
        let Some(key) = a.get("key").and_then(|v| v.str()) else { continue };
        let frames: Vec<Frame> = a.get("frames").map(|v| v.arr()).unwrap_or(&[]).iter().map(|f| Frame {
            sprite: f.get("sprite").and_then(|v| v.num()).unwrap_or(0.0) as i64,
            dx: f.get("dx").and_then(|v| v.num()).unwrap_or(0.0),
            dy: f.get("dy").and_then(|v| v.num()).unwrap_or(0.0),
            ticks: f.get("durationTicks").and_then(|v| v.num()).unwrap_or(10.0),
        }).collect();
        if !frames.is_empty() { anims.insert(key.to_string(), frames); }
    }
    let find = |names: &[&str]| -> Vec<Frame> {
        names.iter().find_map(|n| anims.get(&format!("{n}_left")).or_else(|| anims.get(*n)).cloned())
            .or_else(|| anims.get("stand_left").or_else(|| anims.get("stand")).cloned())
            .unwrap_or_else(|| vec![Frame { sprite: 0, dx: 0.0, dy: 0.0, ticks: 250.0 }])
    };
    let (w, h) = (out_size.0 as f64, out_size.1 as f64);
    let floor = ((w / 2.0).round() as i64, h as i64);
    // classic art leaves room around wall/ceiling poses; art that fills the frame needs the anchor on
    // the drawing's edge instead: the left edge touches the wall, the top edge touches the ceiling.
    // One anchor per animation (the extreme over its frames) keeps the pet from jittering.
    let wall_anchor = |frames: &[Frame]| -> (i64, i64) {
        let l = frames.iter().filter_map(|f| bounds.get(&f.sprite)).map(|b| b.0).min().unwrap_or(floor.0);
        (l.min(floor.0), floor.1)
    };
    let ceiling_anchor = |frames: &[Frame]| -> (i64, i64) {
        let t = frames.iter().filter_map(|f| bounds.get(&f.sprite)).map(|b| b.1).min().unwrap_or(0);
        (floor.0, t.max(0))
    };
    // `speed`: velocity multiplier, `slow`: duration divisor, `still`: drop velocity (physics moves it)
    let poses = |frames: &[Frame], anchor: (i64, i64), speed: f64, slow: f64, still: bool| -> String {
        frames.iter().map(|f| {
            let (vx, vy) = if still { (0.0, 0.0) } else { (f.dx * speed, f.dy * speed) };
            format!("\n\t\t\t\t<Pose Image=\"/s{}.png\" ImageAnchor=\"{},{}\" Velocity=\"{},{}\" Duration=\"{}\" />",
                f.sprite, anchor.0, anchor.1, vx.round(), vy.round(), ((f.ticks / slow).round() as i64).max(1))
        }).collect::<String>() + "\n\t\t\t"
    };
    let climb = find(&["climb"]);
    let descend = anims.get("descend_left").cloned()
        .unwrap_or_else(|| climb.iter().map(|f| Frame { dy: -f.dy, ..*f }).collect());
    let ceil = find(&["climb_ceiling"]);
    let drag = find(&["drag"]);
    let fill: Vec<(&str, String)> = vec![
        ("stand", poses(&find(&["stand"]), floor, 1.0, 1.0, false)),
        ("walk", poses(&find(&["walk"]), floor, 1.0, 1.0, false)),
        ("run", poses(&find(&["walk"]), floor, 2.0, 2.0, false)),
        ("dash", poses(&find(&["walk"]), floor, 4.0, 3.0, false)),
        ("sit", poses(&find(&["sit"]), floor, 1.0, 1.0, false)),
        ("sit_down", poses(&find(&["sit_down", "sit"]), floor, 1.0, 1.0, false)),
        ("sit_look_up", poses(&find(&["sit_look_up", "sit"]), floor, 1.0, 1.0, false)),
        ("sit_dangle", poses(&find(&["sit_dangle", "sit"]), floor, 1.0, 1.0, false)),
        ("sprawl", poses(&find(&["sprawl"]), floor, 1.0, 1.0, false)),
        ("creep", poses(&find(&["creep", "walk"]), floor, 1.0, 1.0, false)),
        ("wink", poses(&find(&["wink"]), floor, 1.0, 1.0, false)),
        ("tripping", poses(&find(&["tripping"]), floor, 1.0, 1.0, false)),
        ("bounce", poses(&find(&["bounce"]), floor, 1.0, 1.0, false)),
        ("grab_wall", poses(&climb[..1], wall_anchor(&climb), 1.0, 1.0, true)),
        ("climb_up", poses(&climb, wall_anchor(&climb), 1.0, 1.0, false)),
        ("climb_down", poses(&descend, wall_anchor(&climb), 1.0, 1.0, false)),
        ("grab_ceiling", poses(&ceil[..1], ceiling_anchor(&ceil), 1.0, 1.0, true)),
        ("ceiling", poses(&ceil, ceiling_anchor(&ceil), 1.0, 1.0, false)),
        ("fall", poses(&find(&["fall"]), floor, 1.0, 1.0, true)),
        ("jump", poses(&find(&["jump", "fling", "fall"]), floor, 1.0, 1.0, true)),
        ("jump_off", poses(&find(&["jump_up", "jump", "fall"]), floor, 1.0, 1.0, false)),
        ("drag", poses(&drag, floor, 1.0, 1.0, true)),
        ("resist", poses(&[drag.clone(), drag.clone(), drag.clone()].concat(), floor, 1.0, 1.0, true)),
        ("drag_offset", ((100.0 * h / 128.0).round() as i64).to_string()),
    ];
    let mut actions = include_str!("../assets/template/actions.xml").to_string();
    for (k, v) in &fill { actions = actions.replace(&format!("{{{{{k}}}}}"), v); }
    fs::write(dest.join("conf").join("actions.xml"), actions).map_err(|e| e.to_string())?;
    fs::write(dest.join("conf").join("behaviors.xml"), include_str!("../assets/template/behaviors.xml")).map_err(|e| e.to_string())?;
    Ok(())
}
