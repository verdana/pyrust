use windows::core::{GUID, HRESULT, Interface};
use windows::Win32::Foundation::{BOOL, HINSTANCE, S_FALSE, S_OK};
use windows::Win32::System::Com::IClassFactory;
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;

use crate::registry::{register_tip, unregister_tip, CLSID_PYRUST_TIP};
use crate::tip::{DLL_REF_COUNT, PyrustClassFactory};

static mut DLL_HINSTANCE: HINSTANCE = HINSTANCE(0);

#[no_mangle]
extern "system" fn DllMain(
    hinst: HINSTANCE,
    reason: u32,
    _reserved: *mut std::ffi::c_void,
) -> BOOL {
    if reason == 1 {
        // DLL_PROCESS_ATTACH
        unsafe { DLL_HINSTANCE = hinst; }
    }
    BOOL(1)
}

pub fn get_dll_path() -> Result<String, String> {
    let hinst = unsafe { DLL_HINSTANCE };
    if hinst.is_invalid() {
        return Err("DLL HINSTANCE not set".into());
    }
    let mut buf = vec![0u16; 260];
    let len = unsafe { GetModuleFileNameW(hinst, &mut buf) as usize };
    if len == 0 || len >= buf.len() {
        return Err("GetModuleFileNameW failed".into());
    }
    String::from_utf16(&buf[..len]).map_err(|e| format!("UTF-16 decode failed: {e}"))
}

#[no_mangle]
extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut std::ffi::c_void,
) -> HRESULT {
    unsafe {
        if *rclsid != CLSID_PYRUST_TIP {
            return HRESULT(0x80040111u32 as i32); // CLASS_E_CLASSNOTAVAILABLE
        }
        let factory: IClassFactory = PyrustClassFactory {}.into();
        factory.query(riid, ppv)
    }
}

#[no_mangle]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    if DLL_REF_COUNT.load(std::sync::atomic::Ordering::Acquire) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

#[no_mangle]
extern "system" fn DllRegisterServer() -> HRESULT {
    match register_tip() {
        Ok(()) => S_OK,
        Err(e) => {
            log::error!("DllRegisterServer failed: {}", e);
            HRESULT(0x80004005u32 as i32)
        }
    }
}

#[no_mangle]
extern "system" fn DllUnregisterServer() -> HRESULT {
    match unregister_tip() {
        Ok(()) => S_OK,
        Err(e) => {
            log::error!("DllUnregisterServer failed: {}", e);
            HRESULT(0x80004005u32 as i32)
        }
    }
}
