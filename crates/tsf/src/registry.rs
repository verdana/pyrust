use std::io::Write;
use windows::core::{GUID, HSTRING};
use windows::Win32::Foundation::WIN32_ERROR;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_SZ,
};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, ITfCategoryMgr, ITfInputProcessorProfileMgr, GUID_TFCAT_TIPCAP_COMLESS,
    GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT, GUID_TFCAT_TIPCAP_UIELEMENTENABLED,
    GUID_TFCAT_TIP_KEYBOARD,
};

fn reg_log(msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("C:\\Users\\Verdana\\pyrust_tsf.log")
    {
        let _ = writeln!(f, "{msg}");
        let _ = f.flush();
    }
}

pub const CLSID_PYRUST_TIP: GUID = GUID::from_u128(0xD4B3_C2A1_9F8E_7D6C_5B4A3928174655AA);
pub const PROFILE_GUID: GUID = GUID::from_u128(0xE5C4_B3A2_0F9E_8D7C_6B5A4938271655BB);

const CLSID_TF_INPUTPROCESSORPROFILES: GUID =
    GUID::from_u128(0x33C53A50_F456_4884_B049_85FD643ECFED);
const LANG_CHINESE_SIMPLIFIED: u16 = 0x0804;
const IME_DISPLAY_NAME: &str = "Zero Pinyin";

fn guid_to_string(g: &GUID) -> String {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        g.data1,
        g.data2,
        g.data3,
        g.data4[0],
        g.data4[1],
        g.data4[2],
        g.data4[3],
        g.data4[4],
        g.data4[5],
        g.data4[6],
        g.data4[7],
    )
}

fn check(err: WIN32_ERROR) -> Result<(), String> {
    if err.is_ok() {
        Ok(())
    } else {
        Err(format!("WIN32_ERROR: {:?}", err))
    }
}

unsafe fn reg_create(hive: HKEY, path: &str, default_val: &str) -> Result<(), String> {
    let path_h = HSTRING::from(path);
    let val_h = HSTRING::from(default_val);
    let mut hkey = std::mem::zeroed();
    check(RegCreateKeyW(hive, &path_h, &mut hkey))?;
    let data: &[u8] = std::slice::from_raw_parts(val_h.as_ptr() as *const u8, val_h.len() * 2);
    let _ = RegSetValueExW(hkey, None, Some(0), REG_SZ, Some(data));
    let _ = RegCloseKey(hkey);
    Ok(())
}

unsafe fn reg_create_empty(hive: HKEY, path: &str) -> Result<(), String> {
    let path_h = HSTRING::from(path);
    let mut hkey = std::mem::zeroed();
    check(RegCreateKeyW(hive, &path_h, &mut hkey))?;
    let _ = RegCloseKey(hkey);
    Ok(())
}

unsafe fn reg_set_value(hive: HKEY, key_path: &str, name: &str, value: &str) -> Result<(), String> {
    let path_h = HSTRING::from(key_path);
    let val_h = HSTRING::from(value);
    let mut hkey = std::mem::zeroed();
    check(RegCreateKeyW(hive, &path_h, &mut hkey))?;
    let data: &[u8] = std::slice::from_raw_parts(val_h.as_ptr() as *const u8, val_h.len() * 2);
    if name.is_empty() {
        let _ = RegSetValueExW(hkey, None, Some(0), REG_SZ, Some(data));
    } else {
        let name_h = HSTRING::from(name);
        let _ = RegSetValueExW(hkey, &name_h, Some(0), REG_SZ, Some(data));
    }
    let _ = RegCloseKey(hkey);
    Ok(())
}

fn register_categories_via_com() -> Result<(), String> {
    reg_log("[tsf] register_categories_via_com BEGIN");
    unsafe {
        let cat_mgr: ITfCategoryMgr =
            CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER).map_err(|e| {
                reg_log(&format!("[tsf] CoCreateInstance(CategoryMgr) failed: {e}"));
                format!("CoCreateInstance(CategoryMgr) failed: {e}")
            })?;
        reg_log("[tsf] ITfCategoryMgr created");

        // Keyboard category — essential for appearing as an IME
        cat_mgr
            .RegisterCategory(
                &CLSID_PYRUST_TIP,
                &GUID_TFCAT_TIP_KEYBOARD,
                &CLSID_PYRUST_TIP,
            )
            .map_err(|e| {
                reg_log(&format!("[tsf] RegisterCategory(KEYBOARD) failed: {e}"));
                format!("RegisterCategory(KEYBOARD) failed: {e}")
            })?;
        reg_log("[tsf] RegisterCategory(KEYBOARD) OK");

        // Immersive support — required for UWP / modern apps on Win8+
        cat_mgr
            .RegisterCategory(
                &CLSID_PYRUST_TIP,
                &GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
                &CLSID_PYRUST_TIP,
            )
            .map_err(|e| {
                reg_log(&format!("[tsf] RegisterCategory(IMMERSIVE) failed: {e}"));
                format!("RegisterCategory(IMMERSIVE) failed: {e}")
            })?;
        reg_log("[tsf] RegisterCategory(IMMERSIVE) OK");

        // UIElement enabled — allows modern candidate window
        cat_mgr
            .RegisterCategory(
                &CLSID_PYRUST_TIP,
                &GUID_TFCAT_TIPCAP_UIELEMENTENABLED,
                &CLSID_PYRUST_TIP,
            )
            .map_err(|e| {
                reg_log(&format!("[tsf] RegisterCategory(UIELEMENT) failed: {e}"));
                format!("RegisterCategory(UIELEMENT) failed: {e}")
            })?;
        reg_log("[tsf] RegisterCategory(UIELEMENT) OK");

        // COMLESS — modern TIP registration
        let _ = cat_mgr.RegisterCategory(
            &CLSID_PYRUST_TIP,
            &GUID_TFCAT_TIPCAP_COMLESS,
            &CLSID_PYRUST_TIP,
        );
        reg_log("[tsf] RegisterCategory(COMLESS) done");

        reg_log("[tsf] register_categories_via_com END");
        Ok(())
    }
}

fn register_profile_via_com() -> Result<(), String> {
    reg_log("[tsf] register_profile_via_com BEGIN");
    unsafe {
        match CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() {
            Ok(()) => reg_log("[tsf] CoInitializeEx OK"),
            Err(e) => {
                reg_log(&format!("[tsf] CoInitializeEx failed: {:?}", e));
                return Err(format!("CoInitializeEx failed: {:?}", e));
            }
        }

        let result = (|| -> Result<(), String> {
            let ppm: ITfInputProcessorProfileMgr =
                CoCreateInstance(&CLSID_TF_INPUTPROCESSORPROFILES, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| {
                        reg_log(&format!("[tsf] CoCreateInstance(ProfileMgr) failed: {e}"));
                        format!("CoCreateInstance failed: {e}")
                    })?;
            reg_log("[tsf] ITfInputProcessorProfileMgr created");

            let desc_h = HSTRING::from(IME_DISPLAY_NAME);
            let desc_wide: &[u16] = std::slice::from_raw_parts(desc_h.as_ptr(), desc_h.len());
            let icon_empty: &[u16] = &[];

            ppm.RegisterProfile(
                &CLSID_PYRUST_TIP,
                LANG_CHINESE_SIMPLIFIED,
                &PROFILE_GUID,
                desc_wide,
                icon_empty,
                0u32,
                HKL::default(),
                0u32,
                true,
                0u32,
            )
            .map_err(|e| {
                reg_log(&format!("[tsf] RegisterProfile failed: {e}"));
                format!("RegisterProfile failed: {e}")
            })?;
            reg_log("[tsf] RegisterProfile OK");

            Ok(())
        })();

        CoUninitialize();
        reg_log(&format!(
            "[tsf] register_profile_via_com result={}",
            result.is_ok()
        ));
        result
    }
}

pub fn register_tip() -> Result<(), String> {
    reg_log("[tsf] register_tip BEGIN");
    let clsid_str = guid_to_string(&CLSID_PYRUST_TIP);
    let profile_str = guid_to_string(&PROFILE_GUID);

    reg_log(&format!("[tsf] CLSID={clsid_str}"));
    reg_log(&format!("[tsf] Profile={profile_str}"));

    let dll_path_str = crate::dll_exports::get_dll_path()?;
    reg_log(&format!("[tsf] DLL path resolved: {dll_path_str}"));

    // ── Step 1: COM In-Proc Server Registration (HKLM) ──
    unsafe {
        let com_key = format!("SOFTWARE\\Classes\\CLSID\\{clsid_str}");
        reg_create(HKEY_LOCAL_MACHINE, &com_key, "Zero Pinyin IME")?;

        let inproc = format!("{com_key}\\InprocServer32");
        reg_create(HKEY_LOCAL_MACHINE, &inproc, &dll_path_str)?;
        reg_set_value(HKEY_LOCAL_MACHINE, &inproc, "ThreadingModel", "Apartment")?;
        reg_log("[tsf] COM InprocServer32 registered");

        // ── Step 2: TSF TIP Registration (HKLM) ──
        let tip_key = format!("SOFTWARE\\Microsoft\\CTF\\TIP\\{clsid_str}");
        reg_create(HKEY_LOCAL_MACHINE, &tip_key, "Zero Pinyin IME")?;

        // Category items — correct format: Category\Item\{CAT_GUID}\{CLSID}
        let cat_kbd_guid = guid_to_string(&GUID_TFCAT_TIP_KEYBOARD);
        let cat_kbd_item = format!("{tip_key}\\Category\\Item\\{cat_kbd_guid}\\{clsid_str}");
        reg_create_empty(HKEY_LOCAL_MACHINE, &cat_kbd_item)?;
        reg_log("[tsf] Category Item KEYBOARD registered");

        let cat_imm_guid = guid_to_string(&GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT);
        let cat_imm_item = format!("{tip_key}\\Category\\Item\\{cat_imm_guid}\\{clsid_str}");
        reg_create_empty(HKEY_LOCAL_MACHINE, &cat_imm_item)?;
        reg_log("[tsf] Category Item IMMERSIVESUPPORT registered");

        let cat_ui_guid = guid_to_string(&GUID_TFCAT_TIPCAP_UIELEMENTENABLED);
        let cat_ui_item = format!("{tip_key}\\Category\\Item\\{cat_ui_guid}\\{clsid_str}");
        reg_create_empty(HKEY_LOCAL_MACHINE, &cat_ui_item)?;
        reg_log("[tsf] Category Item UIELEMENTENABLED registered");

        // LanguageProfile — CRITICAL: tells Windows which language this TIP belongs to
        let lang_profile_key =
            format!("{tip_key}\\LanguageProfile\\0x{LANG_CHINESE_SIMPLIFIED:08X}\\{profile_str}");
        reg_create(HKEY_LOCAL_MACHINE, &lang_profile_key, "Zero Pinyin")?;
        reg_set_value(HKEY_LOCAL_MACHINE, &lang_profile_key, "Profile", "")?;
        reg_set_value(
            HKEY_LOCAL_MACHINE,
            &lang_profile_key,
            "Description",
            "Zero Pinyin IME",
        )?;
        reg_set_value(HKEY_LOCAL_MACHINE, &lang_profile_key, "IconFile", "")?;
        reg_set_value(HKEY_LOCAL_MACHINE, &lang_profile_key, "IconIndex", "0")?;
        reg_log("[tsf] LanguageProfile registered");

        // Profile key at tip level
        let prof_key = format!("{tip_key}\\Profile\\{profile_str}");
        reg_create(HKEY_LOCAL_MACHINE, &prof_key, "Zero Pinyin")?;
        reg_set_value(
            HKEY_LOCAL_MACHINE,
            &prof_key,
            "Description",
            "Zero Pinyin IME",
        )?;
        reg_set_value(HKEY_LOCAL_MACHINE, &prof_key, "IconFile", "")?;
        reg_set_value(HKEY_LOCAL_MACHINE, &prof_key, "IconIndex", "0")?;
        reg_log("[tsf] Profile registered");

        // ── Step 3: Per-user TSF registration (HKCU) ──
        let user_tip_key = format!("Software\\Microsoft\\CTF\\TIP\\{clsid_str}");
        let _ = reg_create(HKEY_CURRENT_USER, &user_tip_key, "Zero Pinyin IME");
        let user_lang_key = format!(
            "{user_tip_key}\\LanguageProfile\\0x{LANG_CHINESE_SIMPLIFIED:08X}\\{profile_str}"
        );
        let _ = reg_create(HKEY_CURRENT_USER, &user_lang_key, "Zero Pinyin");
        reg_log("[tsf] HKCU registration done");

        reg_log("[tsf] All registry keys written");
    }

    // ── Step 4: COM registration (Profile + Categories) ──
    register_profile_via_com()?;
    register_categories_via_com()?;

    reg_log("[tsf] register_tip END — SUCCESS");
    Ok(())
}

pub fn unregister_tip() -> Result<(), String> {
    reg_log("[tsf] unregister_tip BEGIN");

    let clsid_str = guid_to_string(&CLSID_PYRUST_TIP);
    let _profile_str = guid_to_string(&PROFILE_GUID);

    unsafe {
        // COM unregister
        match CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() {
            Ok(()) => {
                let _ = (|| -> Result<(), String> {
                    let ppm: ITfInputProcessorProfileMgr = CoCreateInstance(
                        &CLSID_TF_INPUTPROCESSORPROFILES,
                        None,
                        CLSCTX_INPROC_SERVER,
                    )
                    .map_err(|e| format!("CoCreateInstance failed: {e}"))?;
                    ppm.UnregisterProfile(
                        &CLSID_PYRUST_TIP,
                        LANG_CHINESE_SIMPLIFIED,
                        &PROFILE_GUID,
                        0,
                    )
                    .map_err(|e| format!("UnregisterProfile failed: {e}"))?;
                    reg_log("[tsf] UnregisterProfile OK");
                    Ok(())
                })();

                let _ = (|| -> Result<(), String> {
                    let cat_mgr: ITfCategoryMgr =
                        CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)
                            .map_err(|e| format!("CoCreateInstance(CategoryMgr) failed: {e}"))?;
                    cat_mgr
                        .UnregisterCategory(
                            &CLSID_PYRUST_TIP,
                            &GUID_TFCAT_TIP_KEYBOARD,
                            &CLSID_PYRUST_TIP,
                        )
                        .map_err(|e| format!("UnregisterCategory failed: {e}"))?;
                    reg_log("[tsf] UnregisterCategory OK");
                    Ok(())
                })();

                CoUninitialize();
            }
            Err(e) => reg_log(&format!("[tsf] unregister: CoInitEx failed: {e:?}")),
        }
    }

    reg_log("[tsf] unregister_tip END. Manual cleanup if needed.");
    reg_log(&format!(
        "  Delete HKLM\\SOFTWARE\\Microsoft\\CTF\\TIP\\{clsid_str}"
    ));
    reg_log(&format!("  Delete HKCR\\CLSID\\{clsid_str}"));
    Ok(())
}
