use windows::core::{GUID, HRESULT, Interface};
use windows::Win32::Foundation::{S_OK, S_FALSE};
use windows::Win32::System::Com::IClassFactory;

use crate::registry::{register_tip, unregister_tip, CLSID_PYRUST_TIP};
use crate::tip::{DLL_REF_COUNT, PyrustClassFactory};

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
