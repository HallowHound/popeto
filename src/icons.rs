//! Where the desktop icons are, so pets can stand on them. Asks Explorer through the documented
//! Shell COM interfaces (IShellWindows > IShellBrowser > IFolderView): read only, nothing injected.

use crate::audio::{com_init, release, slot};
use crate::env::Rect;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL};
use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowExW, IsWindowVisible};

const CLSID_SHELL_WINDOWS: GUID = GUID::from_u128(0x9BA05972_F6A8_11CF_A442_00A0C90A8F39);
const IID_ISHELL_WINDOWS: GUID = GUID::from_u128(0x85CB6900_4D95_11CF_960C_0080C7F4EE85);
const IID_ISERVICE_PROVIDER: GUID = GUID::from_u128(0x6D5140C1_7436_11CE_8034_00AA006009FA);
const SID_STOP_LEVEL_BROWSER: GUID = GUID::from_u128(0x4C96BE40_915C_11CF_99D3_00AA004AE837);
const IID_ISHELL_BROWSER: GUID = GUID::from_u128(0x000214E2_0000_0000_C000_000000000046);
const IID_IFOLDER_VIEW: GUID = GUID::from_u128(0xCDE725B0_CCC9_4519_917E_325D72FAB4CE);
const IID_IFOLDER_VIEW2: GUID = GUID::from_u128(0x1AF3A467_214F_4298_908E_06B03E0B39F9);

type Qi = unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32;

unsafe fn qi(obj: *mut c_void, iid: &GUID) -> *mut c_void {
    let mut out = null_mut();
    if slot::<Qi>(obj, 0)(obj, iid, &mut out) != 0 { return null_mut(); }
    out
}

/// Screen rectangles of the visible desktop icons' pictures (empty if icons are hidden or Explorer isn't there).
pub fn desktop_icons() -> Vec<Rect> {
    unsafe {
        com_init();
        let mut out = Vec::new();
        let mut sw: *mut c_void = null_mut();
        if CoCreateInstance(&CLSID_SHELL_WINDOWS, null_mut(), CLSCTX_ALL, &IID_ISHELL_WINDOWS, &mut sw) != 0 || sw.is_null() { return out; }
        // FindWindowSW(&empty, &empty, SWC_DESKTOP, &hwnd, SWFO_NEEDDISPATCH, &disp)
        let empty = [0u64; 3];
        let mut hwnd = 0i32;
        let mut disp: *mut c_void = null_mut();
        let find: unsafe extern "system" fn(*mut c_void, *const c_void, *const c_void, i32, *mut i32, i32, *mut *mut c_void) -> i32 = slot(sw, 15);
        let ok = find(sw, empty.as_ptr() as _, empty.as_ptr() as _, 8, &mut hwnd, 1, &mut disp) == 0 && !disp.is_null();
        release(sw);
        if !ok { return out; }
        let sp = qi(disp, &IID_ISERVICE_PROVIDER);
        release(disp);
        if sp.is_null() { return out; }
        let mut browser: *mut c_void = null_mut();
        let qs: unsafe extern "system" fn(*mut c_void, *const GUID, *const GUID, *mut *mut c_void) -> i32 = slot(sp, 3);
        let r = qs(sp, &SID_STOP_LEVEL_BROWSER, &IID_ISHELL_BROWSER, &mut browser);
        release(sp);
        if r != 0 || browser.is_null() { return out; }
        let mut view: *mut c_void = null_mut();
        let active: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32 = slot(browser, 15);
        let r = active(browser, &mut view);
        release(browser);
        if r != 0 || view.is_null() { return out; }
        // the list of icons lives in SHELLDLL_DefView > SysListView32
        let mut defview: HWND = null_mut();
        let get_window: unsafe extern "system" fn(*mut c_void, *mut HWND) -> i32 = slot(view, 3);
        get_window(view, &mut defview);
        let fv = qi(view, &IID_IFOLDER_VIEW);
        let fv2 = qi(view, &IID_IFOLDER_VIEW2);
        release(view);
        let cls = crate::win::wide("SysListView32");
        let lv = if defview.is_null() { null_mut() } else { FindWindowExW(defview, null_mut(), cls.as_ptr(), null()) };
        if fv.is_null() || lv.is_null() || IsWindowVisible(lv) == 0 {
            release(fv);
            release(fv2);
            return out;
        }
        let dpi = windows_sys::Win32::UI::HiDpi::GetDpiForWindow(lv).max(96) as f64 / 96.0;
        let mut spacing = POINT { x: 0, y: 0 };
        slot::<unsafe extern "system" fn(*mut c_void, *mut POINT) -> i32>(fv, 12)(fv, &mut spacing);
        let mut icon = (48.0 * dpi) as i32;
        if !fv2.is_null() {
            // IFolderView2::GetViewModeAndIconSize
            let (mut mode, mut size) = (0i32, 0i32);
            if slot::<unsafe extern "system" fn(*mut c_void, *mut i32, *mut i32) -> i32>(fv2, 36)(fv2, &mut mode, &mut size) == 0 && size > 0 {
                icon = (size as f64 * dpi) as i32;
            }
            release(fv2);
        }
        if spacing.x > 0 { icon = icon.min(spacing.x); }
        let mut n = 0i32;
        slot::<unsafe extern "system" fn(*mut c_void, u32, *mut i32) -> i32>(fv, 7)(fv, 2 /* SVGIO_ALLVIEW */, &mut n);
        let item: unsafe extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> i32 = slot(fv, 6);
        let pos: unsafe extern "system" fn(*mut c_void, *const c_void, *mut POINT) -> i32 = slot(fv, 11);
        for i in 0..n.min(500) {
            let mut pidl: *mut c_void = null_mut();
            if item(fv, i, &mut pidl) != 0 || pidl.is_null() { continue; }
            let mut p = POINT { x: 0, y: 0 };
            let r = pos(fv, pidl, &mut p);
            CoTaskMemFree(pidl);
            if r != 0 { continue; }
            ClientToScreen(lv, &mut p);
            let cell = if spacing.x > 0 { spacing.x } else { icon + (28.0 * dpi) as i32 };
            let l = p.x + (cell - icon) / 2;
            let t = p.y + (2.0 * dpi) as i32;
            out.push(Rect { l, t, r: l + icon, b: t + icon });
        }
        release(fv);
        out
    }
}
