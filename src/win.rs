//! Win32 plumbing: window discovery, monitors, layered pet windows, PNG -> bitmap, tray, menus.

use crate::env::{Monitor, Rect, Win, World};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }

pub const PET_CLASS: &str = "PopetoPet";
pub const TRAY_CLASS: &str = "PopetoTray";
pub const NOTE_CLASS: &str = "PopetoNote";
pub const WM_TRAY: u32 = WM_APP + 1;

// ---------------------------------------------------------------- events from wndprocs

pub enum Ev {
    Press(isize, i32, i32),
    Release(isize),
    RClick(isize),
    Tray(u32),
    TaskbarCreated,
    Display,
    CloseNote(isize),
    Panic,
    /// Settings window OK (true = close afterwards) or Apply.
    SettingsApply(bool),
    /// Import pets from this zip/folder (file dialog, drag onto exe, or a second Popeto.exe).
    Import(String),
    /// A tray command triggered from elsewhere (settings window buttons).
    Command(usize),
}

thread_local! {
    pub static EVENTS: RefCell<Vec<Ev>> = const { RefCell::new(Vec::new()) };
    static TASKBAR_MSG: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

fn push(e: Ev) { EVENTS.with(|q| q.borrow_mut().push(e)); }

unsafe extern "system" fn pet_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let x = (lp & 0xFFFF) as i16 as i32;
    let y = ((lp >> 16) & 0xFFFF) as i16 as i32;
    match msg {
        WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,
        WM_LBUTTONDOWN => push(Ev::Press(h as isize, x, y)),
        WM_LBUTTONUP => push(Ev::Release(h as isize)),
        WM_CAPTURECHANGED => push(Ev::Release(h as isize)),
        WM_RBUTTONUP => push(Ev::RClick(h as isize)),
        WM_DISPLAYCHANGE => push(Ev::Display),
        _ => {}
    }
    DefWindowProcW(h, msg, wp, lp)
}

unsafe extern "system" fn tray_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_TRAY {
        push(Ev::Tray(lp as u32 & 0xFFFF));
        return 0;
    }
    if msg == WM_DISPLAYCHANGE { push(Ev::Display); }
    if msg == WM_HOTKEY { push(Ev::Panic); return 0; }
    if msg == WM_COPYDATA {
        let cd = &*(lp as *const windows_sys::Win32::System::DataExchange::COPYDATASTRUCT);
        if cd.dwData == 0x504F5045 && !cd.lpData.is_null() {
            let s = std::slice::from_raw_parts(cd.lpData as *const u16, cd.cbData as usize / 2);
            push(Ev::Import(String::from_utf16_lossy(s)));
        }
        return 1;
    }
    let tb = TASKBAR_MSG.with(|t| t.get());
    if tb != 0 && msg == tb { push(Ev::TaskbarCreated); }
    DefWindowProcW(h, msg, wp, lp)
}

/// Notes and pictures pets bring in: drag anywhere to move, click the corner x (or right/middle click) to close.
unsafe extern "system" fn note_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => {
            let (x, y) = ((lp & 0xFFFF) as i16 as i32, ((lp >> 16) & 0xFFFF) as i16 as i32);
            let mut r: RECT = std::mem::zeroed();
            GetWindowRect(h, &mut r);
            let close = GetWindowLongPtrW(h, GWLP_USERDATA) as i32;
            return if x >= r.right - close && y < r.top + close { HTCLIENT as LRESULT } else { HTCAPTION as LRESULT };
        }
        WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,
        WM_LBUTTONUP | WM_NCRBUTTONUP | WM_RBUTTONUP | WM_NCMBUTTONUP | WM_MBUTTONUP => { push(Ev::CloseNote(h as isize)); return 0; }
        _ => {}
    }
    DefWindowProcW(h, msg, wp, lp)
}

pub fn register_classes() {
    unsafe {
        let inst = GetModuleHandleW(null());
        for (name, proc_) in [(PET_CLASS, pet_proc as unsafe extern "system" fn(_, _, _, _) -> _), (TRAY_CLASS, tray_proc), (NOTE_CLASS, note_proc)] {
            let cls = wide(name);
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: Some(proc_),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: inst,
                hIcon: null_mut(),
                hCursor: LoadCursorW(null_mut(), IDC_HAND),
                hbrBackground: null_mut(),
                lpszMenuName: null(),
                lpszClassName: cls.as_ptr(),
                hIconSm: null_mut(),
            };
            RegisterClassExW(&wc);
        }
        let tb = wide("TaskbarCreated");
        TASKBAR_MSG.with(|t| t.set(RegisterWindowMessageW(tb.as_ptr())));
    }
}

pub fn create_pet_window() -> HWND {
    unsafe {
        let cls = wide(PET_CLASS);
        let title = wide("Popeto pet");
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            cls.as_ptr(), title.as_ptr(), WS_POPUP,
            -10000, -10000, 1, 1, null_mut(), null_mut(), GetModuleHandleW(null()), null(),
        )
    }
}

/// Layered helper window: click-through (bubbles, footprints), a pet-like grabbable toy, or a note.
pub fn create_overlay(click_through: bool, note_close_px: i32) -> HWND {
    unsafe {
        let class = wide(if note_close_px > 0 { NOTE_CLASS } else { PET_CLASS });
        let title = wide("Popeto");
        let ex = WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | if click_through { WS_EX_TRANSPARENT } else { 0 };
        let h = CreateWindowExW(ex, class.as_ptr(), title.as_ptr(), WS_POPUP, -10000, -10000, 1, 1, null_mut(), null_mut(), GetModuleHandleW(null()), null());
        if note_close_px > 0 { SetWindowLongPtrW(h, GWLP_USERDATA, note_close_px as isize); }
        h
    }
}

pub fn window_pos(h: HWND) -> (i32, i32) {
    let mut r: RECT = unsafe { std::mem::zeroed() };
    unsafe { GetWindowRect(h, &mut r); }
    (r.left, r.top)
}

pub fn set_cursor(x: i32, y: i32) { unsafe { SetCursorPos(x, y); } }

pub fn minimize_app_window(h: isize) { unsafe { ShowWindowAsync(h as HWND, SW_MINIMIZE); } }

/// Parse "Ctrl+Shift+F12" style text and register it as a global hotkey on `owner`.
pub fn register_hotkey(owner: HWND, spec: &str) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    let mut mods = MOD_NOREPEAT;
    let mut vk = 0u32;
    for part in spec.split('+').map(|p| p.trim().to_ascii_uppercase()) {
        match part.as_str() {
            "CTRL" | "CONTROL" => mods |= MOD_CONTROL,
            "SHIFT" => mods |= MOD_SHIFT,
            "ALT" => mods |= MOD_ALT,
            "WIN" => mods |= MOD_WIN,
            "ESC" | "ESCAPE" => vk = VK_ESCAPE as u32,
            "PAUSE" => vk = VK_PAUSE as u32,
            "SPACE" => vk = VK_SPACE as u32,
            p if p.len() == 1 && p.chars().all(|c| c.is_ascii_alphanumeric()) => vk = p.as_bytes()[0] as u32,
            p if p.starts_with('F') => if let Ok(n) = p[1..].parse::<u32>() { if (1..=24).contains(&n) { vk = VK_F1 as u32 + n - 1; } },
            _ => {}
        }
    }
    unsafe {
        UnregisterHotKey(owner, 1);
        vk != 0 && RegisterHotKey(owner, 1, mods, vk) != 0
    }
}

/// Hand a path to the Popeto that is already running (it imports it). Returns false if none found.
pub fn send_to_running(path: &str) -> bool {
    use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
    unsafe {
        let cls = wide(TRAY_CLASS);
        let h = FindWindowW(cls.as_ptr(), null());
        if h.is_null() { return false; }
        let data: Vec<u16> = path.encode_utf16().collect();
        let cd = COPYDATASTRUCT { dwData: 0x504F5045, cbData: (data.len() * 2) as u32, lpData: data.as_ptr() as *mut _ };
        SendMessageW(h, WM_COPYDATA, 0, &cd as *const _ as LPARAM);
        true
    }
}

/// Standard Open dialog. `filter` pairs: (label, "*.zip;*.png").
pub fn open_file(owner: HWND, title: &str, filter: &[(&str, &str)]) -> Option<std::path::PathBuf> {
    use windows_sys::Win32::UI::Controls::Dialogs::*;
    let mut f: Vec<u16> = Vec::new();
    for (l, p) in filter { f.extend(l.encode_utf16()); f.push(0); f.extend(p.encode_utf16()); f.push(0); }
    f.push(0);
    let t = wide(title);
    let mut buf = vec![0u16; 1024];
    unsafe {
        let mut ofn: OPENFILENAMEW = std::mem::zeroed();
        ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        ofn.hwndOwner = owner;
        ofn.lpstrFilter = f.as_ptr();
        ofn.lpstrFile = buf.as_mut_ptr();
        ofn.nMaxFile = buf.len() as u32;
        ofn.lpstrTitle = t.as_ptr();
        ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_EXPLORER | OFN_NOCHANGEDIR;
        if GetOpenFileNameW(&mut ofn) == 0 { return None; }
    }
    let n = buf.iter().position(|c| *c == 0).unwrap_or(0);
    Some(std::path::PathBuf::from(String::from_utf16_lossy(&buf[..n])))
}

pub fn create_tray_window() -> HWND {
    unsafe {
        let cls = wide(TRAY_CLASS);
        let title = wide("Popeto");
        CreateWindowExW(WS_EX_TOOLWINDOW, cls.as_ptr(), title.as_ptr(), WS_POPUP, 0, 0, 0, 0, null_mut(), null_mut(), GetModuleHandleW(null()), null())
    }
}

pub fn destroy(h: HWND) { unsafe { DestroyWindow(h); } }

// ---------------------------------------------------------------- images

pub struct Bmp {
    pub w: i32,
    pub h: i32,
    pub normal: HBITMAP,
    pub flipped: HBITMAP,
}

/// Decode PNG into straight RGBA, scaled.
pub fn load_rgba(path: &Path, scale: f64, nearest: bool) -> Option<(u32, u32, Vec<u8>)> {
    let f = std::fs::File::open(path).ok()?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(f));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut r = dec.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width, info.height);
    let px = (w * h) as usize;
    let mut rgba = Vec::with_capacity(px * 4);
    let b = &buf[..info.buffer_size()];
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(b),
        png::ColorType::Rgb => b.chunks(3).for_each(|c| rgba.extend_from_slice(&[c[0], c[1], c[2], 255])),
        png::ColorType::GrayscaleAlpha => b.chunks(2).for_each(|c| rgba.extend_from_slice(&[c[0], c[0], c[0], c[1]])),
        png::ColorType::Grayscale => b.iter().for_each(|&c| rgba.extend_from_slice(&[c, c, c, 255])),
        png::ColorType::Indexed => return None,
    }
    if (scale - 1.0).abs() < 1e-3 { return Some((w, h, rgba)); }
    let nw = ((w as f64 * scale).round() as u32).max(1);
    let nh = ((h as f64 * scale).round() as u32).max(1);
    Some((nw, nh, resize(&rgba, w, h, nw, nh, nearest)))
}

fn resize(src: &[u8], w: u32, h: u32, nw: u32, nh: u32, nearest: bool) -> Vec<u8> {
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    // premultiply for correct filtering
    let pm: Vec<f32> = src.chunks(4).flat_map(|c| { let a = c[3] as f32 / 255.0; [c[0] as f32 * a, c[1] as f32 * a, c[2] as f32 * a, c[3] as f32] }).collect();
    let px = |x: u32, y: u32| ((y.min(h - 1) * w + x.min(w - 1)) * 4) as usize;
    for y in 0..nh {
        for x in 0..nw {
            let fx = (x as f32 + 0.5) * w as f32 / nw as f32 - 0.5;
            let fy = (y as f32 + 0.5) * h as f32 / nh as f32 - 0.5;
            let mut acc = [0f32; 4];
            if nearest {
                let i = px(fx.round().max(0.0) as u32, fy.round().max(0.0) as u32);
                acc.copy_from_slice(&pm[i..i + 4]);
            } else {
                let (x0, y0) = (fx.floor().max(0.0) as u32, fy.floor().max(0.0) as u32);
                let (tx, ty) = ((fx - x0 as f32).clamp(0.0, 1.0), (fy - y0 as f32).clamp(0.0, 1.0));
                for (dx, dy, wt) in [(0, 0, (1.0 - tx) * (1.0 - ty)), (1, 0, tx * (1.0 - ty)), (0, 1, (1.0 - tx) * ty), (1, 1, tx * ty)] {
                    let i = px(x0 + dx, y0 + dy);
                    for k in 0..4 { acc[k] += pm[i + k] * wt; }
                }
            }
            let o = ((y * nw + x) * 4) as usize;
            let a = acc[3];
            let un = if a > 0.0 { 255.0 / a } else { 0.0 };
            out[o] = (acc[0] * un).round().clamp(0.0, 255.0) as u8;
            out[o + 1] = (acc[1] * un).round().clamp(0.0, 255.0) as u8;
            out[o + 2] = (acc[2] * un).round().clamp(0.0, 255.0) as u8;
            out[o + 3] = a.round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// Top-down 32bpp DIB; premultiplied BGRA for layered windows, straight for icons.
fn make_dib(w: u32, h: u32, rgba: &[u8], flip: bool, premul: bool) -> HBITMAP {
    unsafe {
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = w as i32;
        bi.bmiHeader.biHeight = -(h as i32);
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB;
        let mut bits: *mut c_void = null_mut();
        let bmp = CreateDIBSection(null_mut(), &bi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if bmp.is_null() || bits.is_null() { return null_mut(); }
        let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let sx = if flip { w - 1 - x } else { x };
                let s = ((y * w + sx) * 4) as usize;
                let d = ((y * w + x) * 4) as usize;
                let a = rgba[s + 3] as u32;
                let m = |c: u8| if premul { ((c as u32 * a + 127) / 255) as u8 } else { c };
                dst[d] = m(rgba[s + 2]);
                dst[d + 1] = m(rgba[s + 1]);
                dst[d + 2] = m(rgba[s]);
                dst[d + 3] = a as u8;
            }
        }
        bmp
    }
}

pub fn make_bmp(path: &Path, scale: f64, nearest: bool) -> Option<Bmp> {
    let (w, h, rgba) = load_rgba(path, scale, nearest)?;
    let normal = make_dib(w, h, &rgba, false, true);
    let flipped = make_dib(w, h, &rgba, true, true);
    if normal.is_null() || flipped.is_null() { return None; }
    Some(Bmp { w: w as i32, h: h as i32, normal, flipped })
}

pub fn free_bmp(b: &Bmp) { unsafe { DeleteObject(b.normal); DeleteObject(b.flipped); } }

/// Image cropped to its opaque pixels and fitted into a `box_px` square (premultiplied, for AlphaBlend).
pub fn make_preview(path: &Path, box_px: u32) -> Option<(HBITMAP, i32, i32)> {
    let (w, h, rgba) = load_rgba(path, 1.0, false)?;
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h { for x in 0..w { if rgba[((y * w + x) * 4 + 3) as usize] > 16 { x0 = x0.min(x); y0 = y0.min(y); x1 = x1.max(x); y1 = y1.max(y); } } }
    if x1 < x0 { return None; }
    let (cw, ch) = (x1 - x0 + 1, y1 - y0 + 1);
    let mut crop = Vec::with_capacity((cw * ch * 4) as usize);
    for y in y0..=y1 { let s = ((y * w + x0) * 4) as usize; crop.extend_from_slice(&rgba[s..s + (cw * 4) as usize]); }
    let k = (box_px as f64 / cw.max(ch) as f64).min(2.0);
    let (nw, nh) = (((cw as f64 * k).round() as u32).max(1), ((ch as f64 * k).round() as u32).max(1));
    let px = if (nw, nh) == (cw, ch) { crop } else { resize(&crop, cw, ch, nw, nh, false) };
    let bmp = make_dib(nw, nh, &px, false, true);
    if bmp.is_null() { None } else { Some((bmp, nw as i32, nh as i32)) }
}

pub fn make_icon(path: &Path) -> HICON {
    unsafe {
        let Some((w, h, rgba)) = load_rgba(path, 1.0, false) else { return LoadIconW(null_mut(), IDI_APPLICATION) };
        // crop to opaque bounds, fit into a square, scale to 32px
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
        for y in 0..h { for x in 0..w { if rgba[((y * w + x) * 4 + 3) as usize] > 16 { x0 = x0.min(x); y0 = y0.min(y); x1 = x1.max(x); y1 = y1.max(y); } } }
        if x1 < x0 { return LoadIconW(null_mut(), IDI_APPLICATION); }
        let side = (x1 - x0 + 1).max(y1 - y0 + 1);
        let (ox, oy) = (x0 as i32 - ((side - (x1 - x0 + 1)) / 2) as i32, y0 as i32 - ((side - (y1 - y0 + 1)) / 2) as i32);
        let mut sq = vec![0u8; (side * side * 4) as usize];
        for y in 0..side { for x in 0..side {
            let (sx, sy) = (ox + x as i32, oy + y as i32);
            if sx >= 0 && sy >= 0 && (sx as u32) < w && (sy as u32) < h {
                let s = ((sy as u32 * w + sx as u32) * 4) as usize;
                let d = ((y * side + x) * 4) as usize;
                sq[d..d + 4].copy_from_slice(&rgba[s..s + 4]);
            }
        } }
        let n = 32;
        let small = resize(&sq, side, side, n, n, false);
        let color = make_dib(n, n, &small, false, false);
        let mask = CreateBitmap(n as i32, n as i32, 1, 1, null());
        let ii = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&ii);
        DeleteObject(mask);
        DeleteObject(color);
        if icon.is_null() { LoadIconW(null_mut(), IDI_APPLICATION) } else { icon }
    }
}

pub struct Painter { dc: HDC }

impl Painter {
    pub fn new() -> Self { Painter { dc: unsafe { CreateCompatibleDC(null_mut()) } } }

    pub fn update(&self, h: HWND, bmp: HBITMAP, x: i32, y: i32, w: i32, hgt: i32, alpha: u8) {
        unsafe {
            let old = SelectObject(self.dc, bmp);
            let pt = POINT { x, y };
            let sz = SIZE { cx: w, cy: hgt };
            let src = POINT { x: 0, y: 0 };
            let bf = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: alpha, AlphaFormat: AC_SRC_ALPHA as u8 };
            UpdateLayeredWindow(h, null_mut(), &pt, &sz, self.dc, &src, 0, &bf, ULW_ALPHA);
            SelectObject(self.dc, old);
        }
    }
}

pub fn move_window(h: HWND, x: i32, y: i32) {
    unsafe { SetWindowPos(h, null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE); }
}
pub fn show(h: HWND, on: bool) { unsafe { ShowWindow(h, if on { SW_SHOWNOACTIVATE } else { SW_HIDE }); } }
/// Put our windows at the top of the always-on-top band in this exact order (first = topmost),
/// in one atomic DeferWindowPos batch so the screen never shows them reshuffled in between.
pub fn restack(top_to_bottom: &[HWND]) {
    if top_to_bottom.is_empty() { return; }
    unsafe {
        let mut dw = BeginDeferWindowPos(top_to_bottom.len() as i32);
        for (i, h) in top_to_bottom.iter().enumerate() {
            if dw.is_null() { break; }
            let after = if i == 0 { HWND_TOPMOST } else { top_to_bottom[i - 1] };
            dw = DeferWindowPos(dw, *h, after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
        }
        if !dw.is_null() { EndDeferWindowPos(dw); }
    }
}
pub fn capture(h: HWND) { unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::SetCapture(h); } }
pub fn release_capture() { unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture(); } }

pub fn cursor() -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut p); }
    (p.x, p.y)
}

// ---------------------------------------------------------------- desktop scanning

unsafe extern "system" fn mon_cb(hm: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> BOOL {
    let v = &mut *(lp as *mut Vec<(Rect, Rect)>);
    let mut mi: MONITORINFO = std::mem::zeroed();
    mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if GetMonitorInfoW(hm, &mut mi) != 0 {
        let c = |r: RECT| Rect { l: r.left, t: r.top, r: r.right, b: r.bottom };
        let item = (c(mi.rcMonitor), c(mi.rcWork));
        if mi.dwFlags & MONITORINFOF_PRIMARY != 0 { v.insert(0, item) } else { v.push(item) }
    }
    1
}

pub fn monitors() -> Vec<Monitor> {
    let mut v: Vec<(Rect, Rect)> = Vec::new();
    unsafe { EnumDisplayMonitors(null_mut(), null(), Some(mon_cb), &mut v as *mut _ as LPARAM); }
    let all = v.clone();
    v.iter()
        .map(|(s, w)| {
            let touch = |f: &dyn Fn(&Rect) -> bool| all.iter().any(|(o, _)| o != s && f(o));
            let vert = |o: &Rect| o.t < s.b && s.t < o.b;
            let hor = |o: &Rect| o.l < s.r && s.l < o.r;
            Monitor {
                screen: *s,
                work: *w,
                open: [
                    touch(&|o| o.r == s.l && vert(o)) && w.l == s.l,
                    touch(&|o| o.b == s.t && hor(o)) && w.t == s.t,
                    touch(&|o| o.l == s.r && vert(o)) && w.r == s.r,
                    touch(&|o| o.t == s.b && hor(o)) && w.b == s.b,
                ],
            }
        })
        .collect()
}

unsafe extern "system" fn enum_cb(h: HWND, lp: LPARAM) -> BOOL {
    (*(lp as *mut Vec<HWND>)).push(h);
    1
}

pub fn frame_rect(h: HWND) -> Option<Rect> {
    unsafe {
        let mut r: RECT = std::mem::zeroed();
        if DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS as u32, &mut r as *mut _ as *mut c_void, std::mem::size_of::<RECT>() as u32) != 0 {
            if GetWindowRect(h, &mut r) == 0 { return None; }
        }
        Some(Rect { l: r.left, t: r.top, r: r.right, b: r.bottom })
    }
}

fn text(h: HWND, class: bool) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { if class { GetClassNameW(h, buf.as_mut_ptr(), 256) } else { InternalGetWindowText(h, buf.as_mut_ptr(), 256) } };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn exe_name(pid: u32) -> String {
    unsafe {
        let hp = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if hp.is_null() { return String::new(); }
        let mut buf = [0u16; 512];
        let mut n = 512u32;
        let ok = QueryFullProcessImageNameW(hp, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut n);
        CloseHandle(hp);
        if ok == 0 { return String::new(); }
        let full = String::from_utf16_lossy(&buf[..n as usize]);
        Path::new(&full).file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default()
    }
}

const SKIP_CLASSES: [&str; 12] = [
    "Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd", PET_CLASS, TRAY_CLASS,
    "Windows.UI.Core.CoreWindow", "XamlExplorerHostIslandWindow", "TaskListThumbnailWnd", "NotifyIconOverflowWindow",
    "#32768", "ForegroundStaging",
];

pub struct Scanner {
    exe: HashMap<u32, String>,
    own_pid: u32,
    pub blacklist: Vec<String>,
}

impl Scanner {
    pub fn new(blacklist: Vec<String>) -> Self {
        Scanner { exe: HashMap::new(), own_pid: unsafe { GetCurrentProcessId() }, blacklist }
    }

    fn qualifies(&mut self, h: HWND) -> Option<Rect> {
        unsafe {
            if IsWindowVisible(h) == 0 || IsIconic(h) != 0 || IsZoomed(h) != 0 { return None; }
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
            if ex & WS_EX_TRANSPARENT != 0 || ex & WS_EX_NOACTIVATE != 0 { return None; }
            if ex & WS_EX_TOOLWINDOW != 0 && ex & WS_EX_APPWINDOW == 0 { return None; }
            let mut cloaked = 0u32;
            DwmGetWindowAttribute(h, DWMWA_CLOAKED as u32, &mut cloaked as *mut _ as *mut c_void, 4);
            if cloaked != 0 { return None; }
            let title = text(h, false);
            if title.trim().is_empty() { return None; }
            let class = text(h, true);
            if SKIP_CLASSES.contains(&class.as_str()) { return None; }
            let mut pid = 0;
            GetWindowThreadProcessId(h, &mut pid);
            if pid == self.own_pid { return None; }
            let r = frame_rect(h)?;
            if r.w() < 120 || r.h() < 60 { return None; }
            if !self.blacklist.is_empty() {
                let exe = self.exe.entry(pid).or_insert_with(|| exe_name(pid)).clone();
                let tl = title.to_lowercase();
                if self.blacklist.iter().any(|b| exe == *b || tl.contains(b.as_str())) { return None; }
            }
            Some(r)
        }
    }

    pub fn scan(&mut self, world: &mut World) {
        let mut hs: Vec<HWND> = Vec::with_capacity(256);
        unsafe { EnumWindows(Some(enum_cb), &mut hs as *mut _ as LPARAM); }
        let old = std::mem::take(&mut world.wins);
        for h in hs {
            if let Some(r) = self.qualifies(h) {
                // fullscreen windows (RDP, games, video) are the screen itself, not something to climb or throw
                if world.monitors.iter().any(|m| r.l <= m.screen.l && r.t <= m.screen.t && r.r >= m.screen.r && r.b >= m.screen.b) { continue; }
                let prev = old.iter().find(|w| w.hwnd == h as isize).map(|w| w.rect).unwrap_or(r);
                world.wins.push(Win { hwnd: h as isize, rect: r, prev });
            }
        }
        if self.exe.len() > 512 { self.exe.clear(); }
    }

    /// Monitor covered by a fullscreen foreground app (game, video, fullscreen RDP) and that app's exe name.
    /// Shell overlays (taskbar previews, Alt-Tab, Start, notifications) also span the monitor but don't count.
    pub fn fullscreen(&mut self, world: &World) -> Option<(usize, String)> {
        unsafe {
            let fg = GetForegroundWindow();
            if fg.is_null() || IsWindowVisible(fg) == 0 || IsIconic(fg) != 0 { return None; }
            let class = text(fg, true);
            const SHELL: [&str; 6] = ["MultitaskingViewFrame", "TopLevelWindowForOverflowXamlIsland", "Shell_InputSwitchTopLevelWindow", "Shell_Flyout", "ApplicationManager_ImmersiveShellWindow", "Windows.Internal.Shell.TabProxyWindow"];
            if SKIP_CLASSES.contains(&class.as_str()) || SHELL.contains(&class.as_str()) { return None; }
            let ex = GetWindowLongPtrW(fg, GWL_EXSTYLE) as u32;
            if ex & (WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE) != 0 { return None; }
            let mut cloaked = 0u32;
            DwmGetWindowAttribute(fg, DWMWA_CLOAKED as u32, &mut cloaked as *mut _ as *mut c_void, 4);
            if cloaked != 0 { return None; }
            let mut pid = 0;
            GetWindowThreadProcessId(fg, &mut pid);
            if pid == self.own_pid { return None; }
            let mut r: RECT = std::mem::zeroed();
            GetWindowRect(fg, &mut r);
            let mon = world.monitors.iter().position(|m| r.left <= m.screen.l && r.top <= m.screen.t && r.right >= m.screen.r && r.bottom >= m.screen.b)?;
            Some((mon, self.exe.entry(pid).or_insert_with(|| exe_name(pid)).clone()))
        }
    }
}

/// Move another app's window so its visible frame's top-left lands on (l, t). Returns old frame pos.
pub fn move_app_window(h: isize, l: i32, t: i32) -> Option<(i32, i32)> {
    let h = h as HWND;
    unsafe {
        if IsWindow(h) == 0 || IsZoomed(h) != 0 { return None; }
        let mut wr: RECT = std::mem::zeroed();
        GetWindowRect(h, &mut wr);
        let fr = frame_rect(h)?;
        let (ol, ot) = (fr.l - wr.left, fr.t - wr.top);
        SetWindowPos(h, null_mut(), l - ol, t - ot, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS);
        Some((fr.l, fr.t))
    }
}

// ---------------------------------------------------------------- tray & menus

pub fn tray(h: HWND, icon: HICON, op: u32) {
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = h;
        nid.uID = 1;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = icon;
        let tip = wide("Popeto - desktop pets");
        nid.szTip[..tip.len()].copy_from_slice(&tip);
        Shell_NotifyIconW(op, &nid);
    }
}
pub const TRAY_ADD: u32 = NIM_ADD;
pub const TRAY_DEL: u32 = NIM_DELETE;
pub const TRAY_MOD: u32 = NIM_MODIFY;
pub const MSG_RUP: u32 = WM_RBUTTONUP;
pub const MSG_CTX: u32 = WM_CONTEXTMENU;
pub const MSG_LDBL: u32 = WM_LBUTTONDBLCLK;
pub const MSG_LUP: u32 = WM_LBUTTONUP;

pub struct Menu(pub HMENU);

impl Menu {
    pub fn new() -> Self { Menu(unsafe { CreatePopupMenu() }) }
    pub fn item(&self, id: usize, text: &str, checked: bool) -> &Self {
        let t = wide(text);
        unsafe { AppendMenuW(self.0, MF_STRING | if checked { MF_CHECKED } else { 0 }, id, t.as_ptr()); }
        self
    }
    pub fn disabled(&self, text: &str) -> &Self {
        let t = wide(text);
        unsafe { AppendMenuW(self.0, MF_STRING | MF_GRAYED, 0, t.as_ptr()); }
        self
    }
    pub fn sep(&self) -> &Self { unsafe { AppendMenuW(self.0, MF_SEPARATOR, 0, null()); } self }
    pub fn sub(&self, text: &str, m: Menu) -> &Self {
        let t = wide(text);
        unsafe { AppendMenuW(self.0, MF_POPUP, m.0 as usize, t.as_ptr()); }
        std::mem::forget(m);
        self
    }
    /// Show at the cursor and return the chosen id (0 = none).
    pub fn run(self, owner: HWND) -> usize {
        unsafe {
            let (x, y) = cursor();
            SetForegroundWindow(owner);
            let r = TrackPopupMenu(self.0, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, x, y, 0, owner, null());
            PostMessageW(owner, WM_NULL, 0, 0);
            DestroyMenu(self.0);
            r as usize
        }
    }
}

pub fn open(path: &Path) {
    let p = wide(&path.to_string_lossy());
    let op = wide("open");
    unsafe { ShellExecuteW(null_mut(), op.as_ptr(), p.as_ptr(), null(), null(), SW_SHOWNORMAL); }
}

pub fn message(text: &str) {
    let t = wide(text);
    let c = wide("Popeto");
    unsafe { MessageBoxW(null_mut(), t.as_ptr(), c.as_ptr(), MB_OK | MB_ICONINFORMATION); }
}

pub fn play_sound(p: &Path) {
    use windows_sys::Win32::Media::Audio::*;
    let w = wide(&p.to_string_lossy());
    unsafe { PlaySoundW(w.as_ptr(), null_mut(), SND_ASYNC | SND_FILENAME | SND_NODEFAULT); }
}
