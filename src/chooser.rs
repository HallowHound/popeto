//! "Choose your pets" window: a scrollable grid of pet previews with names.
//! Click toggles a pet, double-click keeps only that one.

use crate::i18n::{t, tf};
use crate::win::{self, wide};
use std::cell::RefCell;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::SetScrollInfo;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub struct Choice {
    pub name: String,
    pub preview: Option<PathBuf>,
    pub selected: bool,
}

struct Tile {
    name: Vec<u16>,
    bmp: HBITMAP,
    w: i32,
    h: i32,
    sel: bool,
}

struct State {
    tiles: Vec<Tile>,
    s: f64,
    cols: i32,
    view_h: i32,
    scroll: i32,
    grid: HWND,
    label: HWND,
    ok: HWND,
    font: HFONT,
    bold: HFONT,
    done: Option<bool>,
}

thread_local! { static ST: RefCell<Option<State>> = const { RefCell::new(None) }; }

const MAIN_CLASS: &str = "PopetoChooser";
const GRID_CLASS: &str = "PopetoChooserGrid";
const ID_OK: usize = 1;
const ID_CANCEL: usize = 2;
const ID_ALL: usize = 3;
const ID_NONE: usize = 4;
const ID_STARTUP: usize = 5;

fn px(s: f64, v: i32) -> i32 { (v as f64 * s).round() as i32 }
fn rgb(r: u8, g: u8, b: u8) -> u32 { r as u32 | (g as u32) << 8 | (b as u32) << 16 }

impl State {
    fn tile_w(&self) -> i32 { px(self.s, 136) }
    fn tile_h(&self) -> i32 { px(self.s, 160) }
    fn pad(&self) -> i32 { px(self.s, 12) }
    fn img(&self) -> i32 { px(self.s, 104) }
    fn content_h(&self) -> i32 {
        let rows = (self.tiles.len() as i32 + self.cols - 1) / self.cols;
        rows * (self.tile_h() + self.pad()) + self.pad()
    }
    fn max_scroll(&self) -> i32 { (self.content_h() - self.view_h).max(0) }
    fn hit(&self, x: i32, y: i32) -> Option<usize> {
        let (tw, th, p) = (self.tile_w(), self.tile_h(), self.pad());
        let (x, y) = (x - p, y + self.scroll - p);
        if x < 0 || y < 0 { return None; }
        let (c, r) = (x / (tw + p), y / (th + p));
        if c >= self.cols || x % (tw + p) >= tw || y % (th + p) >= th { return None; }
        let i = (r * self.cols + c) as usize;
        (i < self.tiles.len()).then_some(i)
    }
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> { ST.with(|s| s.borrow_mut().as_mut().map(f)) }

/// Refresh the "N of M selected" line, OK button and grid. Called with no state borrow held.
fn refresh() {
    let Some((label, ok, grid, n, total)) = with(|s| (s.label, s.ok, s.grid, s.tiles.iter().filter(|t| t.sel).count(), s.tiles.len())) else { return };
    let text = wide(&tf("Click a pet to switch it on or off. Double-click keeps only that one.    {} of {} selected", &[&n, &total]));
    unsafe {
        SetWindowTextW(label, text.as_ptr());
        EnableWindow(ok, (n > 0) as i32);
        InvalidateRect(grid, null(), 0);
    }
}

fn scroll_to(pos: i32) {
    let Some((grid, pos, max, page)) = with(|s| {
        s.scroll = pos.clamp(0, s.max_scroll());
        (s.grid, s.scroll, s.content_h(), s.view_h)
    }) else { return };
    unsafe {
        let si = SCROLLINFO { cbSize: std::mem::size_of::<SCROLLINFO>() as u32, fMask: SIF_RANGE | SIF_PAGE | SIF_POS, nMin: 0, nMax: max - 1, nPage: page as u32, nPos: pos, nTrackPos: 0 };
        SetScrollInfo(grid, SB_VERT, &si, 1);
        InvalidateRect(grid, null(), 0);
    }
}

fn wheel(wp: WPARAM) {
    let delta = ((wp >> 16) & 0xFFFF) as i16 as i32;
    if let Some((cur, step)) = with(|s| (s.scroll, s.tile_h() / 2)) { scroll_to(cur - delta * step / 120); }
}

unsafe fn paint_grid(h: HWND) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(h, &mut ps);
    let mut rc: RECT = std::mem::zeroed();
    GetClientRect(h, &mut rc);
    let (w, hh) = (rc.right, rc.bottom);
    let mem = CreateCompatibleDC(hdc);
    let buf = CreateCompatibleBitmap(hdc, w.max(1), hh.max(1));
    let old_buf = SelectObject(mem, buf);
    let src = CreateCompatibleDC(hdc);
    ST.with(|st| {
        let st = st.borrow();
        let Some(s) = st.as_ref() else { return };
        let bg = CreateSolidBrush(rgb(243, 243, 243));
        FillRect(mem, &rc, bg);
        DeleteObject(bg);
        SetBkMode(mem, TRANSPARENT as _);
        let (tw, th, p, img) = (s.tile_w(), s.tile_h(), s.pad(), s.img());
        let radius = px(s.s, 14);
        for (i, t) in s.tiles.iter().enumerate() {
            let (c, r) = (i as i32 % s.cols, i as i32 / s.cols);
            let x = p + c * (tw + p);
            let y = p + r * (th + p) - s.scroll;
            if y + th < 0 || y > hh { continue; }
            // card
            let (fill, edge, ew) = if t.sel { (rgb(226, 238, 255), rgb(59, 130, 246), px(s.s, 2)) } else { (rgb(255, 255, 255), rgb(214, 214, 214), 1) };
            let br = CreateSolidBrush(fill);
            let pen = CreatePen(PS_SOLID, ew, edge);
            let (ob, op) = (SelectObject(mem, br), SelectObject(mem, pen));
            RoundRect(mem, x, y, x + tw, y + th, radius, radius);
            SelectObject(mem, ob);
            SelectObject(mem, op);
            DeleteObject(br);
            DeleteObject(pen);
            // picture, dimmed when off
            if !t.bmp.is_null() {
                let ix = x + (tw - t.w) / 2;
                let iy = y + px(s.s, 12) + (img - t.h);
                let o = SelectObject(src, t.bmp);
                let bf = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: if t.sel { 255 } else { 115 }, AlphaFormat: AC_SRC_ALPHA as u8 };
                AlphaBlend(mem, ix, iy, t.w, t.h, src, 0, 0, t.w, t.h, bf);
                SelectObject(src, o);
            }
            // name
            let of = SelectObject(mem, if t.sel { s.bold } else { s.font });
            SetTextColor(mem, if t.sel { rgb(23, 37, 84) } else { rgb(110, 110, 110) });
            let mut tr = RECT { left: x + px(s.s, 6), top: y + px(s.s, 20) + img, right: x + tw - px(s.s, 6), bottom: y + th - px(s.s, 4) };
            let mut name = t.name.clone();
            DrawTextW(mem, name.as_mut_ptr(), (name.len() - 1) as i32, &mut tr, DT_CENTER | DT_WORDBREAK | DT_END_ELLIPSIS | DT_EDITCONTROL | DT_NOPREFIX);
            SelectObject(mem, of);
            // check badge
            let d = px(s.s, 20);
            let (bx, by) = (x + tw - d - px(s.s, 8), y + px(s.s, 8));
            let br = CreateSolidBrush(if t.sel { rgb(59, 130, 246) } else { rgb(255, 255, 255) });
            let pen = CreatePen(PS_SOLID, 1, if t.sel { rgb(59, 130, 246) } else { rgb(190, 190, 190) });
            let (ob, op) = (SelectObject(mem, br), SelectObject(mem, pen));
            Ellipse(mem, bx, by, bx + d, by + d);
            SelectObject(mem, ob);
            SelectObject(mem, op);
            DeleteObject(br);
            DeleteObject(pen);
            if t.sel {
                let of = SelectObject(mem, s.bold);
                SetTextColor(mem, rgb(255, 255, 255));
                let mut mark = wide("\u{2713}");
                let mut br = RECT { left: bx, top: by, right: bx + d, bottom: by + d };
                DrawTextW(mem, mark.as_mut_ptr(), 1, &mut br, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                SelectObject(mem, of);
            }
        }
    });
    BitBlt(hdc, 0, 0, w, hh, mem, 0, 0, SRCCOPY);
    SelectObject(mem, old_buf);
    DeleteObject(buf);
    DeleteDC(mem);
    DeleteDC(src);
    EndPaint(h, &ps);
}

unsafe extern "system" fn grid_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let (x, y) = ((lp & 0xFFFF) as i16 as i32, ((lp >> 16) & 0xFFFF) as i16 as i32);
    match msg {
        WM_PAINT => { paint_grid(h); return 0; }
        WM_ERASEBKGND => return 1,
        WM_LBUTTONDOWN => {
            SetFocus(h);
            if with(|s| s.hit(x, y).map(|i| s.tiles[i].sel = !s.tiles[i].sel)).flatten().is_some() { refresh(); }
            return 0;
        }
        WM_LBUTTONDBLCLK => {
            if with(|s| s.hit(x, y).map(|i| for (k, t) in s.tiles.iter_mut().enumerate() { t.sel = k == i; })).flatten().is_some() { refresh(); }
            return 0;
        }
        WM_MOUSEWHEEL => { wheel(wp); return 0; }
        WM_VSCROLL => {
            let Some((cur, line, page)) = with(|s| (s.scroll, s.tile_h() / 3, s.view_h)) else { return 0 };
            let pos = match (wp & 0xFFFF) as i32 {
                0 => cur - line,
                1 => cur + line,
                2 => cur - page,
                3 => cur + page,
                4 | 5 => {
                    let mut si: SCROLLINFO = std::mem::zeroed();
                    si.cbSize = std::mem::size_of::<SCROLLINFO>() as u32;
                    si.fMask = SIF_TRACKPOS;
                    GetScrollInfo(h, SB_VERT, &mut si);
                    si.nTrackPos
                }
                6 => 0,
                7 => i32::MAX,
                _ => cur,
            };
            scroll_to(pos);
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(h, msg, wp, lp)
}

unsafe extern "system" fn main_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            match wp & 0xFFFF {
                ID_OK => { with(|s| if s.tiles.iter().any(|t| t.sel) { s.done = Some(true) }); }
                ID_CANCEL => { with(|s| s.done = Some(false)); }
                ID_ALL | ID_NONE => {
                    let on = wp & 0xFFFF == ID_ALL;
                    with(|s| s.tiles.iter_mut().for_each(|t| t.sel = on));
                    refresh();
                }
                _ => {}
            }
            return 0;
        }
        WM_CLOSE => { with(|s| s.done = Some(false)); return 0; }
        WM_MOUSEWHEEL => { wheel(wp); return 0; }
        _ => {}
    }
    DefWindowProcW(h, msg, wp, lp)
}

fn register() {
    unsafe {
        let inst = GetModuleHandleW(null());
        for (name, proc_, style, bg) in [
            (MAIN_CLASS, main_proc as unsafe extern "system" fn(_, _, _, _) -> _, 0, (COLOR_BTNFACE + 1) as usize as HBRUSH),
            (GRID_CLASS, grid_proc, CS_DBLCLKS, null_mut()),
        ] {
            let cls = wide(name);
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style,
                lpfnWndProc: Some(proc_),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: inst,
                hIcon: null_mut(),
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                hbrBackground: bg,
                lpszMenuName: null(),
                lpszClassName: cls.as_ptr(),
                hIconSm: null_mut(),
            };
            RegisterClassExW(&wc);
        }
    }
}

unsafe fn font(s: f64, weight: i32) -> HFONT {
    let face = wide("Segoe UI");
    CreateFontW(-px(s, 14), 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET as _, OUT_DEFAULT_PRECIS as _, CLIP_DEFAULT_PRECIS as _, CLEARTYPE_QUALITY as _, (DEFAULT_PITCH | FF_DONTCARE) as _, face.as_ptr())
}

unsafe fn child(parent: HWND, class: &str, text: &str, style: u32, x: i32, y: i32, w: i32, h: i32, id: usize, f: HFONT) -> HWND {
    let (c, t) = (wide(class), wide(text));
    let hw = CreateWindowExW(0, c.as_ptr(), t.as_ptr(), WS_CHILD | WS_VISIBLE | style, x, y, w, h, parent, id as HMENU, GetModuleHandleW(null()), null());
    SendMessageW(hw, WM_SETFONT, f as WPARAM, 1);
    hw
}

/// Show the chooser modally. Returns the new selection and the "show at startup" flag, or None if cancelled.
pub fn run(items: &[Choice], show_at_startup: bool, icon: HICON) -> Option<(Vec<bool>, bool)> {
    if items.is_empty() { return None; }
    register();
    unsafe {
        let s = windows_sys::Win32::UI::HiDpi::GetDpiForSystem() as f64 / 96.0;
        let n = items.len() as i32;
        let cols = if n <= 4 { 4 } else if n <= 10 { 5 } else { 6 };
        let mut st = State {
            tiles: vec![],
            s,
            cols,
            view_h: 0,
            scroll: 0,
            grid: null_mut(),
            label: null_mut(),
            ok: null_mut(),
            font: font(s, 400),
            bold: font(s, 600),
            done: None,
        };
        let img = st.img() as u32;
        st.tiles = items.iter().map(|c| {
            let (bmp, w, h) = c.preview.as_ref().and_then(|p| win::make_preview(p, img)).unwrap_or((null_mut(), 0, 0));
            Tile { name: wide(&c.name), bmp, w, h, sel: c.selected }
        }).collect();
        let rows = (n + cols - 1) / cols;
        st.view_h = rows.min(3) * (st.tile_h() + st.pad()) + st.pad();
        let sbw = GetSystemMetrics(SM_CXVSCROLL);
        let grid_w = cols * (st.tile_w() + st.pad()) + st.pad() + sbw;
        let m = px(s, 14);
        let (top, bh) = (px(s, 40), px(s, 30));
        let client_w = grid_w + 2 * m;
        let bar_y = top + st.view_h + px(s, 14);
        let client_h = bar_y + bh + px(s, 16);

        let style = WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let mut r = RECT { left: 0, top: 0, right: client_w, bottom: client_h };
        AdjustWindowRectEx(&mut r, style, 0, WS_EX_DLGMODALFRAME);
        let (ww, wh) = (r.right - r.left, r.bottom - r.top);
        let mut work: RECT = std::mem::zeroed();
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut _ as *mut _, 0);
        let (x, y) = (work.left + (work.right - work.left - ww) / 2, work.top + (work.bottom - work.top - wh) / 2);
        let (cls, title) = (wide(MAIN_CLASS), wide(t("Popeto - choose your pets")));
        let hwnd = CreateWindowExW(WS_EX_DLGMODALFRAME | WS_EX_APPWINDOW, cls.as_ptr(), title.as_ptr(), style, x, y, ww, wh, null_mut(), null_mut(), GetModuleHandleW(null()), null());
        if !icon.is_null() {
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as WPARAM, icon as LPARAM);
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as WPARAM, icon as LPARAM);
        }
        let f = st.font;
        st.label = child(hwnd, "STATIC", "", 0x200 /* SS_LEFT | SS_CENTERIMAGE */, m, px(s, 8), grid_w, px(s, 26), 0, f);
        st.grid = child(hwnd, GRID_CLASS, "", WS_VSCROLL | WS_TABSTOP, m, top, grid_w, st.view_h, 0, f);
        let bw = px(s, 104);
        let gap = px(s, 8);
        child(hwnd, "BUTTON", t("Select all"), WS_TABSTOP | BS_PUSHBUTTON as u32, m, bar_y, bw, bh, ID_ALL, f);
        child(hwnd, "BUTTON", t("Select none"), WS_TABSTOP | BS_PUSHBUTTON as u32, m + bw + gap, bar_y, bw, bh, ID_NONE, f);
        let chk = child(hwnd, "BUTTON", t("Show at startup"), WS_TABSTOP | BS_AUTOCHECKBOX as u32, m + 2 * (bw + gap) + gap, bar_y, px(s, 150), bh, ID_STARTUP, f);
        SendMessageW(chk, BM_SETCHECK, if show_at_startup { 1 } else { 0 }, 0);
        st.ok = child(hwnd, "BUTTON", "OK", WS_TABSTOP | BS_DEFPUSHBUTTON as u32, client_w - m - 2 * bw - gap, bar_y, bw, bh, ID_OK, f);
        child(hwnd, "BUTTON", t("Cancel"), WS_TABSTOP | BS_PUSHBUTTON as u32, client_w - m - bw, bar_y, bw, bh, ID_CANCEL, f);
        ST.with(|x| *x.borrow_mut() = Some(st));
        scroll_to(0);
        refresh();
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);

        let mut msg: MSG = std::mem::zeroed();
        let mut quit = false;
        while with(|s| s.done.is_none()).unwrap_or(false) {
            if GetMessageW(&mut msg, null_mut(), 0, 0) <= 0 { quit = true; break; }
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        let startup = SendMessageW(chk, BM_GETCHECK, 0, 0) == 1;
        DestroyWindow(hwnd);
        let st = ST.with(|x| x.borrow_mut().take()).unwrap();
        for t in &st.tiles { if !t.bmp.is_null() { DeleteObject(t.bmp); } }
        DeleteObject(st.font);
        DeleteObject(st.bold);
        if quit { PostQuitMessage(0); }
        (st.done == Some(true)).then(|| (st.tiles.iter().map(|t| t.sel).collect(), startup))
    }
}
