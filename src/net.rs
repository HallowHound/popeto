//! Network engine for LAN visits: one UDP port carries everything.
//!
//! - Finding each other: a hello every 3 s to 255.255.255.255, to every network card's own subnet broadcast
//!   (so a VPN doesn't swallow it), to listed addresses and to PCs already found. A direct hello is answered
//!   straight back, so listing a PC on one side is enough, also through NAT.
//! - Peers share the addresses of PCs they talk to, so one listed address can connect a whole office.
//! - Pet zips travel over the same socket in numbered pieces the receiver asks for (and asks again when some
//!   go missing). No TCP and no second port: once two PCs have said hello, Windows' firewall lets the
//!   answers in, even where nobody clicked "Allow".
//!
//! Only packs listed in `serve` (pets out visiting or being given) are sent, and only to PCs that said hello.

use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAGIC: &str = "POPETO2";
/// Payload bytes per piece: with the header it stays under a 1500-byte Ethernet frame.
const CHUNK: usize = 1100;
/// Pieces asked for at once.
const WINDOW: u32 = 48;
pub const MAX_ZIP: u64 = 40 << 20;
const HELLO_EVERY: Duration = Duration::from_secs(3);
const GOSSIP_EVERY: Duration = Duration::from_secs(15);
/// Gossiped addresses that never answered are forgotten after this.
const CANDIDATE_TTL: Duration = Duration::from_secs(600);
/// A download that makes no progress for this long fails.
const STALL: Duration = Duration::from_secs(15);

/// Tests: share of pieces the sender "loses", in percent.
#[cfg(test)]
static LOSS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub enum Ev {
    /// A PC said hello (its id, name, address).
    Peer(u64, String, SocketAddr),
    /// Any other message for the app: the `|`-separated fields after the magic.
    Msg(SocketAddr, Vec<String>),
    /// A download finished (transfer id, ok).
    Fetched(u64, bool),
    /// Connection test result: did anything answer from that address.
    Probe(SocketAddr, bool),
}

enum Cmd {
    Send(SocketAddr, String),
    Fetch { xid: u64, from: SocketAddr, pack: String, dest: PathBuf },
    Probe(SocketAddr),
}

pub struct Net {
    cmd: Sender<Cmd>,
    pub ev: Receiver<Ev>,
    stop: Arc<AtomicBool>,
    /// Packs that may be downloaded right now.
    pub serve: Arc<Mutex<HashSet<String>>>,
    pub error: Option<String>,
}

impl Drop for Net { fn drop(&mut self) { self.stop.store(true, Ordering::Relaxed); } }

impl Net {
    /// Listen on `port`; hellos and broadcasts go to `peer_port` (the same outside tests).
    pub fn start(root: &Path, id: u64, name: String, manual: Vec<String>, port: u16, peer_port: u16) -> Net {
        let (cmd, cmd_rx) = channel();
        let (ev_tx, ev) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let serve = Arc::new(Mutex::new(HashSet::new()));
        let mut net = Net { cmd, ev, stop: stop.clone(), serve: serve.clone(), error: None };
        let sock = match UdpSocket::bind(("0.0.0.0", port)) {
            Ok(s) => s,
            Err(e) => { net.error = Some(format!("UDP {port}: {e}")); return net; }
        };
        let _ = sock.set_broadcast(true);
        let _ = sock.set_read_timeout(Some(Duration::from_millis(15)));
        big_receive_buffer(&sock);
        let mut e = Engine {
            sock, id, name, root: root.to_path_buf(), peer_port, manual, serve, ev: ev_tx, stop,
            known: HashMap::new(), candidates: HashMap::new(), broadcasts: vec![], own: HashSet::new(),
            outs: HashMap::new(), ins: HashMap::new(), probes: HashMap::new(),
            zipped: channel(), unpacked: channel(),
            last_hello: Instant::now() - HELLO_EVERY * 2, last_gossip: Instant::now(), last_ifaces: Instant::now() - Duration::from_secs(3600),
        };
        std::thread::spawn(move || e.run(cmd_rx));
        net
    }

    pub fn send(&self, to: SocketAddr, msg: String) { let _ = self.cmd.send(Cmd::Send(to, msg)); }
    /// Download `pack` from `from` and unpack it into `dest`; ends with `Ev::Fetched(xid, ..)`.
    pub fn fetch(&self, xid: u64, from: SocketAddr, pack: &str, dest: PathBuf) { let _ = self.cmd.send(Cmd::Fetch { xid, from, pack: pack.into(), dest }); }
    /// Say hello to an address and report whether it answers within a few seconds.
    pub fn probe(&self, to: SocketAddr) { let _ = self.cmd.send(Cmd::Probe(to)); }
}

/// "office-pc", "192.168.1.20" or "10.0.0.5:47811" -> IPv4 addresses.
pub fn resolve(host: &str, port: u16) -> Vec<SocketAddr> {
    let host = host.trim();
    if host.is_empty() { return vec![]; }
    let full = if host.contains(':') { host.to_string() } else { format!("{host}:{port}") };
    full.to_socket_addrs().map(|a| a.filter(|a| a.is_ipv4()).collect()).unwrap_or_default()
}

/// Sends a piece's worth of data in one go without the OS dropping the rest.
fn big_receive_buffer(sock: &UdpSocket) {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{setsockopt, SOL_SOCKET, SO_RCVBUF, SO_SNDBUF};
    let size: i32 = 1 << 20;
    for opt in [SO_RCVBUF, SO_SNDBUF] {
        unsafe { setsockopt(sock.as_raw_socket() as usize, SOL_SOCKET as i32, opt as i32, &size as *const i32 as *const u8, 4); }
    }
}

/// IPv4 addresses of this PC's network cards that are up, with their prefix length (no loopback/link-local).
pub fn local_ipv4() -> Vec<(Ipv4Addr, u8)> {
    use windows_sys::Win32::NetworkManagement::IpHelper::*;
    use windows_sys::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};
    let mut out = vec![];
    let mut size: u32 = 32 * 1024;
    let mut buf: Vec<u64> = vec![0; size as usize / 8];
    unsafe {
        let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
        let mut r = GetAdaptersAddresses(AF_INET as u32, flags, std::ptr::null(), buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH, &mut size);
        if r == 111 /* ERROR_BUFFER_OVERFLOW */ {
            buf = vec![0; size as usize / 8 + 1];
            r = GetAdaptersAddresses(AF_INET as u32, flags, std::ptr::null(), buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH, &mut size);
        }
        if r != 0 { return out; }
        let mut a = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while !a.is_null() {
            let ad = &*a;
            if ad.OperStatus == 1 /* up */ && ad.IfType != 24 /* loopback */ {
                let mut u = ad.FirstUnicastAddress;
                while !u.is_null() {
                    let ua = &*u;
                    let sa = ua.Address.lpSockaddr as *const SOCKADDR_IN;
                    if !sa.is_null() && (*sa).sin_family == AF_INET {
                        let ip = Ipv4Addr::from(u32::from_be((*sa).sin_addr.S_un.S_addr));
                        if !ip.is_loopback() && !ip.is_link_local() { out.push((ip, ua.OnLinkPrefixLength)); }
                    }
                    u = ua.Next;
                }
            }
            a = ad.Next;
        }
    }
    out
}

fn subnet_broadcast(ip: Ipv4Addr, prefix: u8) -> Option<Ipv4Addr> {
    if prefix == 0 || prefix >= 31 { return None; }
    let mask = u32::MAX << (32 - prefix as u32);
    Some(Ipv4Addr::from(u32::from(ip) | !mask))
}

/// Someone we talk to.
struct Known { last: Instant }

/// A download in progress.
struct In {
    from: SocketAddr,
    pack: String,
    dest: PathBuf,
    asked: u32,
    next_ask: Instant,
    /// Total size once the sender told us.
    size: Option<usize>,
    pieces: Vec<Option<Vec<u8>>>,
    got: u32,
    /// Pieces requested so far (exclusive end).
    upto: u32,
    progress: Instant,
}

/// An upload: the zipped pack, kept while the other side asks for pieces.
struct Out { data: Arc<Vec<u8>>, last: Instant }

struct Engine {
    sock: UdpSocket,
    id: u64,
    name: String,
    root: PathBuf,
    peer_port: u16,
    manual: Vec<String>,
    serve: Arc<Mutex<HashSet<String>>>,
    ev: Sender<Ev>,
    stop: Arc<AtomicBool>,
    known: HashMap<SocketAddr, Known>,
    /// Addresses to keep saying hello to: gossiped ones (with when we heard of them).
    candidates: HashMap<SocketAddr, Instant>,
    broadcasts: Vec<SocketAddr>,
    /// Our own addresses, so gossip about ourselves is ignored.
    own: HashSet<SocketAddr>,
    outs: HashMap<(SocketAddr, u64), Out>,
    ins: HashMap<u64, In>,
    probes: HashMap<SocketAddr, Instant>,
    zipped: (Sender<(SocketAddr, u64, Option<Vec<u8>>)>, Receiver<(SocketAddr, u64, Option<Vec<u8>>)>),
    unpacked: (Sender<(u64, bool)>, Receiver<(u64, bool)>),
    last_hello: Instant,
    last_gossip: Instant,
    last_ifaces: Instant,
}

impl Engine {
    fn run(&mut self, cmds: Receiver<Cmd>) {
        let mut buf = vec![0u8; 2048];
        while !self.stop.load(Ordering::Relaxed) {
            self.timers();
            while let Ok(c) = cmds.try_recv() { self.command(c); }
            while let Ok((to, xid, data)) = self.zipped.1.try_recv() { self.zip_ready(to, xid, data); }
            while let Ok((xid, ok)) = self.unpacked.1.try_recv() { let _ = self.ev.send(Ev::Fetched(xid, ok)); }
            match self.sock.recv_from(&mut buf) {
                Ok((n, from)) => self.packet(&buf[..n], from),
                Err(_) => {}
            }
        }
    }

    fn send(&self, to: SocketAddr, text: &str) { let _ = self.sock.send_to(format!("{MAGIC}|{text}").as_bytes(), to); }

    fn hello(&self, to: SocketAddr) { self.send(to, &format!("HELLO|{}|{}", self.id, self.name)); }

    fn timers(&mut self) {
        let now = Instant::now();
        if now - self.last_ifaces > Duration::from_secs(60) {
            self.last_ifaces = now;
            let ifs = local_ipv4();
            let mut b: Vec<SocketAddr> = vec![SocketAddr::from((Ipv4Addr::BROADCAST, self.peer_port))];
            for (ip, prefix) in &ifs {
                if let Some(bc) = subnet_broadcast(*ip, *prefix) { b.push(SocketAddr::from((bc, self.peer_port))); }
                self.own.insert(SocketAddr::from((*ip, self.peer_port)));
            }
            b.dedup();
            self.broadcasts = b;
        }
        if now - self.last_hello > HELLO_EVERY {
            self.last_hello = now;
            let mut to: Vec<SocketAddr> = self.broadcasts.clone();
            for m in &self.manual { to.extend(resolve(m, self.peer_port)); }
            self.candidates.retain(|_, t| now - *t < CANDIDATE_TTL);
            to.extend(self.candidates.keys().copied());
            // known PCs get a direct hello too: it keeps the firewall's door open for their answers
            self.known.retain(|_, k| now - k.last < Duration::from_secs(30));
            to.extend(self.known.keys().copied());
            to.sort();
            to.dedup();
            for a in to { self.hello(a); }
        }
        if now - self.last_gossip > GOSSIP_EVERY && !self.known.is_empty() {
            self.last_gossip = now;
            let list: Vec<String> = self.known.iter().filter(|(_, k)| now - k.last < Duration::from_secs(15)).map(|(a, _)| a.to_string()).take(40).collect();
            for a in self.known.keys() { self.send(*a, &format!("PEERS|{}|{}", self.id, list.join(","))); }
        }
        // downloads: ask for the next pieces, or again for lost ones
        let ids: Vec<u64> = self.ins.keys().copied().collect();
        for xid in ids { self.drive(xid, now); }
        self.outs.retain(|_, o| now - o.last < Duration::from_secs(60));
        let done: Vec<SocketAddr> = self.probes.iter().filter(|(_, t)| now > **t).map(|(a, _)| *a).collect();
        for a in done { self.probes.remove(&a); let _ = self.ev.send(Ev::Probe(a, false)); }
    }

    fn command(&mut self, c: Cmd) {
        match c {
            Cmd::Send(to, msg) => self.send(to, &msg),
            Cmd::Fetch { xid, from, pack, dest } => {
                let now = Instant::now();
                self.ins.insert(xid, In { from, pack, dest, asked: 0, next_ask: now, size: None, pieces: vec![], got: 0, upto: 0, progress: now });
            }
            Cmd::Probe(to) => {
                self.probes.insert(to, Instant::now() + Duration::from_secs(4));
                self.hello(to);
            }
        }
    }

    /// Next step of a download.
    fn drive(&mut self, xid: u64, now: Instant) {
        let Some(d) = self.ins.get_mut(&xid) else { return };
        if now - d.progress > STALL {
            self.ins.remove(&xid);
            let _ = self.ev.send(Ev::Fetched(xid, false));
            return;
        }
        if now < d.next_ask { return; }
        match d.size {
            // still waiting for the size: ask again now and then (zipping a big pet takes a moment)
            None => {
                let (to, text) = (d.from, format!("GET|{}|{xid}|{}", self.id, d.pack));
                d.asked += 1;
                d.next_ask = now + Duration::from_millis(1000);
                self.send(to, &text);
            }
            // nothing arrived for a while: ask again from the first missing piece
            Some(_) => {
                let first = d.pieces.iter().position(|p| p.is_none()).unwrap_or(d.pieces.len()) as u32;
                d.upto = d.upto.max(first + WINDOW).min(d.pieces.len() as u32);
                d.next_ask = now + Duration::from_millis(300);
                let to = d.from;
                self.send(to, &format!("WANT|{}|{xid}|{first}|{WINDOW}", self.id));
            }
        }
    }

    fn packet(&mut self, data: &[u8], from: SocketAddr) {
        // pieces carry raw bytes after the fifth '|'
        if data.starts_with(format!("{MAGIC}|DATA|").as_bytes()) {
            let mut cut = 0;
            let mut bars = 0;
            for (i, b) in data.iter().enumerate() { if *b == b'|' { bars += 1; if bars == 5 { cut = i + 1; break; } } }
            if bars < 5 { return; }
            let head = String::from_utf8_lossy(&data[..cut - 1]).to_string();
            let f: Vec<&str> = head.split('|').collect();
            let (xid, seq) = (f[3].parse().unwrap_or(0u64), f[4].parse().unwrap_or(u32::MAX));
            self.piece(from, xid, seq, &data[cut..]);
            return;
        }
        let text = String::from_utf8_lossy(data).to_string();
        let f: Vec<String> = text.split('|').map(|s| s.to_string()).collect();
        if f.len() < 3 || f[0] != MAGIC { return; }
        let sender: u64 = f[2].parse().unwrap_or(0);
        if sender == self.id || sender == 0 { return; }
        let answered: Vec<SocketAddr> = self.probes.keys().filter(|a| a.ip() == from.ip()).copied().collect();
        for a in answered { self.probes.remove(&a); let _ = self.ev.send(Ev::Probe(a, true)); }
        let new = !self.known.contains_key(&from);
        if f[1] == "HELLO" || !new { self.known.insert(from, Known { last: Instant::now() }); }
        match f[1].as_str() {
            "HELLO" => {
                // answer a PC we didn't know straight away: it may not hear our broadcasts
                if new { self.hello(from); }
                self.candidates.remove(&from);
                let _ = self.ev.send(Ev::Peer(sender, f.get(3).cloned().unwrap_or_default(), from));
            }
            "PEERS" if f.len() >= 4 => {
                for a in f[3].split(',').filter_map(|s| s.parse::<SocketAddr>().ok()).take(40) {
                    if a.is_ipv4() && !self.known.contains_key(&a) && !self.own.contains(&a) && self.candidates.len() < 64 {
                        self.candidates.entry(a).or_insert_with(Instant::now);
                    }
                }
            }
            "GET" if f.len() >= 5 => self.get(from, f[3].parse().unwrap_or(0), &f[4..].join("|")),
            "WANT" if f.len() >= 6 => self.want(from, f[3].parse().unwrap_or(0), f[4].parse().unwrap_or(0), f[5].parse().unwrap_or(0)),
            "SIZE" if f.len() >= 5 => self.size(from, f[3].parse().unwrap_or(0), f[4].parse().unwrap_or(0)),
            "NOPE" if f.len() >= 4 => {
                let xid = f[3].parse().unwrap_or(0);
                if self.ins.get(&xid).map(|d| d.from.ip() == from.ip()).unwrap_or(false) {
                    self.ins.remove(&xid);
                    let _ = self.ev.send(Ev::Fetched(xid, false));
                }
            }
            "DONE" if f.len() >= 4 => { self.outs.remove(&(from, f[3].parse().unwrap_or(0))); }
            _ => { let _ = self.ev.send(Ev::Msg(from, f[1..].to_vec())); }
        }
    }

    /// Someone wants one of our packs: zip it (off this thread) if it may go out and they said hello.
    fn get(&mut self, from: SocketAddr, xid: u64, pack: &str) {
        if self.outs.contains_key(&(from, xid)) {
            let n = self.outs[&(from, xid)].data.len();
            self.send(from, &format!("SIZE|{}|{xid}|{n}", self.id));
            return;
        }
        let ok = self.known.contains_key(&from) && self.serve.lock().map(|s| s.contains(pack)).unwrap_or(false);
        if !ok { self.send(from, &format!("NOPE|{}|{xid}", self.id)); return; }
        // a placeholder so repeated GETs while zipping don't zip again
        self.outs.insert((from, xid), Out { data: Arc::new(vec![]), last: Instant::now() });
        let (root, pack, tx) = (self.root.clone(), pack.to_string(), self.zipped.0.clone());
        std::thread::spawn(move || { let _ = tx.send((from, xid, crate::lan::zip_pack(&root, &pack))); });
    }

    fn zip_ready(&mut self, to: SocketAddr, xid: u64, data: Option<Vec<u8>>) {
        match data {
            Some(d) if !d.is_empty() => {
                let n = d.len();
                self.outs.insert((to, xid), Out { data: Arc::new(d), last: Instant::now() });
                self.send(to, &format!("SIZE|{}|{xid}|{n}", self.id));
            }
            _ => { self.outs.remove(&(to, xid)); self.send(to, &format!("NOPE|{}|{xid}", self.id)); }
        }
    }

    fn want(&mut self, from: SocketAddr, xid: u64, start: u32, count: u32) {
        let Some(o) = self.outs.get_mut(&(from, xid)) else { return };
        if o.data.is_empty() { return; }
        o.last = Instant::now();
        let data = o.data.clone();
        let total = data.len().div_ceil(CHUNK) as u32;
        for seq in start..(start.saturating_add(count.min(WINDOW))).min(total) {
            #[cfg(test)]
            if (crate::expr::random() * 100.0) < LOSS.load(Ordering::Relaxed) as f64 { continue; }
            let a = seq as usize * CHUNK;
            let mut p = format!("{MAGIC}|DATA|{}|{xid}|{seq}|", self.id).into_bytes();
            p.extend_from_slice(&data[a..(a + CHUNK).min(data.len())]);
            let _ = self.sock.send_to(&p, from);
        }
    }

    fn size(&mut self, from: SocketAddr, xid: u64, n: u64) {
        let Some(d) = self.ins.get_mut(&xid) else { return };
        if d.from.ip() != from.ip() || d.size.is_some() { return; }
        if n == 0 || n > MAX_ZIP {
            self.ins.remove(&xid);
            let _ = self.ev.send(Ev::Fetched(xid, false));
            return;
        }
        // answers may come from another port than we asked (NAT): talk to where they come from
        d.from = from;
        d.size = Some(n as usize);
        d.pieces = vec![None; (n as usize).div_ceil(CHUNK)];
        d.progress = Instant::now();
        d.upto = WINDOW.min(d.pieces.len() as u32);
        d.next_ask = Instant::now() + Duration::from_millis(300);
        self.send(from, &format!("WANT|{}|{xid}|0|{WINDOW}", self.id));
    }

    fn piece(&mut self, from: SocketAddr, xid: u64, seq: u32, bytes: &[u8]) {
        let Some(d) = self.ins.get_mut(&xid) else { return };
        if d.from.ip() != from.ip() || seq as usize >= d.pieces.len() || d.pieces[seq as usize].is_some() { return; }
        let last = seq as usize == d.pieces.len() - 1;
        let want = if last { d.size.unwrap_or(0) - seq as usize * CHUNK } else { CHUNK };
        if bytes.len() != want { return; }
        d.pieces[seq as usize] = Some(bytes.to_vec());
        d.got += 1;
        d.progress = Instant::now();
        d.next_ask = Instant::now() + Duration::from_millis(300);
        let total = d.pieces.len() as u32;
        if d.got == total {
            let d = self.ins.remove(&xid).unwrap();
            self.send(from, &format!("DONE|{}|{xid}", self.id));
            let data: Vec<u8> = d.pieces.into_iter().flatten().flatten().collect();
            let tx = self.unpacked.0.clone();
            std::thread::spawn(move || { let ok = crate::lan::unpack(data, &d.dest).is_ok(); let _ = tx.send((xid, ok)); });
            return;
        }
        // keep the pipe full: ask for the next window when half of the asked ones are in
        let first = d.pieces.iter().position(|p| p.is_none()).unwrap_or(0) as u32;
        if d.upto < total && d.upto.saturating_sub(first) < WINDOW / 2 {
            let start = d.upto;
            d.upto = (d.upto + WINDOW).min(total);
            self.send(from, &format!("WANT|{}|{xid}|{start}|{WINDOW}", self.id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broadcast_of_a_subnet() {
        assert_eq!(subnet_broadcast(Ipv4Addr::new(192, 168, 1, 132), 24), Some(Ipv4Addr::new(192, 168, 1, 255)));
        assert_eq!(subnet_broadcast(Ipv4Addr::new(172, 16, 0, 193), 16), Some(Ipv4Addr::new(172, 16, 255, 255)));
        assert_eq!(subnet_broadcast(Ipv4Addr::new(10, 0, 0, 1), 32), None);
    }

    /// Two engines on this PC find each other through one listed address and move a pet between them.
    #[test]
    fn two_pcs_find_each_other_and_move_a_pet() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        let fake = std::env::temp_dir().join(format!("popeto-net-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&fake);
        std::fs::create_dir_all(fake.join("img")).unwrap();
        crate::importer::copy_dir(&root.join("pets").join("Popi"), &fake.join("img").join("Popi")).unwrap();
        // a: lists b's address; b lists nobody, so b only learns a from a's hello
        let a = Net::start(&fake, 11, "A".into(), vec!["127.0.0.1:47922".into()], 47921, 47922);
        let b = Net::start(&fake, 22, "B".into(), vec![], 47922, 47921);
        assert!(a.error.is_none() && b.error.is_none());
        let wait = |n: &Net, f: &dyn Fn(&Ev) -> bool| -> bool {
            let end = Instant::now() + Duration::from_secs(20);
            while Instant::now() < end {
                if let Ok(e) = n.ev.recv_timeout(Duration::from_millis(100)) { if f(&e) { return true; } }
            }
            false
        };
        assert!(wait(&b, &|e| matches!(e, Ev::Peer(11, _, _))), "b hears a");
        assert!(wait(&a, &|e| matches!(e, Ev::Peer(22, _, _))), "a hears b back");
        // not offered yet: refused
        let from_a = SocketAddr::from((Ipv4Addr::LOCALHOST, 47921));
        b.fetch(1, from_a, "Popi", fake.join("got1"));
        assert!(wait(&b, &|e| matches!(e, Ev::Fetched(1, false))), "refused while not served");
        a.serve.lock().unwrap().insert("Popi".into());
        b.fetch(2, from_a, "Popi", fake.join("got2").join("Popi"));
        assert!(wait(&b, &|e| matches!(e, Ev::Fetched(2, true))), "download works");
        let (p, _) = crate::config::load_pack(&fake, &fake.join("got2").join("Popi"), 1.0).expect("load");
        assert!(!p.images.is_empty());
        // a lossy network: every fifth piece goes missing and gets asked for again
        LOSS.store(20, Ordering::Relaxed);
        b.fetch(3, from_a, "Popi", fake.join("got3").join("Popi"));
        assert!(wait(&b, &|e| matches!(e, Ev::Fetched(3, true))), "download survives lost pieces");
        LOSS.store(0, Ordering::Relaxed);
        let same = |n: &str| std::fs::read(fake.join("got2").join("Popi").join(n)).unwrap() == std::fs::read(fake.join("got3").join("Popi").join(n)).unwrap();
        assert!(same("s0.png") && same("conf/actions.xml"));
        drop(a);
        drop(b);
        std::thread::sleep(Duration::from_millis(100));
        let _ = std::fs::remove_dir_all(&fake);
    }
}
