//! How loud the speakers are right now (Windows' own peak meter, the one in the volume mixer).
//! Used to make pets dance to music. Plain COM calls, no audio capture, no hooks.

use std::ffi::c_void;
use std::ptr::null_mut;
use windows_sys::core::GUID;
use windows_sys::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED};

const CLSID_MM_DEVICE_ENUMERATOR: GUID = GUID::from_u128(0xBCDE0395_E52F_467C_8E3D_C4579291692E);
const IID_IMM_DEVICE_ENUMERATOR: GUID = GUID::from_u128(0xA95664D2_9614_4F35_A746_DE8DB63617E6);
const IID_IAUDIO_METER_INFORMATION: GUID = GUID::from_u128(0xC02216F6_8C67_4B5B_9D00_D008E73E0064);

/// Call vtable slot `n` of a COM object.
pub(crate) unsafe fn slot<F: Copy>(obj: *mut c_void, n: usize) -> F {
    let vt = *(obj as *const *const usize);
    std::mem::transmute_copy(&*vt.add(n))
}

pub(crate) unsafe fn release(obj: *mut c_void) {
    if !obj.is_null() { slot::<unsafe extern "system" fn(*mut c_void) -> u32>(obj, 2)(obj); }
}

pub(crate) fn com_init() { unsafe { CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED as u32); } }

/// Peak meter of the default output device, reopened now and then (headphones plugged in, ...).
pub struct Meter {
    meter: *mut c_void,
    age: u32,
}

impl Default for Meter { fn default() -> Self { Meter { meter: null_mut(), age: 0 } } }

impl Meter {
    fn open(&mut self) {
        unsafe {
            release(self.meter);
            self.meter = null_mut();
            com_init();
            let mut en: *mut c_void = null_mut();
            if CoCreateInstance(&CLSID_MM_DEVICE_ENUMERATOR, null_mut(), CLSCTX_ALL, &IID_IMM_DEVICE_ENUMERATOR, &mut en) != 0 || en.is_null() { return; }
            let mut dev: *mut c_void = null_mut();
            // GetDefaultAudioEndpoint(eRender, eMultimedia)
            let get: unsafe extern "system" fn(*mut c_void, i32, i32, *mut *mut c_void) -> i32 = slot(en, 4);
            if get(en, 0, 1, &mut dev) == 0 && !dev.is_null() {
                let act: unsafe extern "system" fn(*mut c_void, *const GUID, u32, *const c_void, *mut *mut c_void) -> i32 = slot(dev, 3);
                let mut m: *mut c_void = null_mut();
                if act(dev, &IID_IAUDIO_METER_INFORMATION, CLSCTX_ALL, std::ptr::null(), &mut m) == 0 { self.meter = m; }
                release(dev);
            }
            release(en);
        }
    }

    /// Current peak 0..1 (call about every tick).
    pub fn peak(&mut self) -> f32 {
        self.age += 1;
        if self.meter.is_null() && self.age % 400 != 1 { return 0.0; }
        if self.age % 400 == 1 { self.open(); }
        if self.meter.is_null() { return 0.0; }
        let mut v = 0f32;
        unsafe {
            let get: unsafe extern "system" fn(*mut c_void, *mut f32) -> i32 = slot(self.meter, 3);
            if get(self.meter, &mut v) != 0 { return 0.0; }
        }
        v
    }
}

impl Drop for Meter { fn drop(&mut self) { unsafe { release(self.meter); } } }
