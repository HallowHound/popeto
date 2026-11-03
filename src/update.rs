//! Updates from GitHub releases: check the latest release, download its zip and swap Popeto.exe.
//! A running exe can't be overwritten but can be renamed, so the old one becomes Popeto.old.exe,
//! the new one starts with `--updated`, and the old file is deleted on that start.
//! Only Popeto.exe (plus README/LICENSE) is replaced; pets, pranks and settings stay as they are.

use crate::i18n::{t, tf};
use crate::App;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

const API: &str = "https://api.github.com/repos/HallowHound/popeto/releases/latest";
const MAX_ZIP: usize = 80 << 20;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
pub struct Release { pub tag: String, url: String }

enum Msg {
    Checked(Result<Option<Release>, String>, bool),
    Installed(Result<PathBuf, String>),
}

pub struct Updater {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    pub available: Option<Release>,
    pub busy: bool,
    next_check: u64,
    /// The last tray balloon was about the update.
    pub balloon: bool,
}

impl Default for Updater {
    fn default() -> Self {
        let (tx, rx) = channel();
        // first automatic check 20 s after start
        Updater { tx, rx, available: None, busy: false, next_check: 40 * 20, balloon: false }
    }
}

fn version(s: &str) -> Vec<u64> {
    s.trim().trim_start_matches(['v', 'V']).split('.').map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0)).collect()
}

fn newer(tag: &str, than: &str) -> bool { version(tag) > version(than) }

/// Latest release, if it is newer than this build.
fn check() -> Result<Option<Release>, String> {
    // debug builds can be pointed at a local test server
    let api = if cfg!(debug_assertions) { std::env::var("POPETO_UPDATE_API").unwrap_or_else(|_| API.into()) } else { API.into() };
    let body = crate::online::http_get(&api, 2 << 20).map_err(|e| if e.ends_with("404") { t("no releases found").to_string() } else { e })?;
    let text = String::from_utf8_lossy(&body);
    let tag = crate::online::json_str(&text, "tag_name").ok_or("no tag_name")?;
    if !newer(&tag, VERSION) { return Ok(None); }
    let mut from = 0;
    while let Some(i) = text[from..].find("\"browser_download_url\"") {
        let at = from + i;
        from = at + 1;
        let Some(url) = crate::online::json_str(&text[at..], "browser_download_url") else { continue };
        let file = url.rsplit('/').next().unwrap_or("");
        let trusted = url.starts_with("https://github.com/") || (cfg!(debug_assertions) && api != API);
        if trusted && file.starts_with("Popeto") && file.ends_with(".zip") {
            return Ok(Some(Release { tag, url }));
        }
    }
    Err(format!("release {tag} has no Popeto zip"))
}

/// Download the release and put its Popeto.exe in place of ours. Returns the exe to start.
fn install(rel: &Release) -> Result<PathBuf, String> {
    let data = crate::online::http_get(&rel.url, MAX_ZIP)?;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| e.to_string())?;
    let mut read = |suffix: &str| -> Option<Vec<u8>> {
        let name = zip.file_names().find(|n| n.replace('\\', "/").rsplit('/').next() == Some(suffix))?.to_string();
        let mut f = zip.by_name(&name).ok()?;
        let mut v = Vec::new();
        f.read_to_end(&mut v).ok()?;
        Some(v)
    };
    let exe_bytes = read("Popeto.exe").ok_or("no Popeto.exe in the zip")?;
    if exe_bytes.len() < 100_000 || !exe_bytes.starts_with(b"MZ") { return Err("Popeto.exe in the zip looks broken".into()); }
    let docs: Vec<(&str, Option<Vec<u8>>)> = ["README.txt", "LICENSE.txt"].iter().map(|n| (*n, read(n))).collect();

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("no exe folder")?.to_path_buf();
    let (new, old) = (dir.join("Popeto.new.exe"), dir.join("Popeto.old.exe"));
    std::fs::write(&new, &exe_bytes).map_err(|e| format!("{}: {e}", new.display()))?;
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| { let _ = std::fs::remove_file(&new); e.to_string() })?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(e.to_string());
    }
    for (n, d) in docs { if let Some(d) = d { let _ = std::fs::write(dir.join(n), d); } }
    Ok(exe)
}

/// On `--updated` start: wait for the old Popeto to exit, then delete its exe.
pub fn after_update(mutex_name: &[u16]) {
    use windows_sys::Win32::System::Threading::OpenMutexW;
    for _ in 0..50 {
        let h = unsafe { OpenMutexW(0x0010_0000 /* SYNCHRONIZE */, 0, mutex_name.as_ptr()) };
        if h.is_null() { break; }
        unsafe { windows_sys::Win32::Foundation::CloseHandle(h); }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
        let _ = std::fs::remove_file(dir.join("Popeto.old.exe"));
    }
}

impl App {
    /// A pet says it, or a tray balloon when no pet is on screen.
    fn tell(&mut self, text: &str) {
        match (0..self.pets.len()).find(|&i| self.pets[i].w.shown) {
            Some(i) => { let id = self.pets[i].m.id; self.say_text(id, text, 260); }
            None => crate::win::tray_balloon(self.tray, "Popeto", text),
        }
    }

    /// `manual`: from the tray menu, so also say when there's nothing new.
    pub fn check_updates(&mut self, manual: bool) {
        if self.update.busy { return; }
        self.update.busy = true;
        let tx = self.update.tx.clone();
        std::thread::spawn(move || { let _ = tx.send(Msg::Checked(check(), manual)); });
    }

    pub fn install_update(&mut self) {
        let Some(rel) = self.update.available.clone() else { return };
        if self.update.busy { return; }
        self.update.busy = true;
        self.tell(t("Downloading the update..."));
        let tx = self.update.tx.clone();
        std::thread::spawn(move || { let _ = tx.send(Msg::Installed(install(&rel))); });
    }

    pub fn update_tick(&mut self) {
        if self.settings.check_updates && self.tick_n >= self.update.next_check {
            self.update.next_check = self.tick_n + 40 * 60 * 60 * 12;
            if self.update.available.is_none() { self.check_updates(false); }
        }
        while let Ok(m) = self.update.rx.try_recv() {
            self.update.busy = false;
            match m {
                Msg::Checked(Ok(Some(rel)), _) => {
                    let known = self.update.available.as_ref().map(|r| r.tag == rel.tag).unwrap_or(false);
                    self.update.available = Some(rel.clone());
                    if !known {
                        crate::win::tray_balloon(self.tray, "Popeto", &tf("Popeto {} is available. Click here to update.", &[&rel.tag]));
                        self.update.balloon = true;
                    }
                }
                Msg::Checked(Ok(None), true) => self.tell(&tf("You have the latest version ({})", &[&VERSION])),
                Msg::Checked(Err(e), true) => self.tell(&tf("Couldn't check for updates: {}", &[&e])),
                Msg::Checked(r, false) => if let Err(e) = r { if self.settings.debug { self.log.push(format!("[update] {e}")); } },
                Msg::Installed(Ok(exe)) => {
                    match std::process::Command::new(&exe).arg("--updated").spawn() {
                        Ok(_) => self.quit = true,
                        Err(e) => self.tell(&tf("Update failed: {}", &[&e.to_string()])),
                    }
                }
                Msg::Installed(Err(e)) => self.tell(&tf("Update failed: {}", &[&e])),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn versions_compare() {
        assert!(super::newer("v0.1.10", "0.1.9"));
        assert!(super::newer("v1.0.0", "0.9.9"));
        assert!(!super::newer("v0.1.1", "0.1.1"));
        assert!(!super::newer("v0.1.0", "0.1.1"));
    }
}
