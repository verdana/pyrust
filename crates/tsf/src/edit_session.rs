use windows::core::{implement, Result as WinResult};
use windows::Win32::UI::TextServices::{
    ITfContext, ITfEditSession, ITfEditSession_Impl, TF_SELECTION,
};

#[allow(unused_imports)]
use crate::tlog;

/// Edit session that inserts committed text into the application.
#[implement(ITfEditSession)]
pub struct CommitEditSession {
    context: ITfContext,
    text: String,
}

impl CommitEditSession {
    pub fn new(context: ITfContext, text: String) -> Self {
        Self { context, text }
    }
}

impl ITfEditSession_Impl for CommitEditSession_Impl {
    fn DoEditSession(&self, ec: u32) -> WinResult<()> {
        let text_wide: Vec<u16> = self.text.encode_utf16().collect();
        tlog!(
            "[tsf] DoEditSession: inserting '{}' ({} chars)",
            self.text,
            text_wide.len()
        );

        // Get current selection
        let mut sel = [TF_SELECTION::default()];
        let mut fetched: u32 = 0;
        // SAFETY: context is a valid ITfContext. ec is the edit cookie from TSF.
        // sel and fetched are valid output parameters.
        unsafe { self.context.GetSelection(ec, 0, &mut sel, &mut fetched) }.map_err(|e| {
            tlog!("[tsf] DoEditSession: GetSelection failed: {:?}", e);
            e
        })?;

        // Get the range from selection and collapse to start (insertion point)
        let range_opt: &Option<_> = &sel[0].range;
        if let Some(ref range) = range_opt {
            // Collapse to start: start=end=cursor position
            use windows::Win32::UI::TextServices::TfAnchor;
            // SAFETY: range is a valid ITfRange from GetSelection. ec is the edit cookie.
            let _ = unsafe { range.Collapse(ec, TfAnchor(0)) }; // TF_ANCHOR_START

            // SAFETY: range is collapsed to insertion point. text_wide is valid UTF-16.
            unsafe { range.SetText(ec, 0, &text_wide) }.map_err(|e| {
                tlog!("[tsf] DoEditSession: SetText failed: {:?}", e);
                e
            })?;

            // Move cursor to the end of inserted text
            // SAFETY: range is valid; TfAnchor(1) is TF_ANCHOR_END.
            let _ = unsafe { range.Collapse(ec, TfAnchor(1)) }; // TF_ANCHOR_END

            // SAFETY: new_sel contains a cloned range and valid fields from sel[0].
            let new_sel = TF_SELECTION {
                range: std::mem::ManuallyDrop::new(Some(range.clone())),
                ..sel[0]
            };
            // SAFETY: context is valid, ec is the edit cookie, new_sel is a valid selection.
            let _ = unsafe { self.context.SetSelection(ec, &[new_sel]) };

            tlog!("[tsf] DoEditSession: text inserted OK, cursor moved to end");
        } else {
            tlog!("[tsf] DoEditSession: no range in selection");
        }

        Ok(())
    }
}
