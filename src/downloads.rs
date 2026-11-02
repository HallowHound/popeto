//! Watches the Downloads folder: when a new Shimeji zip finishes downloading, the app offers to add it.
//! Only looks at file names, sizes and the zip's file list; nothing is extracted until you accept.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

pub struct Watcher {
    dir: Option<PathBuf>,
    /// zip -> (size, modified) already handled
    seen: HashMap<PathBuf, (u64, SystemTime)>,
    /// zip -> size at the previous poll (still downloading if it changes)
    growing: HashMap<PathBuf, u64>,
}

/// The real Downloads folder (follows a moved/redirected Downloads), or %USERPROFILE%\Downloads.
fn downloads_dir() -> Option<PathBuf> {
    use windows_sys::Win32::UI::Shell::{FOLDERID_Downloads, SHGetKnownFolderPath};
    unsafe {
        let mut p: windows_sys::core::PWSTR = std::ptr::null_mut();
        if SHGetKnownFolderPath(&FOLDERID_Downloads, 0, std::ptr::null_mut(), &mut p) == 0 && !p.is_null() {
            let n = (0..).take_while(|&i| *p.add(i) != 0).count();
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
            windows_sys::Win32::System::Com::CoTaskMemFree(p as *const _);
            return Some(PathBuf::from(s));
        }
    }
    std::env::var_os("USERPROFILE").map(|u| PathBuf::from(u).join("Downloads"))
}

fn zips(dir: &PathBuf) -> Vec<(PathBuf, u64, SystemTime)> {
    std::fs::read_dir(dir).into_iter().flatten().flatten()
        .filter(|e| e.path().extension().map(|x| x.eq_ignore_ascii_case("zip")).unwrap_or(false))
        .filter_map(|e| { let m = e.metadata().ok()?; Some((e.path(), m.len(), m.modified().ok()?)) })
        .collect()
}

impl Watcher {
    /// Zips already in Downloads when Popeto starts are ignored.
    pub fn new() -> Watcher {
        let dir = downloads_dir().filter(|d| d.is_dir());
        let seen = dir.as_ref().map(|d| zips(d).into_iter().map(|(p, l, t)| (p, (l, t))).collect()).unwrap_or_default();
        Watcher { dir, seen, growing: HashMap::new() }
    }

    /// New, finished Shimeji zips since the last poll, with the pet names inside.
    pub fn poll(&mut self) -> Vec<(PathBuf, Vec<String>)> {
        let Some(dir) = &self.dir else { return vec![] };
        let mut found = Vec::new();
        for (p, len, t) in zips(dir) {
            if self.seen.get(&p) == Some(&(len, t)) { continue; }
            // wait until the size stops changing between two polls
            if self.growing.get(&p) != Some(&len) { self.growing.insert(p, len); continue; }
            self.growing.remove(&p);
            self.seen.insert(p.clone(), (len, t));
            if let Some(names) = crate::importer::peek_zip(&p) { found.push((p, names)); }
        }
        found
    }
}

impl Watcher {
    #[cfg(test)]
    fn with_dir(dir: PathBuf) -> Watcher { Watcher { dir: Some(dir), seen: HashMap::new(), growing: HashMap::new() } }
}

#[cfg(test)]
mod tests {
    /// POPETO_IMPORT_ZIP=<zip> cargo test watcher -- --nocapture
    #[test]
    fn watcher_finds_new_zip() {
        let Ok(zip) = std::env::var("POPETO_IMPORT_ZIP") else { return };
        let dir = std::env::temp_dir().join(format!("popeto-watch-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut w = super::Watcher::with_dir(dir.clone());
        assert!(w.poll().is_empty());
        std::fs::copy(&zip, dir.join("Kopa Cub Shimeji.zip")).unwrap();
        assert!(w.poll().is_empty(), "first sight waits for the download to finish");
        let found = w.poll();
        println!("found {:?}", found.iter().map(|f| &f.1).collect::<Vec<_>>());
        assert_eq!(found.len(), 1);
        assert!(w.poll().is_empty(), "reported once");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
