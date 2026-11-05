//! Start with Windows: a `Popeto` value under HKCU\...\CurrentVersion\Run (this user only, no admin).
//! The registry is the only place this is stored, so Windows' own Startup apps page stays in sync.

use std::ptr::null_mut;
use windows_sys::Win32::System::Registry::*;

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Windows' Startup apps page switches entries off here (first byte 3 = off).
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const NAME: &str = "Popeto";

fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }

fn command() -> Option<String> {
    std::env::current_exe().ok().map(|p| format!("\"{}\"", p.display()))
}

fn read_run() -> Option<String> {
    let (key, name) = (wide(RUN), wide(NAME));
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, null_mut(), buf.as_mut_ptr() as _, &mut size) };
    if r != 0 { return None; }
    let n = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..n.min(buf.len())]))
}

fn switched_off_in_windows() -> bool {
    let (key, name) = (wide(APPROVED), wide(NAME));
    let mut buf = [0u8; 16];
    let mut size = buf.len() as u32;
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_BINARY, null_mut(), buf.as_mut_ptr() as _, &mut size) };
    r == 0 && size > 0 && buf[0] & 1 == 1
}

pub fn enabled() -> bool { read_run().is_some() && !switched_off_in_windows() }

pub fn set(on: bool) {
    let (key, name) = (wide(RUN), wide(NAME));
    unsafe {
        if on {
            let Some(cmd) = command() else { return };
            let v = wide(&cmd);
            RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), REG_SZ, v.as_ptr() as _, (v.len() * 2) as u32);
            // ticking it here also undoes "off" from the Startup apps page
            let ak = wide(APPROVED);
            RegDeleteKeyValueW(HKEY_CURRENT_USER, ak.as_ptr(), name.as_ptr());
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr());
        }
    }
}

/// Portable folder moved (the registered exe is gone): point the entry at this exe.
/// A second copy running elsewhere leaves a working entry alone.
pub fn refresh() {
    if let (Some(cur), Some(cmd)) = (read_run(), command()) {
        if !std::path::Path::new(cur.trim_matches('"')).exists() {
            let (key, name, v) = (wide(RUN), wide(NAME), wide(&cmd));
            unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), REG_SZ, v.as_ptr() as _, (v.len() * 2) as u32); }
        }
    }
}

#[cfg(test)]
mod tests {
    /// Touches the real Run key, so only on request: cargo test -- --ignored autostart
    #[test]
    #[ignore]
    fn autostart_round_trip() {
        let was = super::enabled();
        super::set(true);
        assert!(super::enabled());
        assert_eq!(super::read_run(), super::command());
        super::set(false);
        assert!(!super::enabled());
        if was { super::set(true); }
    }
}
