use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

use windows::core::{implement, Interface, Result as WinResult};
use windows::Win32::Foundation::RECT;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, GUID_PROP_ATTRIBUTE, GUID_PROP_COMPOSING, ITfCategoryMgr, ITfComposition,
    ITfCompositionSink, ITfContext, ITfContextComposition, ITfEditSession, ITfEditSession_Impl,
    ITfRange, TF_SELECTION,
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
    sink: Option<ITfCompositionSink>,
}

impl CompositionEditSession {
    /// Apply display attribute (underline) to the preedit range.
    /// Sets both GUID_PROP_COMPOSING (BOOL) and GUID_PROP_ATTRIBUTE (display style).
    fn apply_display_attribute(ec: u32, context: &ITfContext, range: &ITfRange) {
        // SAFETY: All COM calls are valid within an edit session.
        unsafe {
            // 1. Set GUID_PROP_COMPOSING = TRUE (signals that text is in composition mode)
            // Many apps (like Notepad) rely on this property to render composition styling.
            if let Ok(prop) = context.GetProperty(&GUID_PROP_COMPOSING) {
                // Use raw pointer to avoid ManuallyDrop issues
                let mut var: VARIANT = std::mem::zeroed();
                let var_ptr = &mut var as *mut VARIANT as *mut u8;
                // vt is at offset 0
                *(var_ptr as *mut windows::Win32::System::Variant::VARENUM) = windows::Win32::System::Variant::VT_BOOL;
                // boolVal is in the Anonymous union, at offset 8
                let bool_ptr = var_ptr.add(8) as *mut windows::Win32::Foundation::VARIANT_BOOL;
                *bool_ptr = windows::Win32::Foundation::VARIANT_BOOL(-1);

                if let Err(e) = prop.SetValue(ec, range, &var) {
                    tlog!("[tsf] apply_display_attribute: GUID_PROP_COMPOSING SetValue failed: {:?}", e);
                } else {
                    tlog!("[tsf] apply_display_attribute: GUID_PROP_COMPOSING = TRUE");
                }
            }

            // 2. Get the display attribute property and set our custom attribute.
            let prop = match context.GetProperty(&GUID_PROP_ATTRIBUTE) {
                Ok(p) => p,
                Err(e) => {
                    tlog!("[tsf] apply_display_attribute: GetProperty failed: {:?}", e);
                    return;
                }
            };

            // 3. Get TfGuidAtom for our display attribute GUID.
            let cat_mgr: ITfCategoryMgr = match CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER) {
                Ok(m) => m,
                Err(e) => {
                    tlog!("[tsf] apply_display_attribute: CoCreateInstance(CategoryMgr) failed: {:?}", e);
                    return;
                }
            };

            let atom = match cat_mgr.RegisterGUID(&crate::display_attrs::GUID_ATTR_INPUT) {
                Ok(a) => a,
                Err(e) => {
                    tlog!("[tsf] apply_display_attribute: RegisterGUID failed: {:?}", e);
                    return;
                }
            };

            // 4. Set the property value (TfGuidAtom is VT_I4).
            let mut var: VARIANT = std::mem::zeroed();
            let var_ptr = &mut var as *mut VARIANT as *mut u8;
            *(var_ptr as *mut windows::Win32::System::Variant::VARENUM) = windows::Win32::System::Variant::VT_I4;
            let lval_ptr = var_ptr.add(8) as *mut i32;
            *lval_ptr = atom as i32;

            if let Err(e) = prop.SetValue(ec, range, &var) {
                tlog!("[tsf] apply_display_attribute: GUID_PROP_ATTRIBUTE SetValue failed: {:?}", e);
            } else {
                tlog!("[tsf] apply_display_attribute: OK atom={}", atom);
            }
        }
    }

    pub fn update(
        context: ITfContext,
        text: String,
        composition: Rc<RefCell<Option<ITfComposition>>>,
        preedit_range: Rc<RefCell<Option<ITfRange>>>,
        sink: Option<ITfCompositionSink>,
    ) -> Self {
        Self {
            context,
            text,
            end_composition: false,
            composition,
            preedit_range,
            sink,
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
            sink: None,
        }
    }
}

impl ITfEditSession_Impl for CompositionEditSession_Impl {
    fn DoEditSession(&self, ec: u32) -> WinResult<()> {
        let text_wide: Vec<u16> = self.text.encode_utf16().collect();

        // 1. Determine the range to operate on.
        // Priority: stored preedit_range > composition.GetRange() > new from selection
        let mut current_range = if let Some(ref r) = *self.preedit_range.borrow() {
            Some(r.clone())
        } else if let Some(ref comp) = *self.composition.borrow() {
            match unsafe { comp.GetRange() } {
                Ok(range) => Some(range),
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

            if fetched > 0 && sel[0].range.is_some() {
                let range = sel[0].range.as_ref().unwrap();
                use windows::Win32::UI::TextServices::TfAnchor;

                // Insert text at cursor position
                let _ = unsafe { range.Collapse(ec, TfAnchor(0)) };
                if let Err(e) = unsafe { range.SetText(ec, 0, &text_wide) } {
                    tlog!("[tsf] CompositionEditSession: SetText FAILED: {:?}", e);
                    return Err(e);
                }
                let _ = unsafe { range.ShiftStart(ec, -(text_wide.len() as i32), std::ptr::null_mut(), std::ptr::null()) };

                // Now try StartComposition with the text range
                let mut composition_created = false;
                if let Ok(ctx_comp) = self.context.cast::<ITfContextComposition>() {
                    match unsafe { ctx_comp.StartComposition(ec, range, None) } {
                        Ok(comp) => {
                            tlog!("[tsf] CompositionEditSession: StartComposition SUCCESS");
                            *self.composition.borrow_mut() = Some(comp.clone());
                            composition_created = true;
                        }
                        Err(e) => {
                            tlog!("[tsf] CompositionEditSession: StartComposition FAILED: {:?}", e);
                            // Try with a cloned range as fallback
                            if let Ok(cloned) = unsafe { range.Clone() } {
                                match unsafe { ctx_comp.StartComposition(ec, &cloned, None) } {
                                    Ok(comp) => {
                                        tlog!("[tsf] CompositionEditSession: StartComposition SUCCESS (cloned)");
                                        *self.composition.borrow_mut() = Some(comp.clone());
                                        composition_created = true;
                                    }
                                    Err(e2) => {
                                        tlog!("[tsf] CompositionEditSession: StartComposition FAILED (cloned): {:?}", e2);
                                    }
                                }
                            }
                        }
                    }
                }

                current_range = Some(range.clone());
                *self.preedit_range.borrow_mut() = Some(range.clone());

                if !composition_created && !text_wide.is_empty() {
                    CompositionEditSession::apply_display_attribute(ec, &self.context, range);
                }
            } else {
                // Fallback: try GetStart.
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
            if let Err(e) = unsafe { range.SetText(ec, 0, &text_wide) } {
                tlog!("[tsf] CompositionEditSession: SetText FAILED: {:?}", e);
                return Err(e);
            }

            // SetText collapses the range to end (0 length). Re-expand to cover the text.
            if !text_wide.is_empty() {
                let _ = unsafe { range.ShiftStart(ec, -(text_wide.len() as i32), std::ptr::null_mut(), std::ptr::null()) };
            }

            // Store/update the range for next keystroke (covers new text span).
            if !self.end_composition {
                *self.preedit_range.borrow_mut() = Some(range.clone());
            }

            // 4. Apply display attribute (underline) to the range.
            // If composition exists (StartComposition succeeded), TSF manages attributes automatically.
            if !self.end_composition && !text_wide.is_empty() && self.composition.borrow().is_none() {
                CompositionEditSession::apply_display_attribute(ec, &self.context, range);
            }

            // 5. Move cursor to end.
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
        }

        // 5. Handle completion — clear all state.
        if self.end_composition {
            *self.composition.borrow_mut() = None;
            *self.preedit_range.borrow_mut() = None;
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
