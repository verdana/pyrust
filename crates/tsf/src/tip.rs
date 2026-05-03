use windows::core::{implement, ComObjectInterface, Error, GUID, HRESULT, Interface, Result as WinResult};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::System::Com::{
    CoFreeUnusedLibraries, IClassFactory, IClassFactory_Impl,
};
use windows::Win32::UI::TextServices::{
    ITfCompositionSink, ITfCompositionSink_Impl,
    ITfCompartmentMgr,
    ITfContextKeyEventSink, ITfContextKeyEventSink_Impl,
    ITfDisplayAttributeProvider, ITfDisplayAttributeProvider_Impl,
    ITfEditSession,
    ITfKeyEventSink, ITfKeyEventSink_Impl,
    ITfKeystrokeMgr,
    ITfSource,
    ITfTextInputProcessorEx, ITfTextInputProcessorEx_Impl,
    ITfTextInputProcessor, ITfTextInputProcessor_Impl,
    ITfThreadFocusSink, ITfThreadFocusSink_Impl,
    ITfThreadMgrEventSink, ITfThreadMgrEventSink_Impl,
    ITfThreadMgr, ITfDocumentMgr, ITfContext, ITfComposition,
    GUID_COMPARTMENT_KEYBOARD_OPENCLOSE,
};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::bridge::TsfBridge;
#[allow(unused_imports)]
use crate::tlog;

pub(crate) static DLL_REF_COUNT: AtomicI32 = AtomicI32::new(0);

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
            return Err(Error::new(HRESULT(0x80040110u32 as i32), "Aggregation not supported"));
        }
        let tip: ITfTextInputProcessorEx = PyrustTip::new().into();
        unsafe { tip.query(riid, ppv) }.ok()?;
        Ok(())
    }

    fn LockServer(&self, lock: windows::core::BOOL) -> WinResult<()> {
        if lock.as_bool() {
            DLL_REF_COUNT.fetch_add(1, Ordering::Release);
        } else {
            DLL_REF_COUNT.fetch_sub(1, Ordering::Release);
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
    context_key_cookie: RefCell<Option<u32>>,
    focus_sink_cookie: RefCell<Option<u32>>,
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
            context_key_cookie: RefCell::new(None),
            focus_sink_cookie: RefCell::new(None),
        }
    }

    /// Get the current ITfContext from the active document manager.
    pub fn get_context(&self) -> Option<ITfContext> {
        self.current_doc_mgr.borrow().as_ref().and_then(|dm| {
            unsafe { dm.GetTop() }.ok()
        })
    }
}

impl Drop for PyrustTip {
    fn drop(&mut self) {
        if let Some(mut b) = self.bridge.borrow_mut().take() {
            b.shutdown();
        }
        DLL_REF_COUNT.fetch_sub(1, Ordering::Release);
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
                let sink_ref: windows::core::InterfaceRef<'_, ITfThreadMgrEventSink> = self.as_interface_ref();
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
                let key_sink_ref: windows::core::InterfaceRef<'_, ITfKeyEventSink> = self.as_interface_ref();
                match unsafe {
                    keystroke_mgr.AdviseKeyEventSink(tid, &*key_sink_ref, true)
                } {
                    Ok(()) => tlog!("[tsf] Activate step3b: AdviseKeyEventSink OK"),
                    Err(e) => tlog!("[tsf] Activate step3b WARN: AdviseKeyEventSink failed: {:?}", e),
                }
            }
            Err(e) => tlog!("[tsf] Activate step3a WARN: ITfKeystrokeMgr cast failed: {:?}", e),
        }

        // Step 4: Initialize engine bridge (worker + forwarder + config watcher).
        // UI thread is NOT started here — creating an egui window from inside
        // the TSF COM callback causes COM re-entrancy deadlock → Explorer crash.
        match TsfBridge::initialize_engine() {
            Ok(bridge) => {
                *self.bridge.borrow_mut() = Some(bridge);
                tlog!("[tsf] Activate step4: Engine bridge OK (UI deferred)");
            }
            Err(e) => tlog!("[tsf] Activate step4 WARN: Engine bridge init failed: {:?}", e),
        }

        // Step 5: Set keyboard open state via compartment.
        // Windows 11 TextInputHost may skip keystroke routing if the
        // keyboard compartment is not explicitly set to "open".
        match tm.cast::<ITfCompartmentMgr>() {
            Ok(comp_mgr) => {
                tlog!("[tsf] Activate step5a: ITfCompartmentMgr cast OK");
                match unsafe { comp_mgr.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE) } {
                    Ok(comp) => {
                        let val = VARIANT::from(1i32);
                        match unsafe { comp.SetValue(tid, &val) } {
                            Ok(()) => tlog!("[tsf] Activate step5b: Keyboard compartment set to OPEN"),
                            Err(e) => tlog!("[tsf] Activate step5b WARN: SetValue failed: {:?}", e),
                        }
                    }
                    Err(e) => tlog!("[tsf] Activate step5a WARN: GetCompartment failed: {:?}", e),
                }
            }
            Err(e) => tlog!("[tsf] Activate step5a WARN: ITfCompartmentMgr cast failed: {:?}", e),
        }

        // Step 6: Register ITfThreadFocusSink (NON-FATAL).
        // This sink receives OnSetThreadFocus / OnKillThreadFocus notifications
        // which may be required for keystroke routing on Windows 11.
        if let Ok(source) = tm.cast::<ITfSource>() {
            let focus_ref: windows::core::InterfaceRef<'_, ITfThreadFocusSink> = self.as_interface_ref();
            match unsafe {
                source.AdviseSink(&<ITfThreadFocusSink as Interface>::IID, &*focus_ref)
            } {
                Ok(cookie) => {
                    *self.focus_sink_cookie.borrow_mut() = Some(cookie);
                    tlog!("[tsf] Activate step6: ThreadFocusSink registered cookie={}", cookie);
                }
                Err(e) => tlog!("[tsf] Activate step6 WARN: ThreadFocusSink AdviseSink failed: {:?}", e),
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
                    let _ = unsafe { source.UnadviseSink(cookie) };
                    tlog!("[tsf] Deactivate: ThreadMgrEventSink unadvised cookie={}", cookie);
                }
            }
            if let Ok(keystroke_mgr) = tm.cast::<ITfKeystrokeMgr>() {
                let tid = *self.client_id.borrow();
                let _ = unsafe { keystroke_mgr.UnadviseKeyEventSink(tid) };
                tlog!("[tsf] Deactivate: KeyEventSink unadvised tid={}", tid);
            }
            // Unadvise ThreadFocusSink
            if let Some(cookie) = *self.focus_sink_cookie.borrow() {
                if let Ok(source) = tm.cast::<ITfSource>() {
                    let _ = unsafe { source.UnadviseSink(cookie) };
                    tlog!("[tsf] Deactivate: ThreadFocusSink unadvised cookie={}", cookie);
                }
            }
        }
        *self.sink_cookie.borrow_mut() = None;
        *self.focus_sink_cookie.borrow_mut() = None;

        *self.composition.borrow_mut() = None;
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
    fn ActivateEx(&self, ptim: windows::core::Ref<'_, ITfThreadMgr>, tid: u32, _dwflags: u32) -> WinResult<()> {
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
        _wparam: windows::Win32::Foundation::WPARAM,
        _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<windows::core::BOOL> {
        let active = self.is_active.load(Ordering::Acquire);
        tlog!("[tsf] OnTestKeyDown vk=0x{:x} active={active}", _wparam.0 as u32);
        Ok(active.into())
    }

    fn OnKeyDown(
        &self,
        pic: windows::core::Ref<'_, ITfContext>,
        wparam: windows::Win32::Foundation::WPARAM,
        _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<windows::core::BOOL> {
        let vk = wparam.0 as u32;
        tlog!("[tsf] OnKeyDown vk=0x{:x}", vk);
        if !self.is_active.load(Ordering::Acquire) {
            return Ok(false.into());
        }
        if let Some(ref bridge) = *self.bridge.borrow() {
            let modifiers = crate::Modifiers::default();
            let (resp_tx, resp_rx) = crate::oneshot::channel();
            let _ = bridge.req_tx().send(crate::Request::KeyPress { vk, modifiers, response: resp_tx });
            let response = resp_rx.recv();
            match response {
                crate::Response::Committed(text) => {
                    tlog!("[tsf] OnKeyDown: Committed '{}'", text);
                    // Create edit session to insert text into the application
                    if let Some(context) = pic.as_ref() {
                        use windows::Win32::UI::TextServices::TF_ES_READWRITE;
                        let edit_session: ITfEditSession = crate::edit_session::CommitEditSession::new(
                            context.clone(), text,
                        ).into();
                        let _ = unsafe {
                            context.RequestEditSession(
                                *self.client_id.borrow(),
                                &edit_session,
                                TF_ES_READWRITE,
                            )
                        };
                        tlog!("[tsf] OnKeyDown: RequestEditSession called");
                    }
                    Ok(true.into())
                }
                crate::Response::Consumed => {
                    Ok(true.into())
                }
                crate::Response::Passthrough => Ok(false.into()),
            }
        } else {
            tlog!("[tsf] OnKeyDown: no bridge, passing through");
            Ok(false.into())
        }
    }

    fn OnTestKeyUp(
        &self, _pic: windows::core::Ref<'_, ITfContext>, _w: windows::Win32::Foundation::WPARAM, _l: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<windows::core::BOOL> { Ok(false.into()) }

    fn OnKeyUp(
        &self, _pic: windows::core::Ref<'_, ITfContext>, _w: windows::Win32::Foundation::WPARAM, _l: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<windows::core::BOOL> { Ok(false.into()) }

    fn OnPreservedKey(
        &self, _pic: windows::core::Ref<'_, ITfContext>, _pguid: *const GUID,
    ) -> WinResult<windows::core::BOOL> { Ok(false.into()) }
}

impl ITfContextKeyEventSink_Impl for PyrustTip_Impl {
    fn OnTestKeyDown(
        &self,
        wparam: windows::Win32::Foundation::WPARAM,
        _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<windows::core::BOOL> {
        let active = self.is_active.load(Ordering::Acquire);
        tlog!("[tsf] CtxOnTestKeyDown vk=0x{:x} active={active}", wparam.0 as u32);
        Ok(active.into())
    }

    fn OnKeyDown(
        &self,
        wparam: windows::Win32::Foundation::WPARAM,
        _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<windows::core::BOOL> {
        let vk = wparam.0 as u32;
        tlog!("[tsf] CtxOnKeyDown vk=0x{:x}", vk);
        if !self.is_active.load(Ordering::Acquire) {
            return Ok(false.into());
        }
        if let Some(ref bridge) = *self.bridge.borrow() {
            let modifiers = crate::Modifiers::default();
            let (resp_tx, resp_rx) = crate::oneshot::channel();
            let _ = bridge.req_tx().send(crate::Request::KeyPress { vk, modifiers, response: resp_tx });
            let response = resp_rx.recv();
            match response {
                crate::Response::Committed(text) => {
                    tlog!("[tsf] CtxOnKeyDown: Committed '{}'", text);
                    // Insert committed text via edit session using the stored context
                    if let Some(context) = self.get_context() {
                        use windows::Win32::UI::TextServices::TF_ES_READWRITE;
                        let edit_session: ITfEditSession = crate::edit_session::CommitEditSession::new(
                            context.clone(), text,
                        ).into();
                        let _ = unsafe {
                            context.RequestEditSession(
                                *self.client_id.borrow(),
                                &edit_session,
                                TF_ES_READWRITE,
                            )
                        };
                        tlog!("[tsf] CtxOnKeyDown: RequestEditSession called");
                    } else {
                        tlog!("[tsf] CtxOnKeyDown: no context available, text LOST");
                    }
                    Ok(true.into())
                }
                crate::Response::Consumed => Ok(true.into()),
                crate::Response::Passthrough => Ok(false.into()),
            }
        } else {
            tlog!("[tsf] CtxOnKeyDown: no bridge, passing through");
            Ok(false.into())
        }
    }

    fn OnTestKeyUp(
        &self, _wparam: windows::Win32::Foundation::WPARAM, _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<windows::core::BOOL> { Ok(false.into()) }

    fn OnKeyUp(
        &self, _wparam: windows::Win32::Foundation::WPARAM, _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<windows::core::BOOL> { Ok(false.into()) }
}

impl ITfCompositionSink_Impl for PyrustTip_Impl {
    fn OnCompositionTerminated(&self, _ec_write: u32, _pcomposition: windows::core::Ref<'_, ITfComposition>) -> WinResult<()> {
        tlog!("[tsf] OnCompositionTerminated");
        *self.composition.borrow_mut() = None;
        Ok(())
    }
}

impl ITfDisplayAttributeProvider_Impl for PyrustTip_Impl {
    fn EnumDisplayAttributeInfo(&self) -> WinResult<windows::Win32::UI::TextServices::IEnumTfDisplayAttributeInfo> {
        tlog!("[tsf] EnumDisplayAttributeInfo -> returning empty enum");
        Ok(crate::display_attrs::EmptyEnumDisplayAttr {}.into())
    }
    fn GetDisplayAttributeInfo(&self, _guid: *const GUID) -> WinResult<windows::Win32::UI::TextServices::ITfDisplayAttributeInfo> {
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
    fn OnSetFocus(&self, pdim_focus: windows::core::Ref<'_, ITfDocumentMgr>, pdim_prev: windows::core::Ref<'_, ITfDocumentMgr>) -> WinResult<()> {
        tlog!("[tsf] OnSetFocus gain={} lost={}", pdim_focus.is_some(), pdim_prev.is_some());
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
                let key_sink_ref: windows::core::InterfaceRef<'_, ITfContextKeyEventSink> = self.as_interface_ref();
                match unsafe {
                    source.AdviseSink(&<ITfContextKeyEventSink as Interface>::IID, &*key_sink_ref)
                } {
                    Ok(cookie) => {
                        *self.context_key_cookie.borrow_mut() = Some(cookie);
                        tlog!("[tsf] OnPushContext: ContextKeyEventSink installed cookie={}", cookie);
                    }
                    Err(e) => tlog!("[tsf] OnPushContext: ContextKeyEventSink AdviseSink failed: {:?}", e),
                }
            }
        }
        Ok(())
    }
    fn OnPopContext(&self, pic: windows::core::Ref<'_, ITfContext>) -> WinResult<()> {
        tlog!("[tsf] OnPopContext");
        // Uninstall ITfContextKeyEventSink
        if let Some(ctx) = pic.as_ref() {
            if let Some(cookie) = *self.context_key_cookie.borrow() {
                if let Ok(source) = ctx.cast::<ITfSource>() {
                    let _ = unsafe { source.UnadviseSink(cookie) };
                    tlog!("[tsf] OnPopContext: ContextKeyEventSink unadvised cookie={}", cookie);
                }
            }
        }
        *self.context_key_cookie.borrow_mut() = None;
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
