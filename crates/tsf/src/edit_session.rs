use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::Mutex;

use windows::core::{implement, Interface, Result as WinResult};
use windows::Win32::Foundation::RECT;
use windows::Win32::UI::TextServices::{
    ITfComposition, ITfCompositionSink, ITfContext, ITfContextComposition, ITfEditSession,
    ITfEditSession_Impl, ITfRange, TF_SELECTION,
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

/// Edit session that manages TSF composition (underlined preedit text).
///
/// When `end_composition` is false: starts a new composition or updates the
/// existing one with new preedit text (e.g., "ni" → "nihao").
/// When `end_composition` is true: replaces composition text with final
/// committed text and ends the composition.
#[implement(ITfEditSession)]
pub struct CompositionEditSession {
    context: ITfContext,
    text: String,
    end_composition: bool,
    composition: Rc<RefCell<Option<ITfComposition>>>,
    /// Tracked preedit text range for replacement without requiring StartComposition.
    preedit_range: Rc<RefCell<Option<ITfRange>>>,
    /// Raw pointer to ITfCompositionSink — reconstructed as InterfaceRef in DoEditSession.
    /// Valid for the lifetime of the synchronous edit session (TF_ES_SYNC).
    sink_ptr: Option<*const c_void>,
}

impl CompositionEditSession {
    pub fn update(
        context: ITfContext,
        text: String,
        composition: Rc<RefCell<Option<ITfComposition>>>,
        preedit_range: Rc<RefCell<Option<ITfRange>>>,
        sink_ptr: Option<*const c_void>,
    ) -> Self {
        Self {
            context,
            text,
            end_composition: false,
            composition,
            preedit_range,
            sink_ptr,
        }
    }

    pub fn commit(
        context: ITfContext,
        text: String,
        composition: Rc<RefCell<Option<ITfComposition>>>,
        preedit_range: Rc<RefCell<Option<ITfRange>>>,
    ) -> Self {
        Self {
            context,
            text,
            end_composition: true,
            composition,
            preedit_range,
            sink_ptr: None,
        }
    }
}

impl ITfEditSession_Impl for CompositionEditSession_Impl {
    fn DoEditSession(&self, ec: u32) -> WinResult<()> {
        tlog!("[tsf] CompositionEditSession: BEGIN text='{}' end_comp={} has_range={}", self.text, self.end_composition, self.preedit_range.borrow().is_some());
        let text_wide: Vec<u16> = self.text.encode_utf16().collect();

        // 1. Determine the range to operate on.
        // Priority: stored preedit_range > composition.GetRange() > new from selection
        let mut current_range = if let Some(ref r) = *self.preedit_range.borrow() {
            tlog!("[tsf] CompositionEditSession: using stored preedit range");
            Some(r.clone())
        } else if let Some(ref comp) = *self.composition.borrow() {
            match unsafe { comp.GetRange() } {
                Ok(range) => {
                    tlog!("[tsf] CompositionEditSession: using composition range");
                    Some(range)
                }
                Err(e) => {
                    tlog!("[tsf] CompositionEditSession: comp.GetRange failed: {:?}", e);
                    None
                }
            }
        } else {
            None
        };

        // 2. If no range yet and updating, create one from the selection.
        if current_range.is_none() && !self.end_composition {
            let mut sel = [TF_SELECTION::default()];
            let mut fetched: u32 = 0;
            if let Err(e) = unsafe { self.context.GetSelection(ec, 0, &mut sel, &mut fetched) } {
                tlog!("[tsf] CompositionEditSession: GetSelection FAILED: {:?}", e);
                return Err(e);
            }
            tlog!("[tsf] CompositionEditSession: GetSelection fetched={} has_range={}", fetched, sel[0].range.is_some());

            if fetched > 0 && sel[0].range.is_some() {
                let range = sel[0].range.as_ref().unwrap();
                use windows::Win32::UI::TextServices::TfAnchor;
                let _ = unsafe { range.Collapse(ec, TfAnchor(0)) };

                // Insert text, then expand range backward to cover it.
                tlog!("[tsf] CompositionEditSession: initial SetText '{}'", self.text);
                if let Err(e) = unsafe { range.SetText(ec, 0, &text_wide) } {
                    tlog!("[tsf] CompositionEditSession: SetText FAILED: {:?}", e);
                    return Err(e);
                }
                let _ = unsafe { range.ShiftStart(ec, -(text_wide.len() as i32), std::ptr::null_mut(), std::ptr::null()) };

                // StartComposition is attempted here; failure is logged but not fatal
                // since text is already visible via SetText + preedit_range tracking.
                if let Ok(ctx_comp) = self.context.cast::<ITfContextComposition>() {
                    // Reconstruct &ITfCompositionSink from raw pointer.
                    // SAFETY: sink_ptr is a valid ITfCompositionSink COM pointer from PyrustTip,
                    // valid for the lifetime of this synchronous edit session (TF_ES_SYNC).
                    // ITfCompositionSink is #[repr(transparent)] over NonNull<c_void>,
                    // same layout as *const c_void.
                    let sink_ref: Option<&ITfCompositionSink> =
                        self.sink_ptr.map(|p| unsafe { &*(p as *const ITfCompositionSink) });
                    match unsafe { ctx_comp.StartComposition(ec, range, sink_ref) } {
                        Ok(comp) => {
                            tlog!("[tsf] CompositionEditSession: StartComposition SUCCESS");
                            *self.composition.borrow_mut() = Some(comp.clone());
                        }
                        Err(e) => {
                            tlog!("[tsf] CompositionEditSession: StartComposition FAILED: {:?}", e);
                        }
                    }
                }

                current_range = Some(range.clone());
                // Store range for next keystroke so we can REPLACE instead of re-insert.
                *self.preedit_range.borrow_mut() = Some(range.clone());
            } else {
                // Fallback: try GetStart.
                tlog!("[tsf] CompositionEditSession: no selection, fallback GetStart");
                if let Ok(start_range) = unsafe { self.context.GetStart(ec) } {
                    if let Err(e) = unsafe { start_range.SetText(ec, 0, &text_wide) } {
                        tlog!("[tsf] CompositionEditSession: fallback SetText FAILED: {:?}", e);
                        return Err(e);
                    }
                    let _ = unsafe { start_range.ShiftStart(ec, -(text_wide.len() as i32), std::ptr::null_mut(), std::ptr::null()) };
                    current_range = Some(start_range.clone());
                    *self.preedit_range.borrow_mut() = Some(start_range);
                }
            }
        }

        // 3. Replace/update text on the range.
        if let Some(ref range) = current_range {
            tlog!("[tsf] CompositionEditSession: SetText '{}' on range", self.text);
            if let Err(e) = unsafe { range.SetText(ec, 0, &text_wide) } {
                tlog!("[tsf] CompositionEditSession: SetText FAILED: {:?}", e);
                return Err(e);
            }

            // Store/update the range for next keystroke (covers new text span).
            if !self.end_composition {
                *self.preedit_range.borrow_mut() = Some(range.clone());
            }

            // 4. Move cursor to end.
            if let Ok(cursor_range) = unsafe { range.Clone() } {
                use windows::Win32::UI::TextServices::TfAnchor;
                let _ = unsafe { cursor_range.Collapse(ec, TfAnchor(1)) };
                let new_sel = TF_SELECTION {
                    range: std::mem::ManuallyDrop::new(Some(cursor_range)),
                    style: windows::Win32::UI::TextServices::TF_SELECTIONSTYLE {
                        ase: windows::Win32::UI::TextServices::TF_AE_END,
                        fInterimChar: false.into(),
                    },
                };
                if let Err(e) = unsafe { self.context.SetSelection(ec, &[new_sel]) } {
                    tlog!("[tsf] CompositionEditSession: SetSelection FAILED: {:?}", e);
                }
            }
        } else {
            tlog!("[tsf] CompositionEditSession: SKIPPING — no range, text NOT inserted");
        }

        // 5. Handle completion — clear all state.
        if self.end_composition {
            *self.composition.borrow_mut() = None;
            *self.preedit_range.borrow_mut() = None;
        }

        tlog!("[tsf] CompositionEditSession: END");
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
