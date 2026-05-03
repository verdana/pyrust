use std::io::Write;
use windows::core::{implement, Result as WinResult};
use windows::Win32::UI::TextServices::{
    ITfContext, ITfEditSession, ITfEditSession_Impl,
    TF_SELECTION,
};

fn ses_log(msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true).append(true)
        .open("C:\\Users\\Verdana\\pyrust_tsf.log")
    {
        let _ = writeln!(f, "{msg}");
        let _ = f.flush();
    }
}

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
        ses_log(&format!("[tsf] DoEditSession: inserting '{}' ({} chars)", self.text, text_wide.len()));

        // Get current selection
        let mut sel = [TF_SELECTION::default()];
        let mut fetched: u32 = 0;
        unsafe {
            self.context.GetSelection(ec, 0, &mut sel, &mut fetched)
        }.map_err(|e| {
            ses_log(&format!("[tsf] DoEditSession: GetSelection failed: {:?}", e));
            e
        })?;

        // Get the range from selection and collapse to start (insertion point)
        let range_opt: &Option<_> = &sel[0].range;
        if let Some(ref range) = range_opt {
            // Collapse to start: start=end=cursor position
            use windows::Win32::UI::TextServices::TfAnchor;
            let _ = unsafe { range.Collapse(ec, TfAnchor(0)) }; // TF_ANCHOR_START

            // SetText on the collapsed range inserts text at cursor
            unsafe {
                range.SetText(ec, 0, &text_wide)
            }.map_err(|e| {
                ses_log(&format!("[tsf] DoEditSession: SetText failed: {:?}", e));
                e
            })?;

            // Move cursor to the end of inserted text
            let _ = unsafe { range.Collapse(ec, TfAnchor(1)) }; // TF_ANCHOR_END

            // Update the selection to place cursor at end
            let new_sel = TF_SELECTION {
                range: std::mem::ManuallyDrop::new(Some(range.clone())),
                ..sel[0]
            };
            let _ = unsafe {
                self.context.SetSelection(ec, &[new_sel])
            };

            ses_log(&format!("[tsf] DoEditSession: text inserted OK, cursor moved to end"));
        } else {
            ses_log("[tsf] DoEditSession: no range in selection");
        }

        Ok(())
    }
}
