//! Fresh prank pictures from the internet. A background thread keeps a few random pictures ready in
//! memory (never written to disk), using Windows' own HTTPS stack (WinHTTP), so no extra libraries.
//! Sources come from `online.txt` files in the prank folders.

use std::ffi::c_void;
use std::ptr::null;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows_sys::Win32::Networking::WinHttp::*;

pub struct Pic {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug)]
pub enum Source {
    /// URL that returns a picture.
    Image(String),
    /// URL that returns JSON with a picture link in `field`.
    Json(String, String),
    /// A subreddit through meme-api.com: one of its better posts.
    Reddit(String),
}

/// `reddit <sub>`, `json <url> <field>`, `image <url>`; `#` starts a comment.
pub fn parse_sources(text: &str) -> Vec<Source> {
    text.lines()
        .map(|l| l.trim().trim_start_matches('\u{feff}'))
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            match (it.next()?.to_ascii_lowercase().as_str(), it.next(), it.next()) {
                ("reddit", Some(sub), _) => Some(Source::Reddit(sub.trim_start_matches("r/").into())),
                ("json", Some(url), Some(field)) => Some(Source::Json(url.into(), field.into())),
                ("image", Some(url), _) => Some(Source::Image(url.into())),
                _ => None,
            }
        })
        .collect()
}

#[derive(Default)]
struct Shared {
    queue: Vec<Pic>,
    sources: Vec<Source>,
    enabled: bool,
    last_error: Option<String>,
}

pub struct Online {
    shared: Arc<Mutex<Shared>>,
}

const READY: usize = 3;
const MAX_SIDE: u32 = 700;

impl Online {
    pub fn start() -> Online {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let s = shared.clone();
        std::thread::spawn(move || {
          // recently shown picture links, so the same meme doesn't come twice
          let mut seen: Vec<String> = Vec::new();
          loop {
            let src = {
                let g = s.lock().unwrap();
                if !g.enabled || g.sources.is_empty() || g.queue.len() >= READY { None } else {
                    Some(g.sources[(crate::expr::random() * g.sources.len() as f64) as usize % g.sources.len()].clone())
                }
            };
            let Some(src) = src else { std::thread::sleep(Duration::from_secs(2)); continue };
            match fetch(&src, &mut seen) {
                Ok(p) => {
                    let mut g = s.lock().unwrap();
                    if g.enabled { g.queue.push(p); }
                    drop(g);
                    std::thread::sleep(Duration::from_secs(2));
                }
                Err(e) => {
                    s.lock().unwrap().last_error = Some(e);
                    std::thread::sleep(Duration::from_secs(10));
                }
            }
          }
        });
        Online { shared }
    }

    pub fn configure(&self, enabled: bool, sources: Vec<Source>) {
        let mut g = self.shared.lock().unwrap();
        g.enabled = enabled && !sources.is_empty();
        g.sources = sources;
        if !g.enabled { g.queue.clear(); }
    }

    pub fn take(&self) -> Option<Pic> { self.shared.lock().unwrap().queue.pop() }
    pub fn ready(&self) -> bool { !self.shared.lock().unwrap().queue.is_empty() }
    pub fn take_error(&self) -> Option<String> { self.shared.lock().unwrap().last_error.take() }
}

/// Posts in a meme-api.com answer: (picture link, upvotes), without adult/spoiler posts and animations.
fn reddit_posts(text: &str) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let starts: Vec<usize> = text.match_indices("\"postLink\"").map(|(i, _)| i).collect();
    for (k, &a) in starts.iter().enumerate() {
        let item = &text[a..starts.get(k + 1).copied().unwrap_or(text.len())];
        let compact: String = item.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.contains("\"nsfw\":true") || compact.contains("\"spoiler\":true") { continue; }
        let Some(url) = json_str(item, "url") else { continue };
        let low = url.to_ascii_lowercase();
        if !(low.ends_with(".jpg") || low.ends_with(".jpeg") || low.ends_with(".png")) { continue; }
        let ups = compact.split("\"ups\":").nth(1).map(|r| r.chars().take_while(|c| c.is_ascii_digit()).collect::<String>()).and_then(|n| n.parse().ok()).unwrap_or(0);
        out.push((url, ups));
    }
    out
}

/// One of the better posts: the most upvoted few that weren't shown lately.
fn pick_post(mut posts: Vec<(String, u64)>, seen: &[String]) -> Option<String> {
    posts.retain(|(u, _)| !seen.contains(u));
    posts.sort_by(|a, b| b.1.cmp(&a.1));
    posts.truncate(5);
    if posts.is_empty() { return None; }
    Some(posts.swap_remove((crate::expr::random() * posts.len() as f64) as usize % posts.len()).0)
}

fn fetch(src: &Source, seen: &mut Vec<String>) -> Result<Pic, String> {
    let link = match src {
        Source::Image(u) => u.clone(),
        Source::Reddit(sub) => {
            let u = format!("https://meme-api.com/gimme/{sub}/30");
            let body = http_get(&u, 2 << 20)?;
            let link = pick_post(reddit_posts(&String::from_utf8_lossy(&body)), seen).ok_or_else(|| format!("no new pictures in r/{sub}"))?;
            seen.push(link.clone());
            if seen.len() > 200 { seen.remove(0); }
            link
        }
        Source::Json(u, field) => {
            let body = http_get(u, 1 << 20)?;
            let text = String::from_utf8_lossy(&body);
            let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
            if compact.contains("\"nsfw\":true") || compact.contains("\"spoiler\":true") { return Err("skipped adult/spoiler post".into()); }
            json_str(&text, field).ok_or_else(|| format!("no '{field}' in answer from {u}"))?
        }
    };
    let bytes = http_get(&link, 12 << 20)?;
    let (w, h, rgba) = crate::win::decode_rgba(&bytes).ok_or_else(|| format!("not a PNG/JPEG picture: {link}"))?;
    if w < 80 || h < 80 { return Err("picture too small".into()); }
    let k = (MAX_SIDE as f64 / w.max(h) as f64).min(1.0);
    if k < 1.0 {
        let (nw, nh) = (((w as f64 * k) as u32).max(1), ((h as f64 * k) as u32).max(1));
        return Ok(Pic { w: nw, h: nh, rgba: crate::win::resize(&rgba, w, h, nw, nh, false) });
    }
    Ok(Pic { w, h, rgba })
}

/// First string value of `"field": "..."` anywhere in the JSON text, unescaped.
pub(crate) fn json_str(text: &str, field: &str) -> Option<String> {
    let key = format!("\"{field}\"");
    let mut from = 0;
    while let Some(i) = text[from..].find(&key) {
        let rest = text[from + i + key.len()..].trim_start();
        from += i + key.len();
        let Some(rest) = rest.strip_prefix(':') else { continue };
        let Some(rest) = rest.trim_start().strip_prefix('"') else { continue };
        let mut out = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => return Some(out),
                '\\' => match chars.next()? {
                    'u' => {
                        let hex: String = chars.by_ref().take(4).collect();
                        out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?).unwrap_or('?'));
                    }
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    o => out.push(o),
                },
                c => out.push(c),
            }
        }
        return None;
    }
    None
}

fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }

struct H(*mut c_void);
impl Drop for H { fn drop(&mut self) { if !self.0.is_null() { unsafe { WinHttpCloseHandle(self.0); } } } }

/// HTTPS/HTTP GET through WinHTTP (system proxy and certificates, redirects followed).
pub fn http_get(url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let (secure, rest) = if let Some(r) = url.strip_prefix("https://") { (true, r) } else if let Some(r) = url.strip_prefix("http://") { (false, r) } else { return Err(format!("bad url {url}")) };
    let (hostport, path) = match rest.find('/') { Some(i) => (&rest[..i], &rest[i..]), None => (rest, "/") };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => (h, p.parse::<u16>().unwrap_or(443)),
        _ => (hostport, if secure { 443 } else { 80 }),
    };
    unsafe {
        let agent = wide("Popeto/1.0 (desktop pets)");
        // 4 = WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY (system proxy settings)
        let session = H(WinHttpOpen(agent.as_ptr(), 4 as _, null(), null(), 0));
        if session.0.is_null() { return Err("WinHttpOpen failed".into()); }
        WinHttpSetTimeouts(session.0, 10_000, 10_000, 15_000, 20_000);
        let h = wide(host);
        let conn = H(WinHttpConnect(session.0, h.as_ptr(), port, 0));
        if conn.0.is_null() { return Err(format!("cannot connect to {host}")); }
        let (verb, obj) = (wide("GET"), wide(path));
        let flags = if secure { 0x0080_0000 } else { 0 }; // WINHTTP_FLAG_SECURE
        let req = H(WinHttpOpenRequest(conn.0, verb.as_ptr(), obj.as_ptr(), null(), null(), null(), flags as _));
        if req.0.is_null() { return Err("WinHttpOpenRequest failed".into()); }
        if WinHttpSendRequest(req.0, null(), 0, null(), 0, 0, 0) == 0 || WinHttpReceiveResponse(req.0, std::ptr::null_mut()) == 0 {
            return Err(format!("no answer from {host}"));
        }
        let mut code: u32 = 0;
        let mut len = 4u32;
        // 19 = WINHTTP_QUERY_STATUS_CODE, 0x20000000 = WINHTTP_QUERY_FLAG_NUMBER
        WinHttpQueryHeaders(req.0, 19 | 0x2000_0000, null(), &mut code as *mut _ as *mut c_void, &mut len, std::ptr::null_mut());
        if code != 200 { return Err(format!("{host} answered {code}")); }
        let mut out = Vec::new();
        loop {
            let mut avail = 0u32;
            if WinHttpQueryDataAvailable(req.0, &mut avail) == 0 || avail == 0 { break; }
            let start = out.len();
            out.resize(start + avail as usize, 0);
            let mut read = 0u32;
            if WinHttpReadData(req.0, out[start..].as_mut_ptr() as *mut c_void, avail, &mut read) == 0 { return Err("download interrupted".into()); }
            out.truncate(start + read as usize);
            if out.len() > limit { return Err("file too big".into()); }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn json_and_sources() {
        let t = r#"{"postLink":"x","url":"https:\/\/i.redd.it\/a.png","nsfw":false}"#;
        assert_eq!(super::json_str(t, "url").unwrap(), "https://i.redd.it/a.png");
        let s = super::parse_sources("# c\nreddit Pikabu\njson https://a.b/c message\nimage https://x.y/z\n");
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn picks_popular_pictures() {
        let t = r#"{"count":4,"memes":[
            {"postLink":"a","url":"https://i.redd.it/low.jpg","nsfw":false,"spoiler":false,"ups":3},
            {"postLink":"b","url":"https://i.redd.it/top.png","nsfw":false,"spoiler":false,"ups":9000},
            {"postLink":"c","url":"https://i.redd.it/adult.jpg","nsfw":true,"spoiler":false,"ups":99999},
            {"postLink":"d","url":"https://i.redd.it/anim.gif","nsfw":false,"spoiler":false,"ups":50000}]}"#;
        let posts = super::reddit_posts(t);
        assert_eq!(posts, vec![("https://i.redd.it/low.jpg".to_string(), 3), ("https://i.redd.it/top.png".to_string(), 9000)]);
        // the best one wins among two; once shown, the other comes next
        let seen = vec!["https://i.redd.it/top.png".to_string()];
        assert_eq!(super::pick_post(posts.clone(), &seen).unwrap(), "https://i.redd.it/low.jpg");
        assert_eq!(super::pick_post(posts, &[]).map(|u| u.contains("redd.it")), Some(true));
    }
    /// POPETO_NET_TEST=1 cargo test fetch_live -- --nocapture
    #[test]
    fn fetch_live() {
        if std::env::var("POPETO_NET_TEST").is_err() { return; }
        let mut seen = Vec::new();
        for s in super::parse_sources(include_str!("../assets/pranks/universal/online.txt")).into_iter()
            .chain(super::parse_sources(include_str!("../assets/pranks/en/online.txt")))
            .chain(super::parse_sources(include_str!("../assets/pranks/ru/online.txt"))) {
            match super::fetch(&s, &mut seen) {
                Ok(p) => println!("{s:?}: {}x{}", p.w, p.h),
                Err(e) => println!("{s:?}: ERR {e}"),
            }
        }
    }
}
