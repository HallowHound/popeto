//! LAN visits: pets walk off your screen and show up on another PC running Popeto on the same network.
//! Popetos find each other with UDP broadcasts (plus addresses you list). A visiting pet the other PC
//! doesn't have is sent over TCP as a zip, kept in `visitors\` and loaded only for the visit.
//! Only pets that are visiting right now can be downloaded; received files are images, xml and wav only.

use crate::expr::random;
use crate::i18n::tf;
use crate::pranks::{pick, ACT_WALK};
use crate::App;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const PORT: u16 = 47811;
const MAGIC: &str = "POPETO1";
const MAX_ZIP: u64 = 40 << 20;

pub struct Peer { pub id: u64, pub name: String, pub addr: SocketAddr, seen: Instant }

enum NetEv {
    Peer(u64, String, SocketAddr),
    Msg(SocketAddr, Vec<String>),
    /// A visitor's pet arrived over TCP (visit id, ok).
    Fetched(u64, bool),
    /// You answered "add this pet?" (gift id, yes).
    GiftAnswer(u64, bool),
    /// A gift finished downloading (gift id, ok).
    GiftFetched(u64, bool),
}

/// Copies of our pets offered to another PC, waiting for its answer.
struct GiftOut { packs: Vec<String>, to: SocketAddr, deadline: u64 }

/// Pets another PC offers us (only the ones we don't have yet).
struct GiftIn { gid: u64, from: SocketAddr, packs: Vec<String>, origin: String }

/// Pet names in a GIFT packet, '/'-separated (pet folder names can't contain '/').
/// Kept well under the 2 KB UDP buffer even in Cyrillic; bigger gifts go as several offers.
const GIFT_BYTES: usize = 1200;

/// Our pet away on a visit.
struct Away { pack: String, peer: SocketAddr, side: char, welcome_by: u64, welcomed: bool, deadline: u64, msg: Option<String> }

/// Someone else's pet visiting us.
struct Visitor {
    vid: u64, origin: SocketAddr, enter: char, until: u64, name: String, tries: u32,
    /// Hello bubble and message, given once the guest stands on screen (a new pet first falls a bit).
    greet: Option<(String, Option<String>)>, arrived: u64,
}

/// A pet walking to the screen edge to leave.
struct Leave { side: char, edge: i32, to: Option<SocketAddr>, ticks: u32, msg: Option<String> }

struct Arrival { from: SocketAddr, origin: String, pack: String, side: char, name: String, msg: Option<String> }

pub struct Lan {
    id: u64,
    stop: Arc<AtomicBool>,
    cmd: Sender<(SocketAddr, String)>,
    ev: Receiver<NetEv>,
    ev_tx: Sender<NetEv>,
    /// Packs that may be downloaded right now (our pets on a visit).
    serve: Arc<Mutex<HashSet<String>>>,
    pub peers: Vec<Peer>,
    away: HashMap<u64, Away>,
    visitors: HashMap<u32, Visitor>,
    leaving: HashMap<u32, Leave>,
    arriving: HashMap<u64, Arrival>,
    seen_vids: HashSet<u64>,
    /// Packs loaded from visitors\ for a visit (not your own pets).
    pub visitor_packs: HashSet<String>,
    next_auto: u64,
    pub error: Option<String>,
    gifts_out: HashMap<u64, GiftOut>,
    /// Offers waiting for the question box (one at a time), and the one being asked or downloaded.
    gifts_in: Vec<GiftIn>,
    gift_now: Option<GiftIn>,
}

impl Drop for Lan { fn drop(&mut self) { self.stop.store(true, Ordering::Relaxed); } }

fn clean(s: &str) -> String { s.replace(['|', '\n', '\r'], " ").chars().take(60).collect() }

/// Longest message a pet carries (fits a UDP packet even in Cyrillic).
pub const MAX_MSG: usize = 300;

/// Message text inside a `|`-separated packet.
fn enc(s: &str) -> String {
    s.chars().take(MAX_MSG).filter(|&c| c != '\r').map(|c| match c { '%' => "%25".into(), '|' => "%7C".into(), '\n' => "%0A".into(), c => c.to_string() }).collect()
}
fn dec(s: &str) -> String { s.replace("%0A", "\n").replace("%7C", "|").replace("%25", "%") }

fn opposite(c: char) -> char { if c == 'L' { 'R' } else { 'L' } }

impl Lan {
    pub fn start(root: &Path, name: String, manual: Vec<String>) -> Lan {
        let id = (random() * 1e15) as u64 + 1;
        let stop = Arc::new(AtomicBool::new(false));
        let (cmd, cmd_rx) = channel::<(SocketAddr, String)>();
        let (ev_tx, ev) = channel::<NetEv>();
        let serve = Arc::new(Mutex::new(HashSet::new()));
        let mut lan = Lan {
            id, stop: stop.clone(), cmd, ev, ev_tx: ev_tx.clone(), serve: serve.clone(), peers: vec![], away: HashMap::new(),
            visitors: HashMap::new(), leaving: HashMap::new(), arriving: HashMap::new(), seen_vids: HashSet::new(),
            visitor_packs: HashSet::new(), next_auto: 0, error: None,
            gifts_out: HashMap::new(), gifts_in: vec![], gift_now: None,
        };
        let udp = match UdpSocket::bind(("0.0.0.0", PORT)) { Ok(u) => u, Err(e) => { lan.error = Some(format!("UDP {PORT}: {e}")); return lan; } };
        let _ = udp.set_broadcast(true);
        let _ = udp.set_read_timeout(Some(Duration::from_millis(200)));
        match TcpListener::bind(("0.0.0.0", PORT)) {
            Ok(tcp) => {
                let _ = tcp.set_nonblocking(true);
                let (root, stop, serve) = (root.to_path_buf(), stop.clone(), serve.clone());
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        match tcp.accept() {
                            Ok((s, _)) => { let (root, serve) = (root.clone(), serve.clone()); std::thread::spawn(move || { let _ = serve_pack(s, &root, &serve); }); }
                            Err(_) => std::thread::sleep(Duration::from_millis(150)),
                        }
                    }
                });
            }
            Err(e) => lan.error = Some(format!("TCP {PORT}: {e}")),
        }
        let name = clean(&name);
        std::thread::spawn(move || {
            let mut last_hello = Instant::now() - Duration::from_secs(10);
            let mut buf = [0u8; 2048];
            while !stop.load(Ordering::Relaxed) {
                if last_hello.elapsed() > Duration::from_secs(3) {
                    last_hello = Instant::now();
                    let hello = format!("{MAGIC}|HELLO|{id}|{name}");
                    let _ = udp.send_to(hello.as_bytes(), ("255.255.255.255", PORT));
                    for m in &manual {
                        let host = if m.contains(':') { m.clone() } else { format!("{m}:{PORT}") };
                        if let Ok(addrs) = host.to_socket_addrs() { for a in addrs.filter(|a| a.is_ipv4()) { let _ = udp.send_to(hello.as_bytes(), a); } }
                    }
                }
                while let Ok((to, msg)) = cmd_rx.try_recv() { let _ = udp.send_to(format!("{MAGIC}|{msg}").as_bytes(), to); }
                let Ok((n, from)) = udp.recv_from(&mut buf) else { continue };
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                let parts: Vec<String> = text.split('|').map(|s| s.to_string()).collect();
                if parts.len() < 3 || parts[0] != MAGIC { continue; }
                let sender: u64 = parts[2].parse().unwrap_or(0);
                if sender == id || sender == 0 { continue; }
                if parts[1] == "HELLO" {
                    let _ = ev_tx.send(NetEv::Peer(sender, parts.get(3).cloned().unwrap_or_default(), from));
                } else {
                    let _ = ev_tx.send(NetEv::Msg(from, parts[1..].to_vec()));
                }
            }
        });
        lan
    }

    fn send(&self, to: SocketAddr, msg: String) { let _ = self.cmd.send((to, msg)); }
    pub fn peer_name(&self, a: SocketAddr) -> String {
        self.peers.iter().find(|p| p.addr.ip() == a.ip()).map(|p| p.name.clone()).unwrap_or_else(|| a.ip().to_string())
    }
    pub fn is_visitor(&self, pet: u32) -> bool { self.visitors.contains_key(&pet) }
    /// "Name (from PC)" for a visiting pet.
    pub fn visitor_label(&self, pet: u32, pack: &str) -> Option<String> {
        self.visitors.get(&pet).map(|v| tf("{} (from {})", &[&if v.name.trim().is_empty() { pack } else { v.name.as_str() }, &self.peer_name(v.origin)]))
    }
    /// Send a visitor home now.
    pub fn send_home(&mut self, pet: u32) { if let Some(v) = self.visitors.get_mut(&pet) { v.until = 0; } }
    pub fn away_count(&self) -> usize { self.away.len() }
    /// Walking off the screen or just arrived: other things leave this pet alone.
    pub fn busy(&self, pet: u32) -> bool { self.leaving.contains_key(&pet) }
}

/// TCP: "GET <pack>\n" -> 8-byte length + zip of that pet (only while it is visiting).
fn serve_pack(mut s: TcpStream, root: &Path, serve: &Mutex<HashSet<String>>) -> std::io::Result<()> {
    // accepted sockets inherit the listener's non-blocking mode on Windows
    s.set_nonblocking(false)?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut line = Vec::new();
    let mut b = [0u8; 1];
    while line.len() < 200 && s.read(&mut b)? == 1 && b[0] != b'\n' { line.push(b[0]); }
    let line = String::from_utf8_lossy(&line).to_string();
    let pack = line.strip_prefix("GET ").unwrap_or("").trim().to_string();
    let ok = serve.lock().map(|s| s.contains(&pack)).unwrap_or(false);
    let data = if ok { zip_pack(root, &pack).unwrap_or_default() } else { vec![] };
    s.write_all(&(data.len() as u64).to_le_bytes())?;
    s.write_all(&data)
}

fn zip_pack(root: &Path, pack: &str) -> Option<Vec<u8>> {
    let dir = root.join("img").join(pack);
    if !dir.is_dir() { return None; }
    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut out);
        let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut stack = vec![dir.clone()];
        let mut total = 0u64;
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).ok()?.flatten() {
                let p = e.path();
                if p.is_dir() { stack.push(p); continue; }
                if !allowed(&p) { continue; }
                let rel = p.strip_prefix(&dir).ok()?.to_string_lossy().replace('\\', "/");
                let bytes = std::fs::read(&p).ok()?;
                total += bytes.len() as u64;
                if total > MAX_ZIP { return None; }
                z.start_file(rel, opt).ok()?;
                z.write_all(&bytes).ok()?;
            }
        }
        // conf kept outside the pet folder (old Shimeji-ee layout): send the files actually used
        for f in ["actions.xml", "behaviors.xml"] {
            if dir.join("conf").join(f).exists() { continue; }
            if let Some(p) = crate::config::find_conf(root, &dir, pack, f) {
                z.start_file(format!("conf/{}", p.file_name()?.to_string_lossy()), opt).ok()?;
                z.write_all(&std::fs::read(&p).ok()?).ok()?;
            }
        }
        z.finish().ok()?;
    }
    Some(out.into_inner())
}

fn allowed(p: &Path) -> bool {
    p.extension().map(|x| ["png", "xml", "wav"].iter().any(|e| x.eq_ignore_ascii_case(e))).unwrap_or(false)
}

/// Download a visiting pet into visitors\<pack>.
fn fetch_pack(from: SocketAddr, pack: &str, dest: &Path) -> Result<(), String> {
    let mut s = TcpStream::connect_timeout(&SocketAddr::new(from.ip(), PORT), Duration::from_secs(4)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(20))).map_err(|e| e.to_string())?;
    s.write_all(format!("GET {pack}\n").as_bytes()).map_err(|e| e.to_string())?;
    let mut len = [0u8; 8];
    s.read_exact(&mut len).map_err(|e| e.to_string())?;
    let len = u64::from_le_bytes(len);
    if len == 0 || len > MAX_ZIP { return Err("refused".into()); }
    let mut data = Vec::new();
    s.take(len).read_to_end(&mut data).map_err(|e| e.to_string())?;
    unpack(data, dest)
}

fn unpack(data: Vec<u8>, dest: &Path) -> Result<(), String> {
    let mut a = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(dest);
    let mut total = 0u64;
    for i in 0..a.len() {
        let mut e = a.by_index(i).map_err(|e| e.to_string())?;
        // enclosed_name rejects absolute paths and "..", so nothing lands outside visitors\<pack>
        let Some(rel) = e.enclosed_name() else { continue };
        let out = dest.join(rel);
        if e.is_dir() || !allowed(&out) { continue; }
        total += e.size();
        if total > MAX_ZIP { return Err("too big".into()); }
        if let Some(p) = out.parent() { std::fs::create_dir_all(p).map_err(|e| e.to_string())?; }
        let mut f = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut e, &mut f).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Where received pets wait before they are added to img\.
fn gift_dir(root: &Path) -> PathBuf { root.join("visitors").join(".gifts") }

fn valid_pack_name(n: &str) -> bool {
    !n.is_empty() && n.len() < 100 && !n.contains(['/', '\\', ':']) && n != "." && n != ".." && !n.eq_ignore_ascii_case("unused")
}

// ---------------------------------------------------------------- app side

impl App {
    /// Start or stop networking to follow the settings.
    pub fn configure_lan(&mut self) {
        let l = &self.settings.lan;
        if !l.on { self.lan_send_all_home(); self.lan = None; return; }
        if self.lan.is_none() {
            let name = if l.name.trim().is_empty() { std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into()) } else { l.name.clone() };
            let lan = Lan::start(&self.root, name, l.peers.clone());
            if let Some(e) = &lan.error { self.log.push(format!("LAN: {e}")); }
            self.lan = Some(lan);
        }
    }

    fn lan_dbg(&mut self, s: &str) { if self.settings.debug { self.log.push(format!("[lan t={}] {s}", self.tick_n)); } }

    /// Edge x of the outermost monitor on that side, and its floor.
    fn outer_edge(&self, side: char) -> (i32, i32, i32) {
        let m = if side == 'L' { self.world.monitors.iter().min_by_key(|m| m.screen.l) } else { self.world.monitors.iter().max_by_key(|m| m.screen.r) };
        let w = m.map(|m| m.work).unwrap_or_default();
        if side == 'L' { (w.l, w.b, 1) } else { (w.r, w.b, -1) }
    }

    /// Send one of our pets to visit `to` (None = any PC that's around), optionally carrying a message.
    pub fn go_visit(&mut self, i: usize, to: Option<SocketAddr>, msg: Option<String>) {
        let Some(lan) = &self.lan else { return };
        let to = to.or_else(|| pick(&lan.peers.iter().map(|p| p.addr).collect::<Vec<_>>()));
        let Some(to) = to else { return };
        let id = self.pets[i].m.id;
        if lan.visitors.contains_key(&id) { return; }
        let w = self.world.monitors[self.pets[i].m.st.monitor.min(self.world.monitors.len() - 1)].work;
        let x = self.pets[i].m.st.anchor.0;
        let side = if x - w.l < w.r - x { 'L' } else { 'R' };
        let edge = if side == 'L' { w.l } else { w.r };
        let es = edge.to_string();
        let ok = self.run_on(i, &ACT_WALK, &[("TargetX", &es)], "#lan-leave");
        self.lan_dbg(&format!("pet {id} heads to the {side} edge to visit {to}: {ok}"));
        if ok {
            self.say_always(id, "visit");
            self.lan.as_mut().unwrap().leaving.insert(id, Leave { side, edge, to: Some(to), ticks: 0, msg });
        }
    }

    /// Spawn a pet walking in from the outer edge on `side`. Returns its id.
    fn walk_in(&mut self, pack: usize, side: char) -> Option<u32> {
        let (x, y, dir) = self.outer_edge(side);
        let (u, mx) = (self.settings.unique_pets, self.settings.max_pets);
        self.settings.unique_pets = false;
        self.settings.max_pets = usize::MAX;
        let before = self.pets.len();
        self.spawn(pack, Some((x, y)), dir > 0, None);
        self.settings.unique_pets = u;
        self.settings.max_pets = mx;
        if self.pets.len() == before { return None; }
        let i = self.pets.len() - 1;
        let tx = (x + dir * (120.0 + random() * 200.0) as i32 * self.scale.max(0.5) as i32).to_string();
        self.run_on(i, &ACT_WALK, &[("TargetX", &tx)], "#lan-arrive");
        Some(self.pets[i].m.id)
    }

    pub fn lan_tick(&mut self) {
        let Some(mut lan) = self.lan.take() else { return };
        let mut gifts_to_add: Vec<GiftIn> = vec![];
        let t = self.tick_n;
        // network events
        while let Ok(e) = lan.ev.try_recv() {
            match e {
                NetEv::Peer(id, name, addr) => {
                    if let Some(p) = lan.peers.iter_mut().find(|p| p.id == id) { p.addr = addr; p.name = name; p.seen = Instant::now(); }
                    else { self.lan_dbg(&format!("found {name} at {addr}")); lan.peers.push(Peer { id, name, addr, seen: Instant::now() }); }
                }
                NetEv::Msg(from, m) => self.lan_msg(&mut lan, from, &m),
                NetEv::Fetched(vid, ok) => {
                    let Some(a) = lan.arriving.remove(&vid) else { continue };
                    if ok { self.arrive(&mut lan, vid, a); } else {
                        self.lan_dbg(&format!("could not get {} from {}", a.pack, a.from));
                        lan.send(a.from, format!("RETURN|{}|{vid}|{}", lan.id, opposite(a.side)));
                    }
                }
                NetEv::GiftAnswer(gid, yes) => {
                    let Some(g) = lan.gift_now.as_ref().filter(|g| g.gid == gid) else { continue };
                    if !yes {
                        lan.send(g.from, format!("GIFTREPLY|{}|{gid}|no", lan.id));
                        lan.gift_now = None;
                        continue;
                    }
                    let (from, packs, dir) = (g.from, g.packs.clone(), gift_dir(&self.root));
                    let tx = lan.ev_tx.clone();
                    std::thread::spawn(move || {
                        let ok = packs.iter().map(|p| fetch_pack(from, p, &dir.join(p)).is_ok()).fold(false, |a, b| a | b);
                        let _ = tx.send(NetEv::GiftFetched(gid, ok));
                    });
                }
                NetEv::GiftFetched(gid, ok) => {
                    let Some(g) = lan.gift_now.take().filter(|g| g.gid == gid) else { continue };
                    // the answer goes out once the pet is really added (add_gift)
                    if ok { gifts_to_add.push(g); } else {
                        lan.send(g.from, format!("GIFTREPLY|{}|{gid}|fail", lan.id));
                        self.tell(&tf("Couldn't get {} from {}", &[&g.packs.join(", "), &g.origin]));
                    }
                }
            }
        }
        // offers: ask one at a time, when you can see it
        if lan.gift_now.is_none() && !lan.gifts_in.is_empty() && self.fs_settled.is_none() && !self.life.away {
            let g = lan.gifts_in.remove(0);
            let q = if g.packs.len() == 1 {
                tf("{} wants to give you a pet: {}.\n\nAdd it to your pets?", &[&g.origin, &g.packs[0]])
            } else {
                tf("{} wants to give you {} pets: {}.\n\nAdd them to your pets?", &[&g.origin, &g.packs.len(), &g.packs.join(", ")])
            };
            let (tx, gid) = (lan.ev_tx.clone(), g.gid);
            lan.gift_now = Some(g);
            std::thread::spawn(move || { let _ = tx.send(NetEv::GiftAnswer(gid, crate::win::ask_yes_no(&q))); });
        }
        // our offers nobody answered
        let stale: Vec<u64> = lan.gifts_out.iter().filter(|(_, g)| t > g.deadline).map(|(k, _)| *k).collect();
        for gid in stale {
            let g = lan.gifts_out.remove(&gid).unwrap();
            for p in &g.packs { self.stop_serving_gift(&mut lan, p); }
            self.tell(&tf("{} didn't answer, {} stays only here", &[&lan.peer_name(g.to), &g.packs.join(", ")]));
        }
        lan.peers.retain(|p| p.seen.elapsed() < Duration::from_secs(12));

        // our pets walking off the screen
        let ids: Vec<u32> = lan.leaving.keys().copied().collect();
        for id in ids {
            let Some(i) = self.pet_index(id) else { lan.leaving.remove(&id); continue };
            let l = lan.leaving.get_mut(&id).unwrap();
            l.ticks += 1;
            let a = self.pets[i].m.st.anchor.0;
            let off_track = self.pets[i].m.beh != "#lan-leave" && (a - l.edge).abs() > 8;
            let gone_wrong = self.pets[i].m.st.dragging || l.ticks > 1600;
            if off_track && !gone_wrong {
                // fell off a window or tripped: keep walking once it stands again
                let edge = l.edge;
                if self.pets[i].m.beh != "Fall" && self.floor_of(i) != crate::env::BorderRef::NotOn {
                    let es = edge.to_string();
                    if !self.run_on(i, &ACT_WALK, &[("TargetX", &es)], "#lan-leave") { lan.leaving.get_mut(&id).unwrap().ticks += 100; }
                }
                continue;
            }
            if gone_wrong {
                lan.leaving.remove(&id);
                // a guest that got interrupted tries again a bit later
                if let Some(v) = lan.visitors.get_mut(&id) { v.until = t + 200; } self.lan_dbg(&format!("pet {id} stopped leaving at x={a} ({})", self.pets[i].m.beh)); continue; }
            if (a - l.edge).abs() > 8 { continue; }
            let l = lan.leaving.remove(&id).unwrap();
            let pack = self.packs[self.pets[i].m.pack].name.clone();
            match l.to {
                Some(to) => {
                    let vid = (random() * 1e15) as u64 + 1;
                    lan.serve.lock().unwrap().insert(pack.clone());
                    let mine = self.settings.names.get(&pack).cloned().unwrap_or_default();
                    let msg = l.msg.as_deref().map(enc).unwrap_or_default();
                    for _ in 0..2 { lan.send(to, format!("VISIT|{}|{vid}|{}|{}|{}|{msg}", lan.id, clean(&pack), opposite(l.side), clean(&mine))); }
                    let mins = self.settings.lan.minutes.max(1) as u64;
                    lan.away.insert(vid, Away { pack, peer: to, side: l.side, welcome_by: t + 200, welcomed: false, deadline: t + (mins + 3) * 60 * 40, msg: l.msg });
                    self.lan_dbg(&format!("pet {id} left for {to} (visit {vid})"));
                }
                None => {
                    // a visitor going home
                    if let Some(v) = lan.visitors.remove(&id) {
                        lan.send(v.origin, format!("RETURN|{}|{}|{}", lan.id, v.vid, opposite(l.side)));
                        self.lan_dbg(&format!("visitor {id} went home"));
                    }
                }
            }
            self.remove(i);
        }

        // visitors: time to go home; vanished ones (dismissed, reloaded) go home at once
        let vis: Vec<u32> = lan.visitors.keys().copied().collect();
        for id in vis {
            match self.pet_index(id) {
                None => {
                    let v = lan.visitors.remove(&id).unwrap();
                    self.lan_dbg(&format!("visitor {id} vanished, sent home"));
                    lan.send(v.origin, format!("RETURN|{}|{}|{}", lan.id, v.vid, opposite(v.enter)));
                }
                Some(i) => {
                    self.greet_visitor(&mut lan, id, i);
                    let v = &lan.visitors[&id];
                    // asleep or not, guests leave on time; only wait while someone holds them or a prank uses them
                    let p = &self.pets[i];
                    if t < v.until || lan.leaving.contains_key(&id) || p.m.st.dragging || p.m.holding || self.pranks.busy(id) { continue; }
                    let (side, tries) = (v.enter, v.tries);
                    if tries >= 6 {
                        // can't walk there (stuck on a window, odd pack): just go
                        let v = lan.visitors.remove(&id).unwrap();
                        lan.send(v.origin, format!("RETURN|{}|{}|{}", lan.id, v.vid, opposite(v.enter)));
                        self.remove(i);
                        continue;
                    }
                    let v = lan.visitors.get_mut(&id).unwrap();
                    v.tries += 1;
                    v.until = t + 200;
                    let w = self.world.monitors[self.pets[i].m.st.monitor.min(self.world.monitors.len() - 1)].work;
                    let edge = if side == 'L' { w.l } else { w.r };
                    let es = edge.to_string();
                    if self.run_on(i, &ACT_WALK, &[("TargetX", &es)], "#lan-leave") {
                        if tries == 0 { self.say_always(id, "bye"); }
                        lan.leaving.insert(id, Leave { side, edge, to: None, ticks: 0, msg: None });
                    }
                }
            }
        }

        // our pets away: nobody answered, or they stayed too long
        let now_home: Vec<u64> = lan.away.iter().filter(|(_, a)| (!a.welcomed && t > a.welcome_by) || t > a.deadline).map(|(v, _)| *v).collect();
        for vid in now_home {
            let a = lan.away.remove(&vid).unwrap();
            self.lan_dbg(&format!("visit {vid}: {} comes home by itself", a.pack));
            let side = a.side;
            self.come_home(&mut lan, a, side);
        }

        // now and then a pet goes visiting by itself
        if self.settings.lan.auto && !lan.peers.is_empty() && (!self.life.away || self.settings.debug) && t >= lan.next_auto {
            if lan.next_auto != 0 {
                let own: Vec<usize> = (0..self.pets.len()).filter(|&i| !lan.visitors.contains_key(&self.pets[i].m.id) && !lan.leaving.contains_key(&self.pets[i].m.id) && (self.pet_free(i) || self.settings.debug)).collect();
                // leave at least one pet at home
                if own.len() > 1 { if let Some(i) = pick(&own) { self.lan = Some(lan); self.go_visit(i, None, None); lan = self.lan.take().unwrap(); } }
            }
            lan.next_auto = t + if self.settings.debug { 40 * 25 } else { 40 * 60 * (4 + (random() * 6.0) as u64) };
        }
        self.lan = Some(lan);
        for g in gifts_to_add { self.add_gift(g); }
    }

    /// Offer a copy of this pet to another PC; it is added there if they say yes.
    pub fn give_pet(&mut self, i: usize, to: SocketAddr) {
        let pack = self.packs[self.pets[i].m.pack].name.clone();
        let id = self.pets[i].m.id;
        if self.give_pets(vec![pack], to) {
            let peer = self.lan.as_ref().map(|l| l.peer_name(to)).unwrap_or_default();
            self.say_text(id, &tf("Asking {} if they want me...", &[&peer]), 160);
        }
    }

    /// Offer copies of these pets to another PC in one question (several, if the list is long).
    pub fn give_pets(&mut self, packs: Vec<String>, to: SocketAddr) -> bool {
        let t = self.tick_n;
        let Some(lan) = &mut self.lan else { return false };
        let packs: Vec<String> = packs.into_iter().filter(|p| !lan.visitor_packs.contains(p) && valid_pack_name(p) && !p.contains(['|', '\n', '\r'])).collect();
        if packs.is_empty() { return false; }
        let mut groups: Vec<Vec<String>> = vec![vec![]];
        for p in packs {
            let len: usize = groups.last().unwrap().iter().map(|q| q.len() + 1).sum();
            if len + p.len() > GIFT_BYTES && !groups.last().unwrap().is_empty() { groups.push(vec![]); }
            groups.last_mut().unwrap().push(p);
        }
        for g in groups {
            let gid = (random() * 1e15) as u64 + 1;
            { let mut s = lan.serve.lock().unwrap(); for p in &g { s.insert(p.clone()); } }
            for _ in 0..2 { lan.send(to, format!("GIFT|{}|{gid}|{}", lan.id, g.join("/"))); }
            // they may be away from the PC: wait up to 10 minutes
            lan.gifts_out.insert(gid, GiftOut { packs: g, to, deadline: t + 40 * 60 * 10 });
        }
        self.lan_dbg(&format!("offered pets to {to}"));
        true
    }

    /// A gift's files are no longer needed for download (unless the same pet is also out visiting).
    fn stop_serving_gift(&mut self, lan: &mut Lan, pack: &str) {
        let busy = lan.away.values().any(|a| a.pack == pack) || lan.gifts_out.values().any(|g| g.packs.iter().any(|p| p == pack));
        if !busy { lan.serve.lock().unwrap().remove(pack); }
    }

    /// Copy received pets into img\ and let a few of them walk in.
    fn add_gift(&mut self, g: GiftIn) {
        let (mut added, mut failed) = (vec![], vec![]);
        for p in &g.packs {
            let src = gift_dir(&self.root).join(p);
            if !src.is_dir() { continue; }
            let rep = crate::importer::import(&src, &self.root);
            let _ = std::fs::remove_dir_all(&src);
            added.extend(rep.added);
            failed.extend(rep.failed);
        }
        if let Some(lan) = &self.lan {
            let ans = if added.is_empty() { "fail".to_string() } else { format!("ok|{}", added.len()) };
            lan.send(g.from, format!("GIFTREPLY|{}|{}|{ans}", lan.id, g.gid));
        }
        if added.is_empty() {
            self.tell(&tf("Couldn't add {}: {}", &[&g.packs.join(", "), &failed.join("; ")]));
            return;
        }
        let rep = crate::importer::Report { added, failed };
        let want = self.settings.start_pets.trim().to_string();
        if !(want.is_empty() || want.eq_ignore_ascii_case("all")) {
            self.settings.start_pets = format!("{want},{}", rep.added.join(","));
            self.settings.save();
        }
        self.reload_keep();
        // a few walk in to say hello; the rest wait in the pet chooser
        for n in rep.added.iter().take(3) {
            if let Some(pk) = self.pack_index(n) {
                if let Some(id) = self.walk_in(pk, if random() < 0.5 { 'L' } else { 'R' }) {
                    self.say_text(id, &tf("Hi! {} sent me. I live here now", &[&g.origin]), 240);
                }
            }
        }
        if rep.added.len() > 3 { self.tell(&tf("{} new pets from {}: tray > Choose pets", &[&rep.added.len(), &g.origin])); }
        self.lan_dbg(&format!("gift {:?} from {} added", rep.added, g.origin));
    }

    fn come_home(&mut self, lan: &mut Lan, a: Away, side: char) {
        lan.serve.lock().unwrap().remove(&a.pack);
        if let Some(pk) = self.pack_index(&a.pack) {
            let Some(id) = self.walk_in(pk, side) else { return };
            match &a.msg {
                Some(_) if a.welcomed => self.say_text(id, crate::i18n::t("Message delivered!"), 160),
                // nobody took it: bring the message back to you
                Some(m) => { self.say_text(id, crate::i18n::t("Nobody was there, so I brought your message back"), 200); self.show_note(m); }
                None => self.say_always(id, "back"),
            }
        }
    }

    fn lan_msg(&mut self, lan: &mut Lan, from: SocketAddr, m: &[String]) {
        let vid: u64 = m.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);
        match m[0].as_str() {
            "VISIT" if m.len() >= 5 => {
                if !lan.seen_vids.insert(vid) { return; }
                let pack = m[3].clone();
                let side = if m[4] == "R" { 'R' } else { 'L' };
                if !valid_pack_name(&pack) { return; }
                lan.send(from, format!("WELCOME|{}|{vid}", lan.id));
                let origin = lan.peer_name(from);
                let msg = m.get(6).map(|s| dec(s)).filter(|s| !s.trim().is_empty());
                let a = Arrival { from, origin, pack: pack.clone(), side, name: m.get(5).cloned().unwrap_or_default(), msg };
                if self.pack_index(&pack).is_some() { self.arrive(lan, vid, a); return; }
                // fetch the pet first
                self.lan_dbg(&format!("fetching {pack} from {from}"));
                let dest = self.root.join("visitors").join(&pack);
                let tx = lan.ev_tx.clone();
                lan.arriving.insert(vid, a);
                std::thread::spawn(move || { let ok = fetch_pack(from, &pack, &dest).is_ok(); let _ = tx.send(NetEv::Fetched(vid, ok)); });
            }
            "GIFT" if m.len() >= 4 => {
                if !lan.seen_vids.insert(vid) { return; }
                let offered: Vec<String> = m[3].split('/').filter(|p| valid_pack_name(p)).map(|p| p.to_string()).collect();
                let packs: Vec<String> = offered.into_iter().filter(|p| !self.root.join("img").join(p).is_dir()).collect();
                let origin = lan.peer_name(from);
                if packs.is_empty() {
                    lan.send(from, format!("GIFTREPLY|{}|{vid}|have", lan.id));
                    return;
                }
                self.lan_dbg(&format!("{origin} offers {packs:?} (gift {vid})"));
                lan.gifts_in.push(GiftIn { gid: vid, from, packs, origin });
            }
            "GIFTREPLY" if m.len() >= 4 => {
                // only the PC we offered it to answers
                let Some(g) = lan.gifts_out.get(&vid).filter(|g| g.to.ip() == from.ip()) else { return };
                let (packs, peer) = (g.packs.clone(), lan.peer_name(from));
                lan.gifts_out.remove(&vid);
                for p in &packs { self.stop_serving_gift(lan, p); }
                let what = if packs.len() == 1 { packs[0].clone() } else { tf("{} pets", &[&packs.len()]) };
                let got = m.get(4).and_then(|n| n.parse::<usize>().ok()).unwrap_or(packs.len());
                self.tell(&match m[3].as_str() {
                    "ok" if packs.len() == 1 => tf("{} now lives on {} too!", &[&what, &peer]),
                    "ok" => tf("{} new pets now live on {}!", &[&got, &peer]),
                    "have" => tf("{} already has {}", &[&peer, &what]),
                    "no" => tf("{} said no thanks to {}", &[&peer, &what]),
                    _ => tf("Couldn't send {} to {}", &[&what, &peer]),
                });
            }
            "WELCOME" => { if let Some(a) = lan.away.get_mut(&vid).filter(|a| a.peer.ip() == from.ip()) { a.welcomed = true; } }
            "RETURN" if m.len() >= 4 => {
                // only the PC the pet went to can send it back
                if lan.away.get(&vid).map(|a| a.peer.ip() == from.ip()).unwrap_or(false) {
                    let a = lan.away.remove(&vid).unwrap();
                    let side = if m[3] == "R" { 'R' } else { 'L' };
                    self.lan_dbg(&format!("visit {vid}: {} is back", a.pack));
                    self.come_home(lan, a, side);
                }
            }
            _ => {}
        }
    }

    fn arrive(&mut self, lan: &mut Lan, vid: u64, a: Arrival) {
        let pk = match self.pack_index(&a.pack) {
            Some(pk) => pk,
            None => {
                let dir = self.root.join("visitors").join(&a.pack);
                match crate::config::load_pack(&self.root, &dir, self.scale) {
                    Ok((pack, _)) => {
                        self.bmps.push((0..pack.images.len()).map(|_| None).collect());
                        self.packs.push(pack);
                        lan.visitor_packs.insert(a.pack.clone());
                        self.packs.len() - 1
                    }
                    Err(e) => {
                        self.lan_dbg(&format!("visitor {} not loaded: {e}", a.pack));
                        lan.send(a.from, format!("RETURN|{}|{vid}|{}", lan.id, opposite(a.side)));
                        return;
                    }
                }
            }
        };
        let Some(id) = self.walk_in(pk, a.side) else {
            lan.send(a.from, format!("RETURN|{}|{vid}|{}", lan.id, opposite(a.side)));
            return;
        };
        let mins = self.settings.lan.minutes.max(1) as u64;
        let hi = if a.msg.is_some() { tf("A message from {}!", &[&a.origin]) } else { tf("Hi! I'm visiting from {}", &[&a.origin]) };
        lan.visitors.insert(id, Visitor {
            vid, origin: a.from, enter: a.side, until: self.tick_n + mins * 60 * 40, name: a.name.clone(), tries: 0,
            greet: Some((hi, a.msg.clone())), arrived: self.tick_n,
        });
        self.lan_dbg(&format!("visitor {} from {} arrived as pet {id}", a.pack, a.origin));
    }

    /// Say hello and hand over the message once the new guest is visible and on its feet.
    fn greet_visitor(&mut self, lan: &mut Lan, id: u32, i: usize) {
        let Some(v) = lan.visitors.get_mut(&id) else { return };
        if v.greet.is_none() { return; }
        let p = &self.pets[i];
        let ready = p.w.shown && p.m.beh != "Fall" && !p.m.st.dragging && self.floor_of(i) != crate::env::BorderRef::NotOn;
        // the pack's first Fall starts a tick or two after the spawn, so give it a moment
        let age = self.tick_n.saturating_sub(v.arrived);
        if age < 40 || (!ready && age < 400) { return; }
        let (hi, msg) = v.greet.take().unwrap();
        self.emote(id, crate::life::Emote::Heart);
        if let Some(m) = msg {
            self.lan_dbg(&format!("visitor {id} brings a message ({} chars)", m.chars().count()));
            self.carry_note(id, &m);
        }
        self.say_text(id, &hi, 200);
    }

    /// Visitors leave right away (Popeto closing, LAN switched off).
    pub fn lan_send_all_home(&mut self) {
        let Some(lan) = &mut self.lan else { return };
        let gone: Vec<Visitor> = lan.visitors.drain().map(|(_, v)| v).collect();
        if gone.is_empty() { return; }
        for v in gone { lan.send(v.origin, format!("RETURN|{}|{}|{}", lan.id, v.vid, opposite(v.enter))); }
        // give the network thread a moment to send
        std::thread::sleep(Duration::from_millis(300));
    }

    /// Visitor's pet becomes yours: copy it from visitors\ into img\.
    pub fn keep_visitor(&mut self, i: usize) {
        let pack = self.packs[self.pets[i].m.pack].name.clone();
        let dir: PathBuf = self.root.join("visitors").join(&pack);
        if dir.is_dir() { self.import_from(&dir); }
    }
}


#[cfg(test)]
mod tests {
    #[test]
    fn pack_survives_the_trip() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../dist/Popeto");
        let Some(pack) = crate::config::discover(&root).into_iter().next() else { return };
        let name = pack.file_name().unwrap().to_string_lossy().to_string();
        let data = super::zip_pack(&root, &name).expect("zip");
        let dest = std::env::temp_dir().join("popeto-lan-test").join(&name);
        super::unpack(data, &dest).expect("unpack");
        let (p, _) = crate::config::load_pack(&root, &dest, 1.0).expect("load");
        assert!(!p.images.is_empty() && !p.behaviors.is_empty());
        let _ = std::fs::remove_dir_all(dest.parent().unwrap());
    }

    #[test]
    fn message_survives_the_packet() {
        let m = "50% off | today\nПривет";
        let e = super::enc(m);
        assert!(!e.contains('|') && !e.contains('\n'));
        assert_eq!(super::dec(&e), m);
    }
}
