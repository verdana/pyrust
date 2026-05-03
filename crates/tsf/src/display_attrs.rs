use windows::core::{implement, Error, HRESULT, Result as WinResult};
use windows::Win32::UI::TextServices::{
    IEnumTfDisplayAttributeInfo, IEnumTfDisplayAttributeInfo_Impl,
    ITfDisplayAttributeInfo,
};

/// Empty enumerator — returns no display attributes.
/// This tells TSF we don't provide custom display attributes
/// (TSF will use its defaults) without returning E_NOTIMPL.
#[implement(IEnumTfDisplayAttributeInfo)]
pub struct EmptyEnumDisplayAttr;

impl IEnumTfDisplayAttributeInfo_Impl for EmptyEnumDisplayAttr_Impl {
    fn Clone(&self) -> WinResult<IEnumTfDisplayAttributeInfo> {
        Err(Error::new(HRESULT(0x80004001u32 as i32), "Not implemented"))
    }
    fn Next(
        &self,
        _ulcount: u32,
        _rginfo: *mut Option<ITfDisplayAttributeInfo>,
        pcfetched: *mut u32,
    ) -> WinResult<()> {
        unsafe { *pcfetched = 0 };
        Ok(()) // S_OK with 0 items = end of enumeration
    }
    fn Reset(&self) -> WinResult<()> {
        Ok(())
    }
    fn Skip(&self, _ulcount: u32) -> WinResult<()> {
        Ok(())
    }
}
