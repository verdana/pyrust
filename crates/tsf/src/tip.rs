use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use windows::core::{
    implement, ComObjectInterface, Error, Interface, Result as WinResult, GUID, HRESULT,
};
use windows::Win32::System::Com::{CoFreeUnusedLibraries, IClassFactory, IClassFactory_Impl};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::Win32::UI::TextServices::{
    ITfCompartmentMgr, ITfComposition, ITfCompositionSink, ITfCompositionSink_Impl, ITfContext,
    ITfContextKeyEventSink, ITfContextKeyEventSink_Impl, ITfDisplayAttributeProvider,
    ITfDisplayAttributeProvider_Impl, ITfDocumentMgr, ITfEditSession, ITfKeyEventSink,
    ITfKeyEventSink_Impl, ITfKeystrokeMgr, ITfSource, ITfTextInputProcessor,
    ITfTextInputProcessorEx, ITfTextInputProcessorEx_Impl, ITfTextInputProcessor_Impl,
    ITfThreadFocusSink, ITfThreadFocusSink_Impl, ITfThreadMgr, ITfThreadMgrEventSink,
    ITfThreadMgrEventSink_Impl, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE,
};

use crate::bridge::TsfBridge;
use crate::edit_session::CaretPosEditSession;
#[allow(unused_imports)]
use crate::tlog;

pub(crate) static DLL_REF_COUNT: AtomicI32 = AtomicI32::new(0);

/// Result of handle_shift_key — tells the caller what to do.
enum ShiftResult {
    /// Not a Shift key event; caller should continue normal processing.
    NotShift,
    /// Shift event fully handled; caller should consume and return.
    Consumed,
    /// Shift released with pending pinyin; caller should commit raw
    /// pinyin (via handle_keypress VK_ENTER) then toggle mode.
    CommitThenToggle,
}

#[implement(IClassFactory)]
pub struct PyrustClassFactory;

impl IClassFactory_Impl for PyrustClassFactory_Impl {
    fn CreateInstance(
        &self,
        punkouter: windows::core::Ref<'_, windows::core::IUnknown>,
        riid: *const GUID,
        ppv: *mut *mut std::ffi::c_void,
    ) -> WinResult<()> {
        if !punkouter.is_none() {
            return Err(Error::new(
                HRESULT(0x80040110u32 as i32),
                "Aggregation not supported",
            ));
        }
        let tip: ITfTextInputProcessorEx = PyrustTip::new().into();
        // SAFETY: COM QueryInterface — riid and ppv are valid per IClassFactory contract.
        unsafe { tip.query(riid, ppv) }.ok()?;
        Ok(())
    }

    fn LockServer(&self, lock: windows::core::BOOL) -> WinResult<()> {
        if lock.as_bool() {
            DLL_REF_COUNT.fetch_add(1, Ordering::Release);
        } else {
            DLL_REF_COUNT.fetch_sub(1, Ordering::Release);
            // SAFETY: CoFreeUnusedLibraries is a standard COM cleanup call.
            unsafe { CoFreeUnusedLibraries() };
        }
        Ok(())
    }
}

#[implement(
    ITfTextInputProcessor,
    ITfTextInputProcessorEx,
    ITfKeyEventSink,
    ITfContextKeyEventSink,
    ITfCompositionSink,
    ITfDisplayAttributeProvider,
    ITfThreadMgrEventSink,
    ITfThreadFocusSink
)]
pub struct PyrustTip {
    thread_mgr: RefCell<Option<ITfThreadMgr>>,
    client_id: RefCell<u32>,
    bridge: RefCell<Option<TsfBridge>>,
    is_active: AtomicBool,
    #[allow(dead_code)]
    is_password: AtomicBool,
    current_doc_mgr: RefCell<Option<ITfDocumentMgr>>,
    composition: RefCell<Option<ITfComposition>>,
    sink_cookie: RefCell<Option<u32>>,
    context_key_cookies: RefCell<Vec<u32>>,
    focus_sink_cookie: RefCell<Option<u32>>,
    shift_pending: Cell<bool>,
}

impl PyrustTip {
    pub fn new() -> Self {
        DLL_REF_COUNT.fetch_add(1, Ordering::Release);
        Self {
            thread_mgr: RefCell::new(None),
            client_id: RefCell::new(0),
            bridge: RefCell::new(None),
            is_active: AtomicBool::new(false),
            is_password: AtomicBool::new(false),
            current_doc_mgr: RefCell::new(None),
            composition: RefCell::new(None),
            sink_cookie: RefCell::new(None),
            context_key_cookies: RefCell::new(Vec::new()),
            focus_sink_cookie: RefCell::new(None),
            shift_pending: Cell::new(false),
        }
    }

    /// Get the current ITfContext from the active document manager.
    pub fn get_context(&self) -> Option<ITfContext> {
        self.current_doc_mgr.borrow().as_ref().and_then(|dm| {
            // SAFETY: dm is a valid ITfDocumentMgr COM reference.
            unsafe { dm.GetTop() }.ok()
        })
    }

    fn is_shift_key(vk: u32) -> bool {
        matches!(vk, 0x10 | 0xA0 | 0xA1) // VK_SHIFT, VK_LSHIFT, VK_RSHIFT
    }

    /// Handle Shift key logic for mode toggle.
    fn handle_shift_key(&self, vk: u32, is_down: bool) -> ShiftResult {
        if !Self::is_shift_key(vk) {
            if is_down {
                self.shift_pending.set(false);
            }
            return ShiftResult::NotShift;
        }
        if is_down {
            self.shift_pending.set(true);
            ShiftResult::Consumed
        } else {
            if self.shift_pending.get() {
                self.shift_pending.set(false);
                // Check if there's pinyin to commit before toggling.
                let has_pinyin = self.bridge.borrow().as_ref().map_or(false, |b| b.has_pinyin());
                if has_pinyin {
                    ShiftResult::CommitThenToggle
                } else {
                    if let Some(ref bridge) = *self.bridge.borrow() {
                        let _ = bridge.req_tx().send(crate::Request::ToggleMode);
                        tlog!("[tsf] Shift toggle: mode switched (no pinyin to commit)");
                    }
                    ShiftResult::Consumed
                }
            } else {
                ShiftResult::Consumed
            }
        }
    }

    fn get_modifiers() -> crate::Modifiers {
        // SAFETY: GetKeyState reads the per-thread key state table; returns i16,
        // high bit (0x80) means key is currently pressed.
        unsafe {
            crate::Modifiers {
                shift: GetKeyState(VK_SHIFT.0 as i32) < 0,
                ctrl: GetKeyState(VK_CONTROL.0 as i32) < 0,
                alt: GetKeyState(VK_MENU.0 as i32) < 0,
            }
        }
    }

    fn should_consume_key(&self, vk: u32) -> bool {
        if !self.is_active.load(Ordering::Acquire) {
            return false;
        }
        // Shift 键始终消费（用于中英切换）
        if Self::is_shift_key(vk) {
            return true;
        }
        if let Some(ref bridge) = *self.bridge.borrow() {
            if !bridge.is_zh_mode() {
                return false;
            }
        }
        matches!(vk,
            0x41..=0x5A | 0x30..=0x39 | 0x20 | 0x08 | 0x0D | 0x1B | 0x25..=0x28
            | 0xBC | 0xBE | 0xBA | 0xBF | 0xBB | 0xBD | 0xDC | 0xDE | 0xDB | 0xDD
        )
    }

    fn handle_keypress(&self, context: &ITfContext, vk: u32) -> WinResult<windows::core::BOOL> {
        let binding = self.bridge.borrow();
        let bridge = match *binding {
            Some(ref b) => b,
            None => {
                tlog!("[tsf] handle_keypress: no bridge, passing through");
                return Ok(false.into());
            }
        };
        let modifiers = Self::get_modifiers();
        let caret_pos = self.get_caret_pos(context);
        let (resp_tx, resp_rx) = crate::oneshot::channel();
        let _ = bridge.req_tx().send(crate::Request::KeyPress {
            vk,
            modifiers,
            caret_pos,
            response: resp_tx,
        });
        match resp_rx.recv() {
            Some(crate::Response::Committed(text)) => {
                tlog!("[tsf] handle_keypress: Committed '{}'", text);
                use windows::Win32::UI::TextServices::TF_ES_READWRITE;
                let edit_session: ITfEditSession =
                    crate::edit_session::CommitEditSession::new(context.clone(), text).into();
                // SAFETY: context is a valid ITfContext. edit_session implements ITfEditSession.
                let _ = unsafe {
                    context.RequestEditSession(
                        *self.client_id.borrow(),
                        &edit_session,
                        TF_ES_READWRITE,
                    )
                };
                Ok(true.into())
            }
            Some(crate::Response::Consumed) => Ok(true.into()),
            Some(crate::Response::Passthrough) | None => Ok(false.into()),
        }
    }

    /// Get the caret screen position via a synchronous TSF edit session.
    /// Returns (x, y) in screen coordinates, or None if unavailable.
    fn get_caret_pos(&self, context: &ITfContext) -> Option<(i32, i32)> {
        use std::sync::Mutex;
        use windows::Win32::UI::TextServices::{TF_ES_READ, TF_ES_SYNC};

        let result: *mut Mutex<Option<(i32, i32)>> =
            Box::into_raw(Box::new(Mutex::new(None)));

        // SAFETY: result points to a heap-allocated Mutex that outlives the
        // synchronous edit session (TF_ES_SYNC ensures DoEditSession completes
        // before RequestEditSession returns).
        let session: ITfEditSession =
            unsafe { CaretPosEditSession::new(context.clone(), result) }.into();

        // TF_ES_READ | TF_ES_SYNC: read-only and synchronous — blocks until
        // DoEditSession runs, so we can safely read the result afterward.
        match unsafe {
            context.RequestEditSession(
                *self.client_id.borrow(),
                &session,
                TF_ES_READ | TF_ES_SYNC,
            )
        } {
            Ok(_) => {}
            Err(e) => {
                tlog!("[tsf] get_caret_pos: RequestEditSession failed: {:?}", e);
                // SAFETY: session won't run, free the result allocation.
                let _ = unsafe { Box::from_raw(result) };
                return None;
            }
        }

        // SAFETY: The synchronous edit session has completed (TF_ES_SYNC),
        // so we can safely read and free the result.
        let pos = unsafe { Box::from_raw(result) }
            .into_inner()
            .ok()
            .flatten();

        tlog!("[tsf] get_caret_pos: result={:?}", pos);
        pos
    }
}

impl Drop for PyrustTip {
    fn drop(&mut self) {
        if let Some(mut b) = self.bridge.borrow_mut().take() {
            b.shutdown();
        }
        DLL_REF_COUNT.fetch_sub(1, Ordering::Release);
        // SAFETY: CoFreeUnusedLibraries is a standard COM cleanup call.
        unsafe { CoFreeUnusedLibraries() };
    }
}

impl ITfTextInputProcessor_Impl for PyrustTip_Impl {
    fn Activate(&self, ptim: windows::core::Ref<'_, ITfThreadMgr>, tid: u32) -> WinResult<()> {
        tlog!("[tsf] Activate BEGIN tid={}", tid);

        // Step 1: thread_mgr — THIS IS THE ONLY FATAL STEP
        let tm = if let Some(tm) = ptim.as_ref() {
            tlog!("[tsf] Activate step1: got ITfThreadMgr");
            tm.clone()
        } else {
            tlog!("[tsf] Activate FATAL: no ITfThreadMgr provided");
            return Err(Error::new(HRESULT(0x80004003u32 as i32), "No thread mgr"));
        };
        *self.thread_mgr.borrow_mut() = Some(tm.clone());
        *self.client_id.borrow_mut() = tid;

        // Step 2: Register ITfThreadMgrEventSink (NON-FATAL)
        match tm.cast::<ITfSource>() {
            Ok(source) => {
                tlog!("[tsf] Activate step2a: ITfSource cast OK");
                let sink_ref: windows::core::InterfaceRef<'_, ITfThreadMgrEventSink> =
                    self.as_interface_ref();
                // SAFETY: sink_ref points to a valid COM interface (self implements ITfThreadMgrEventSink).
                // The IID is the correct identifier for ITfThreadMgrEventSink.
                match unsafe {
                    source.AdviseSink(&<ITfThreadMgrEventSink as Interface>::IID, &*sink_ref)
                } {
                    Ok(cookie) => {
                        *self.sink_cookie.borrow_mut() = Some(cookie);
                        tlog!("[tsf] Activate step2b: AdviseSink OK cookie={}", cookie);
                    }
                    Err(e) => tlog!("[tsf] Activate step2b WARN: AdviseSink failed: {:?}", e),
                }
            }
            Err(e) => tlog!("[tsf] Activate step2a WARN: ITfSource cast failed: {:?}", e),
        }

        // Step 3: Register ITfKeyEventSink (NON-FATAL)
        match tm.cast::<ITfKeystrokeMgr>() {
            Ok(keystroke_mgr) => {
                tlog!("[tsf] Activate step3a: ITfKeystrokeMgr cast OK");
                let key_sink_ref: windows::core::InterfaceRef<'_, ITfKeyEventSink> =
                    self.as_interface_ref();
                // SAFETY: key_sink_ref points to a valid COM interface (self implements ITfKeyEventSink).
                // tid is the client ID provided by TSF in Activate.
                match unsafe { keystroke_mgr.AdviseKeyEventSink(tid, &*key_sink_ref, true) } {
                    Ok(()) => tlog!("[tsf] Activate step3b: AdviseKeyEventSink OK"),
                    Err(e) => tlog!(
                        "[tsf] Activate step3b WARN: AdviseKeyEventSink failed: {:?}",
                        e
                    ),
                }
            }
            Err(e) => tlog!(
                "[tsf] Activate step3a WARN: ITfKeystrokeMgr cast failed: {:?}",
                e
            ),
        }

        // Step 4: Initialize engine bridge (worker + forwarder + config watcher).
        // UI thread is NOT started here — creating an egui window from inside
        // the TSF COM callback causes COM re-entrancy deadlock → Explorer crash.
        match TsfBridge::initialize_engine() {
            Ok(bridge) => {
                *self.bridge.borrow_mut() = Some(bridge);
                tlog!("[tsf] Activate step4: Engine bridge OK (UI deferred)");
            }
            Err(e) => tlog!(
                "[tsf] Activate step4 WARN: Engine bridge init failed: {:?}",
                e
            ),
        }

        // Step 5: Set keyboard open state via compartment.
        // Windows 11 TextInputHost may skip keystroke routing if the
        // keyboard compartment is not explicitly set to "open".
        match tm.cast::<ITfCompartmentMgr>() {
            Ok(comp_mgr) => {
                tlog!("[tsf] Activate step5a: ITfCompartmentMgr cast OK");
                // SAFETY: comp_mgr is a valid ITfCompartmentMgr obtained from ITfThreadMgr.
                match unsafe { comp_mgr.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE) } {
                    Ok(comp) => {
                        let val = VARIANT::from(1i32);
                        // SAFETY: comp is a valid ITfCompartment. tid is the client ID. val is a valid VARIANT.
                        match unsafe { comp.SetValue(tid, &val) } {
                            Ok(()) => {
                                tlog!("[tsf] Activate step5b: Keyboard compartment set to OPEN")
                            }
                            Err(e) => tlog!("[tsf] Activate step5b WARN: SetValue failed: {:?}", e),
                        }
                    }
                    Err(e) => tlog!("[tsf] Activate step5a WARN: GetCompartment failed: {:?}", e),
                }
            }
            Err(e) => tlog!(
                "[tsf] Activate step5a WARN: ITfCompartmentMgr cast failed: {:?}",
                e
            ),
        }

        // Step 6: Register ITfThreadFocusSink (NON-FATAL).
        // This sink receives OnSetThreadFocus / OnKillThreadFocus notifications
        // which may be required for keystroke routing on Windows 11.
        if let Ok(source) = tm.cast::<ITfSource>() {
            let focus_ref: windows::core::InterfaceRef<'_, ITfThreadFocusSink> =
                self.as_interface_ref();
            // SAFETY: focus_ref points to a valid COM interface (self implements ITfThreadFocusSink).
            match unsafe { source.AdviseSink(&<ITfThreadFocusSink as Interface>::IID, &*focus_ref) }
            {
                Ok(cookie) => {
                    *self.focus_sink_cookie.borrow_mut() = Some(cookie);
                    tlog!(
                        "[tsf] Activate step6: ThreadFocusSink registered cookie={}",
                        cookie
                    );
                }
                Err(e) => tlog!(
                    "[tsf] Activate step6 WARN: ThreadFocusSink AdviseSink failed: {:?}",
                    e
                ),
            }
        }

        self.is_active.store(true, Ordering::Release);
        tlog!("[tsf] Activate END — SUCCESS (is_active=true)");
        Ok(())
    }

    fn Deactivate(&self) -> WinResult<()> {
        tlog!("[tsf] Deactivate BEGIN");
        self.is_active.store(false, Ordering::Release);

        if let Some(ref tm) = *self.thread_mgr.borrow() {
            if let Some(cookie) = *self.sink_cookie.borrow() {
                if let Ok(source) = tm.cast::<ITfSource>() {
                    // SAFETY: source is a valid ITfSource; cookie was saved from AdviseSink.
                    let _ = unsafe { source.UnadviseSink(cookie) };
                    tlog!(
                        "[tsf] Deactivate: ThreadMgrEventSink unadvised cookie={}",
                        cookie
                    );
                }
            }
            if let Ok(keystroke_mgr) = tm.cast::<ITfKeystrokeMgr>() {
                let tid = *self.client_id.borrow();
                // SAFETY: keystroke_mgr is a valid ITfKeystrokeMgr; tid matches the registered sink.
                let _ = unsafe { keystroke_mgr.UnadviseKeyEventSink(tid) };
                tlog!("[tsf] Deactivate: KeyEventSink unadvised tid={}", tid);
            }
            // Unadvise ThreadFocusSink
            if let Some(cookie) = *self.focus_sink_cookie.borrow() {
                if let Ok(source) = tm.cast::<ITfSource>() {
                    // SAFETY: source is a valid ITfSource; cookie was saved from AdviseSink.
                    let _ = unsafe { source.UnadviseSink(cookie) };
                    tlog!(
                        "[tsf] Deactivate: ThreadFocusSink unadvised cookie={}",
                        cookie
                    );
                }
            }
        }
        *self.sink_cookie.borrow_mut() = None;
        *self.focus_sink_cookie.borrow_mut() = None;

        *self.composition.borrow_mut() = None;
        *self.context_key_cookies.borrow_mut() = Vec::new();
        if let Some(mut b) = self.bridge.borrow_mut().take() {
            b.shutdown();
        }
        *self.thread_mgr.borrow_mut() = None;
        *self.current_doc_mgr.borrow_mut() = None;
        tlog!("[tsf] Deactivate END");
        Ok(())
    }
}

impl ITfTextInputProcessorEx_Impl for PyrustTip_Impl {
    fn ActivateEx(
        &self,
        ptim: windows::core::Ref<'_, ITfThreadMgr>,
        tid: u32,
        _dwflags: u32,
    ) -> WinResult<()> {
        tlog!("[tsf] ActivateEx flags=0x{:x}", _dwflags);
        self.Activate(ptim, tid)
    }
}

impl ITfKeyEventSink_Impl for PyrustTip_Impl {
    fn OnSetFocus(&self, foreground: windows::core::BOOL) -> WinResult<()> {
        tlog!("[tsf] OnSetFocus foreground={}", foreground.as_bool());
        Ok(())
    }

    fn OnTestKeyDown(
        &self,
        _pic: windows::core::Ref<'_, ITfContext>,
        wparam: windows::Win32::Foundation::WPARAM,
        _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<windows::core::BOOL> {
        let vk = wparam.0 as u32;
        match self.handle_shift_key(vk, true) {
            ShiftResult::Consumed | ShiftResult::CommitThenToggle => {
                tlog!("[tsf] OnTestKeyDown vk=0x{:x} shift consumed", vk);
                return Ok(true.into());
            }
            ShiftResult::NotShift => {}
        }
        let consume = self.should_consume_key(vk);
        tlog!("[tsf] OnTestKeyDown vk=0x{:x} consume={consume}", vk);
        Ok(consume.into())
    }

    fn OnKeyDown(
        &self,
        pic: windows::core::Ref<'_, ITfContext>,
        wparam: windows::Win32::Foundation::WPARAM,
        _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<windows::core::BOOL> {
        let vk = wparam.0 as u32;
        match self.handle_shift_key(vk, true) {
            ShiftResult::Consumed | ShiftResult::CommitThenToggle => {
                tlog!("[tsf] OnKeyDown vk=0x{:x} shift consumed", vk);
                return Ok(true.into());
            }
            ShiftResult::NotShift => {}
        }
        tlog!("[tsf] OnKeyDown vk=0x{:x}", vk);
        if !self.is_active.load(Ordering::Acquire) {
            return Ok(false.into());
        }
        if let Some(ctx) = pic.as_ref() {
            self.handle_keypress(ctx, vk)
        } else {
            Ok(false.into())
        }
    }

    fn OnTestKeyUp(
        &self,
        _pic: windows::core::Ref<'_, ITfContext>,
        wparam: windows::Win32::Foundation::WPARAM,
        _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<windows::core::BOOL> {
        let vk = wparam.0 as u32;
        match self.handle_shift_key(vk, false) {
            ShiftResult::Consumed | ShiftResult::CommitThenToggle => return Ok(true.into()),
            ShiftResult::NotShift => {}
        }
        Ok(false.into())
    }

    fn OnKeyUp(
        &self,
        _pic: windows::core::Ref<'_, ITfContext>,
        wparam: windows::Win32::Foundation::WPARAM,
        _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<windows::core::BOOL> {
        let vk = wparam.0 as u32;
        match self.handle_shift_key(vk, false) {
            ShiftResult::CommitThenToggle => {
                tlog!("[tsf] OnKeyUp vk=0x{:x} Shift commit+toggle", vk);
                if let Some(ctx) = self.get_context() {
                    let _ = self.handle_keypress(&ctx, 0x0D);
                }
                if let Some(ref bridge) = *self.bridge.borrow() {
                    let _ = bridge.req_tx().send(crate::Request::ToggleMode);
                    tlog!("[tsf] Shift toggle: committed pinyin + mode switched");
                }
                return Ok(true.into());
            }
            ShiftResult::Consumed => return Ok(true.into()),
            ShiftResult::NotShift => {}
        }
        Ok(false.into())
    }

    fn OnPreservedKey(
        &self,
        _pic: windows::core::Ref<'_, ITfContext>,
        _pguid: *const GUID,
    ) -> WinResult<windows::core::BOOL> {
        Ok(false.into())
    }
}

impl ITfContextKeyEventSink_Impl for PyrustTip_Impl {
    fn OnTestKeyDown(
        &self,
        wparam: windows::Win32::Foundation::WPARAM,
        _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<windows::core::BOOL> {
        let vk = wparam.0 as u32;
        match self.handle_shift_key(vk, true) {
            ShiftResult::Consumed | ShiftResult::CommitThenToggle => {
                tlog!("[tsf] CtxOnTestKeyDown vk=0x{:x} shift consumed", vk);
                return Ok(true.into());
            }
            ShiftResult::NotShift => {}
        }
        let consume = self.should_consume_key(vk);
        tlog!("[tsf] CtxOnTestKeyDown vk=0x{:x} consume={consume}", vk);
        Ok(consume.into())
    }

    fn OnKeyDown(
        &self,
        wparam: windows::Win32::Foundation::WPARAM,
        _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<windows::core::BOOL> {
        let vk = wparam.0 as u32;
        match self.handle_shift_key(vk, true) {
            ShiftResult::Consumed | ShiftResult::CommitThenToggle => {
                tlog!("[tsf] CtxOnKeyDown vk=0x{:x} shift consumed", vk);
                return Ok(true.into());
            }
            ShiftResult::NotShift => {}
        }
        tlog!("[tsf] CtxOnKeyDown vk=0x{:x}", vk);
        if !self.is_active.load(Ordering::Acquire) {
            return Ok(false.into());
        }
        if let Some(ctx) = self.get_context() {
            self.handle_keypress(&ctx, vk)
        } else {
            tlog!("[tsf] CtxOnKeyDown: no context available");
            Ok(false.into())
        }
    }

    fn OnTestKeyUp(
        &self,
        wparam: windows::Win32::Foundation::WPARAM,
        _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<windows::core::BOOL> {
        let vk = wparam.0 as u32;
        match self.handle_shift_key(vk, false) {
            ShiftResult::Consumed | ShiftResult::CommitThenToggle => return Ok(true.into()),
            ShiftResult::NotShift => {}
        }
        Ok(false.into())
    }

    fn OnKeyUp(
        &self,
        wparam: windows::Win32::Foundation::WPARAM,
        _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<windows::core::BOOL> {
        let vk = wparam.0 as u32;
        match self.handle_shift_key(vk, false) {
            ShiftResult::CommitThenToggle => {
                tlog!("[tsf] CtxOnKeyUp vk=0x{:x} Shift commit+toggle", vk);
                if let Some(ctx) = self.get_context() {
                    let _ = self.handle_keypress(&ctx, 0x0D);
                }
                if let Some(ref bridge) = *self.bridge.borrow() {
                    let _ = bridge.req_tx().send(crate::Request::ToggleMode);
                    tlog!("[tsf] Shift toggle: committed pinyin + mode switched");
                }
                return Ok(true.into());
            }
            ShiftResult::Consumed => return Ok(true.into()),
            ShiftResult::NotShift => {}
        }
        Ok(false.into())
    }
}

impl ITfCompositionSink_Impl for PyrustTip_Impl {
    fn OnCompositionTerminated(
        &self,
        _ec_write: u32,
        _pcomposition: windows::core::Ref<'_, ITfComposition>,
    ) -> WinResult<()> {
        tlog!("[tsf] OnCompositionTerminated");
        *self.composition.borrow_mut() = None;
        Ok(())
    }
}

impl ITfDisplayAttributeProvider_Impl for PyrustTip_Impl {
    fn EnumDisplayAttributeInfo(
        &self,
    ) -> WinResult<windows::Win32::UI::TextServices::IEnumTfDisplayAttributeInfo> {
        tlog!("[tsf] EnumDisplayAttributeInfo -> returning empty enum");
        Ok(crate::display_attrs::EmptyEnumDisplayAttr {}.into())
    }
    fn GetDisplayAttributeInfo(
        &self,
        _guid: *const GUID,
    ) -> WinResult<windows::Win32::UI::TextServices::ITfDisplayAttributeInfo> {
        tlog!("[tsf] GetDisplayAttributeInfo -> E_NOTIMPL");
        Err(Error::new(HRESULT(0x80004001u32 as i32), "Not implemented"))
    }
}

impl ITfThreadMgrEventSink_Impl for PyrustTip_Impl {
    fn OnInitDocumentMgr(&self, pdim: windows::core::Ref<'_, ITfDocumentMgr>) -> WinResult<()> {
        tlog!("[tsf] OnInitDocumentMgr has_doc={}", pdim.is_some());
        *self.current_doc_mgr.borrow_mut() = pdim.cloned();
        Ok(())
    }
    fn OnUninitDocumentMgr(&self, _pdim: windows::core::Ref<'_, ITfDocumentMgr>) -> WinResult<()> {
        tlog!("[tsf] OnUninitDocumentMgr");
        *self.current_doc_mgr.borrow_mut() = None;
        Ok(())
    }
    fn OnSetFocus(
        &self,
        pdim_focus: windows::core::Ref<'_, ITfDocumentMgr>,
        pdim_prev: windows::core::Ref<'_, ITfDocumentMgr>,
    ) -> WinResult<()> {
        tlog!(
            "[tsf] OnSetFocus gain={} lost={}",
            pdim_focus.is_some(),
            pdim_prev.is_some()
        );
        if pdim_prev.is_some() && pdim_focus.is_none() {
            if let Some(ref bridge) = *self.bridge.borrow() {
                let _ = bridge.req_tx().send(crate::Request::Reset);
            }
        }
        *self.current_doc_mgr.borrow_mut() = pdim_focus.cloned();
        Ok(())
    }
    fn OnPushContext(&self, pic: windows::core::Ref<'_, ITfContext>) -> WinResult<()> {
        tlog!("[tsf] OnPushContext");
        // Install ITfContextKeyEventSink on the new context
        if let Some(ctx) = pic.as_ref() {
            if let Ok(source) = ctx.cast::<ITfSource>() {
                let key_sink_ref: windows::core::InterfaceRef<'_, ITfContextKeyEventSink> =
                    self.as_interface_ref();
                // SAFETY: key_sink_ref points to a valid COM interface (self implements ITfContextKeyEventSink).
                match unsafe {
                    source.AdviseSink(&<ITfContextKeyEventSink as Interface>::IID, &*key_sink_ref)
                } {
                    Ok(cookie) => {
                        self.context_key_cookies.borrow_mut().push(cookie);
                        tlog!(
                            "[tsf] OnPushContext: ContextKeyEventSink installed cookie={}",
                            cookie
                        );
                    }
                    Err(e) => tlog!(
                        "[tsf] OnPushContext: ContextKeyEventSink AdviseSink failed: {:?}",
                        e
                    ),
                }
            }
        }
        Ok(())
    }
    fn OnPopContext(&self, pic: windows::core::Ref<'_, ITfContext>) -> WinResult<()> {
        tlog!("[tsf] OnPopContext");
        // Uninstall ITfContextKeyEventSink — pop last cookie
        if let Some(ctx) = pic.as_ref() {
            if let Some(cookie) = self.context_key_cookies.borrow_mut().pop() {
                if let Ok(source) = ctx.cast::<ITfSource>() {
                    // SAFETY: source is a valid ITfSource; cookie was saved from AdviseSink in OnPushContext.
                    let _ = unsafe { source.UnadviseSink(cookie) };
                    tlog!(
                        "[tsf] OnPopContext: ContextKeyEventSink unadvised cookie={}",
                        cookie
                    );
                }
            }
        }
        Ok(())
    }
}

impl ITfThreadFocusSink_Impl for PyrustTip_Impl {
    fn OnSetThreadFocus(&self) -> WinResult<()> {
        tlog!("[tsf] OnSetThreadFocus");
        Ok(())
    }
    fn OnKillThreadFocus(&self) -> WinResult<()> {
        tlog!("[tsf] OnKillThreadFocus");
        Ok(())
    }
}
