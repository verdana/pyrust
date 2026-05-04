use std::io::Write;
use windows::core::{Interface, GUID, HRESULT};
use windows::Win32::Foundation::{HMODULE, S_FALSE, S_OK};
use windows::Win32::System::Com::IClassFactory;
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;

use crate::registry::{register_tip, unregister_tip, CLSID_PYRUST_TIP};
use crate::tip::{PyrustClassFactory, DLL_REF_COUNT};

fn dll_log(msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("C:\\Users\\Verdana\\pyrust_tsf.log")
    {
        let _ = writeln!(f, "{msg}");
        let _ = f.flush();
    }
}

static mut DLL_HINSTANCE: HMODULE = HMODULE(std::ptr::null_mut());

#[no_mangle]
extern "system" fn DllMain(hinst: HMODULE, reason: u32, _reserved: *mut std::ffi::c_void) -> bool {
    if reason == 1 {
        dll_log("[tsf] DllMain: DLL_PROCESS_ATTACH");
        unsafe {
            DLL_HINSTANCE = hinst;
        }
    } else if reason == 0 {
        dll_log("[tsf] DllMain: DLL_PROCESS_DETACH");
    }
    true
}

pub fn get_dll_path() -> Result<String, String> {
    let hinst = unsafe { DLL_HINSTANCE };
    if hinst.is_invalid() {
        return Err("DLL HINSTANCE not set".into());
    }
    let mut buf = vec![0u16; 260];
    let len = unsafe { GetModuleFileNameW(Some(hinst), &mut buf) as usize };
    if len == 0 || len >= buf.len() {
        return Err("GetModuleFileNameW failed".into());
    }
    let path = String::from_utf16(&buf[..len]).map_err(|e| format!("UTF-16 decode failed: {e}"))?;
    dll_log(&format!("[tsf] DLL path: {path}"));
    Ok(path)
}

#[no_mangle]
extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut std::ffi::c_void,
) -> HRESULT {
    dll_log("[tsf] DllGetClassObject called");
    unsafe {
        if *rclsid != CLSID_PYRUST_TIP {
            dll_log("[tsf] DllGetClassObject: wrong CLSID -> CLASS_E_CLASSNOTAVAILABLE");
            return HRESULT(0x80040111u32 as i32);
        }
        let factory: IClassFactory = PyrustClassFactory {}.into();
        let hr = factory.query(riid, ppv);
        dll_log(&format!("[tsf] DllGetClassObject: query returned {:?}", hr));
        hr
    }
}

#[no_mangle]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    let count = DLL_REF_COUNT.load(std::sync::atomic::Ordering::Acquire);
    dll_log(&format!("[tsf] DllCanUnloadNow refcount={count}"));
    if count == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

#[no_mangle]
extern "system" fn DllRegisterServer() -> HRESULT {
    dll_log("[tsf] DllRegisterServer BEGIN");
    match register_tip() {
        Ok(()) => {
            dll_log("[tsf] DllRegisterServer SUCCESS");
            S_OK
        }
        Err(e) => {
            dll_log(&format!("[tsf] DllRegisterServer FAILED: {e}"));
            HRESULT(0x80004005u32 as i32)
        }
    }
}

#[no_mangle]
extern "system" fn DllUnregisterServer() -> HRESULT {
    dll_log("[tsf] DllUnregisterServer BEGIN");
    match unregister_tip() {
        Ok(()) => {
            dll_log("[tsf] DllUnregisterServer SUCCESS");
            S_OK
        }
        Err(e) => {
            dll_log(&format!("[tsf] DllUnregisterServer FAILED: {e}"));
            HRESULT(0x80004005u32 as i32)
        }
    }
}
