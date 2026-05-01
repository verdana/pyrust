use windows::core::{GUID, HSTRING};
use windows::Win32::System::Registry::{
    RegCreateKeyW, RegSetValueExW, RegCloseKey, HKEY_LOCAL_MACHINE,
    REG_SZ,
};
use windows::Win32::Foundation::WIN32_ERROR;

pub const CLSID_PYRUST_TIP: GUID = GUID::from_u128(0xD4B3_C2A1_9F8E_7D6C_5B4A3928174655AA);
pub const PROFILE_GUID: GUID = GUID::from_u128(0xE5C4_B3A2_0F9E_8D7C_6B5A4938271655BB);
pub const CATEGORY_KEYBOARD: GUID = GUID::from_u128(0x34745C63_B2F0_4784_8B67_5E12C8701A31);
pub const CATEGORY_PROFILE: GUID = GUID::from_u128(0xB814541B_44C3_41CC_927B_34E2BD6DC7C5);

fn guid_to_string(g: &GUID) -> String {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        g.data1, g.data2, g.data3,
        g.data4[0], g.data4[1], g.data4[2], g.data4[3],
        g.data4[4], g.data4[5], g.data4[6], g.data4[7],
    )
}

fn check(err: WIN32_ERROR) -> Result<(), String> {
    if err.is_ok() { Ok(()) } else { Err(format!("WIN32_ERROR: {:?}", err)) }
}

unsafe fn reg_create(path: &str, default_val: &str) -> Result<(), String> {
    let path_h = HSTRING::from(path);
    let val_h = HSTRING::from(default_val);
    let mut hkey = std::mem::zeroed();
    check(RegCreateKeyW(HKEY_LOCAL_MACHINE, &path_h, &mut hkey))?;
    let data: &[u8] = std::slice::from_raw_parts(val_h.as_ptr() as *const u8, val_h.len() * 2);
    let _ = RegSetValueExW(hkey, None, 0, REG_SZ, Some(data));
    let _ = RegCloseKey(hkey);
    Ok(())
}

unsafe fn reg_create_empty(path: &str) -> Result<(), String> {
    let path_h = HSTRING::from(path);
    let mut hkey = std::mem::zeroed();
    check(RegCreateKeyW(HKEY_LOCAL_MACHINE, &path_h, &mut hkey))?;
    let _ = RegCloseKey(hkey);
    Ok(())
}

unsafe fn reg_set_value(key_path: &str, name: &str, value: &str) -> Result<(), String> {
    let path_h = HSTRING::from(key_path);
    let val_h = HSTRING::from(value);
    let mut hkey = std::mem::zeroed();
    check(RegCreateKeyW(HKEY_LOCAL_MACHINE, &path_h, &mut hkey))?;
    let data: &[u8] = std::slice::from_raw_parts(val_h.as_ptr() as *const u8, val_h.len() * 2);
    if name.is_empty() {
        let _ = RegSetValueExW(hkey, None, 0, REG_SZ, Some(data));
    } else {
        let name_h = HSTRING::from(name);
        let _ = RegSetValueExW(hkey, &name_h, 0, REG_SZ, Some(data));
    }
    let _ = RegCloseKey(hkey);
    Ok(())
}

pub fn register_tip() -> Result<(), String> {
    let clsid_str = guid_to_string(&CLSID_PYRUST_TIP);
    let profile_str = guid_to_string(&PROFILE_GUID);
    let cat_kbd_str = guid_to_string(&CATEGORY_KEYBOARD);
    let cat_prof_str = guid_to_string(&CATEGORY_PROFILE);

    let dll_path = std::env::current_exe()
        .map_err(|e| format!("Cannot get DLL path: {e}"))?;
    let dll_path_str = dll_path.to_string_lossy().to_string();

    unsafe {
        let com_key = format!("SOFTWARE\\Classes\\CLSID\\{}", clsid_str);
        reg_create(&com_key, "pyrust Pinyin IME")?;

        let inproc = format!("{}\\InprocServer32", com_key);
        reg_create(&inproc, &dll_path_str)?;
        reg_set_value(&inproc, "ThreadingModel", "Apartment")?;
        reg_set_value(&inproc, "", &dll_path_str)?;

        let tip_key = format!("SOFTWARE\\Microsoft\\CTF\\TIP\\{}", clsid_str);
        reg_create(&tip_key, "pyrust Pinyin IME")?;

        let cat_kbd_key = format!("{}\\Category\\Category\\{}", tip_key, cat_kbd_str);
        reg_create_empty(&cat_kbd_key)?;

        let cat_prof_key = format!("{}\\Category\\Category\\{}", tip_key, cat_prof_str);
        reg_create_empty(&cat_prof_key)?;

        let prof_key = format!("{}\\Profiles\\{}", tip_key, profile_str);
        reg_create(&prof_key, "pyrust Pinyin")?;
    }

    log::info!("[tsf] Registry registration complete");
    Ok(())
}

pub fn unregister_tip() -> Result<(), String> {
    let clsid_str = guid_to_string(&CLSID_PYRUST_TIP);
    let tip_key = format!("SOFTWARE\\Microsoft\\CTF\\TIP\\{}", clsid_str);
    let com_key = format!("SOFTWARE\\Classes\\CLSID\\{}", clsid_str);
    log::info!("[tsf] Unregister. Remove keys manually if needed:\n  {}\n  {}", tip_key, com_key);
    Ok(())
}
