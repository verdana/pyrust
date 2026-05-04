use std::sync::Mutex;

use windows::core::{implement, Result as WinResult};
use windows::Win32::Foundation::RECT;
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

/// Edit session that retrieves the caret screen position via ITfContextView::GetTextExt.
#[implement(ITfEditSession)]
pub struct CaretPosEditSession {
    context: ITfContext,
    result: *mut Mutex<Option<(i32, i32)>>,
}

// SAFETY: The Mutex pointer is valid for the lifetime of the edit session (synchronous call).
unsafe impl Send for CaretPosEditSession {}
unsafe impl Sync for CaretPosEditSession {}

impl CaretPosEditSession {
    /// Create a new caret position edit session.
    ///
    /// # Safety
    /// `result` must point to a valid `Mutex` that outlives the synchronous edit session.
    pub unsafe fn new(context: ITfContext, result: *mut Mutex<Option<(i32, i32)>>) -> Self {
        Self { context, result }
    }
}

impl ITfEditSession_Impl for CaretPosEditSession_Impl {
    fn DoEditSession(&self, ec: u32) -> WinResult<()> {
        // Get the active view from the context
        let view = unsafe { self.context.GetActiveView() }?;

        // Get the current selection to find the insertion point
        let mut sel = [TF_SELECTION::default()];
        let mut fetched: u32 = 0;
        unsafe { self.context.GetSelection(ec, 0, &mut sel, &mut fetched) }?;

        if fetched == 0 {
            tlog!("[tsf] CaretPosEditSession: no selection");
            return Ok(());
        }

        let range = match &*sel[0].range {
            Some(r) => r,
            None => {
                tlog!("[tsf] CaretPosEditSession: no range in selection");
                return Ok(());
            }
        };

        // Collapse range to insertion point (start = cursor position)
        use windows::Win32::UI::TextServices::TfAnchor;
        let _ = unsafe { range.Collapse(ec, TfAnchor(0)) };

        // Get screen rectangle of the insertion point
        let mut rect = RECT::default();
        let mut clipped = windows::core::BOOL::default();
        // SAFETY: view is valid, ec is the edit session cookie, range is collapsed
        // to the insertion point, rect and clipped are valid output parameters.
        unsafe { view.GetTextExt(ec, range, &mut rect, &mut clipped) }.map_err(|e| {
            tlog!("[tsf] CaretPosEditSession: GetTextExt failed: {:?}", e);
            e
        })?;

        let x = rect.left;
        let y = rect.bottom;
        tlog!("[tsf] CaretPosEditSession: caret at ({}, {})", x, y);

        // Store the result
        // SAFETY: self.result is guaranteed valid by the caller (synchronous edit session).
        if let Ok(mut guard) = unsafe { &*self.result }.lock() {
            *guard = Some((x, y));
        }

        Ok(())
    }
}
