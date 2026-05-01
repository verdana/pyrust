use std::sync::Weak;
use windows::core::{implement, Error, HRESULT, Result as WinResult};
use windows::Win32::UI::TextServices::{ITfEditSession, ITfEditSession_Impl};
use crate::tip::PyrustTip;

#[implement(ITfEditSession)]
pub struct CommitEditSession {
    tip: Weak<PyrustTip>,
}

impl CommitEditSession {
    pub fn new(tip: Weak<PyrustTip>) -> Self { Self { tip } }
}

impl ITfEditSession_Impl for CommitEditSession_Impl {
    fn DoEditSession(&self, _ec: u32) -> WinResult<()> {
        let _tip = self.tip.upgrade().ok_or_else(|| {
            Error::new(HRESULT(0x80004005u32 as i32), "TIP already dropped")
        })?;
        Ok(())
    }
}
