use windows::core::{implement, ComObjectInterface, Error, GUID, HRESULT, Interface, Result as WinResult};
use windows::Win32::Foundation::BOOL;
use windows::Win32::System::Com::{
    CoFreeUnusedLibraries, IClassFactory, IClassFactory_Impl,
};
use windows::Win32::UI::TextServices::{
    ITfCompositionSink, ITfCompositionSink_Impl,
    ITfContextKeyEventSink, ITfContextKeyEventSink_Impl,
    ITfDisplayAttributeProvider, ITfDisplayAttributeProvider_Impl,
    ITfKeyEventSink, ITfKeyEventSink_Impl,
    ITfKeystrokeMgr,
    ITfSource,
    ITfTextInputProcessorEx, ITfTextInputProcessorEx_Impl,
    ITfTextInputProcessor, ITfTextInputProcessor_Impl,
    ITfThreadMgrEventSink, ITfThreadMgrEventSink_Impl,
    ITfThreadMgr, ITfDocumentMgr, ITfContext, ITfComposition,
};
use std::cell::RefCell;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::bridge::TsfBridge;

pub(crate) static DLL_REF_COUNT: AtomicI32 = AtomicI32::new(0);

/// Write a diagnostic message to the log file on disk.
/// In a TSF DLL there is no console; this is the only way to see what happens.
fn tsf_log(msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("C:\\Users\\Verdana\\pyrust_tsf.log")
    {
        let _ = writeln!(f, "{msg}");
        let _ = f.flush();
    }
}

macro_rules! tlog {
    ($($arg:tt)*) => { tsf_log(&format!($($arg)*)) };
}

#[implement(IClassFactory)]
pub struct PyrustClassFactory;

impl IClassFactory_Impl for PyrustClassFactory_Impl {
    fn CreateInstance(
        &self,
        punkouter: Option<&windows::core::IUnknown>,
        riid: *const GUID,
        ppv: *mut *mut std::ffi::c_void,
    ) -> WinResult<()> {
        if punkouter.is_some() {
            return Err(Error::new(HRESULT(0x80040110u32 as i32), "Aggregation not supported"));
        }
        let tip: ITfTextInputProcessorEx = PyrustTip::new().into();
        unsafe { tip.query(riid, ppv) }.ok()?;
        Ok(())
    }

    fn LockServer(&self, lock: BOOL) -> WinResult<()> {
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
    ITfThreadMgrEventSink
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
        }
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
    fn Activate(&self, ptim: Option<&ITfThreadMgr>, tid: u32) -> WinResult<()> {
        tlog!("[tsf] Activate BEGIN tid={}", tid);

        // Step 1: thread_mgr — THIS IS THE ONLY FATAL STEP
        let tm = match ptim {
            Some(tm) => {
                tlog!("[tsf] Activate step1: got ITfThreadMgr");
                tm.clone()
            }
            None => {
                tlog!("[tsf] Activate FATAL: no ITfThreadMgr provided");
                return Err(Error::new(HRESULT(0x80004003u32 as i32), "No thread mgr"));
            }
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
                    keystroke_mgr.AdviseKeyEventSink(tid, &*key_sink_ref, BOOL(1))
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
        }
        *self.sink_cookie.borrow_mut() = None;

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
    fn ActivateEx(&self, ptim: Option<&ITfThreadMgr>, tid: u32, _dwflags: u32) -> WinResult<()> {
        tlog!("[tsf] ActivateEx flags=0x{:x}", _dwflags);
        self.Activate(ptim, tid)
    }
}

impl ITfKeyEventSink_Impl for PyrustTip_Impl {
    fn OnSetFocus(&self, foreground: BOOL) -> WinResult<()> {
        tlog!("[tsf] OnSetFocus foreground={}", foreground.as_bool());
        Ok(())
    }

    fn OnTestKeyDown(
        &self,
        _pic: Option<&ITfContext>,
        _wparam: windows::Win32::Foundation::WPARAM,
        _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<BOOL> {
        let active = self.is_active.load(Ordering::Acquire);
        tlog!("[tsf] OnTestKeyDown vk=0x{:x} active={active}", _wparam.0 as u32);
        if active { Ok(BOOL(1)) } else { Ok(BOOL(0)) }
    }

    fn OnKeyDown(
        &self,
        pic: Option<&ITfContext>,
        wparam: windows::Win32::Foundation::WPARAM,
        _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<BOOL> {
        let vk = wparam.0 as u32;
        tlog!("[tsf] OnKeyDown vk=0x{:x}", vk);
        if !self.is_active.load(Ordering::Acquire) {
            return Ok(BOOL(0));
        }
        if let Some(ref bridge) = *self.bridge.borrow() {
            let modifiers = crate::Modifiers::default();
            let (resp_tx, resp_rx) = crate::oneshot::channel();
            let _ = bridge.req_tx().send(crate::Request::KeyPress { vk, modifiers, response: resp_tx });
            let response = resp_rx.recv();
            match response {
                crate::Response::Consumed | crate::Response::Committed(_) => {
                    if let Some(ref context) = pic {
                        bridge.request_edit(context);
                    }
                    Ok(BOOL(1))
                }
                crate::Response::Passthrough => Ok(BOOL(0)),
            }
        } else {
            tlog!("[tsf] OnKeyDown: no bridge, passing through");
            Ok(BOOL(0))
        }
    }

    fn OnTestKeyUp(
        &self, _pic: Option<&ITfContext>, _w: windows::Win32::Foundation::WPARAM, _l: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<BOOL> { Ok(BOOL(0)) }

    fn OnKeyUp(
        &self, _pic: Option<&ITfContext>, _w: windows::Win32::Foundation::WPARAM, _l: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<BOOL> { Ok(BOOL(0)) }

    fn OnPreservedKey(
        &self, _pic: Option<&ITfContext>, _pguid: *const GUID,
    ) -> WinResult<BOOL> { Ok(BOOL(0)) }
}

impl ITfContextKeyEventSink_Impl for PyrustTip_Impl {
    fn OnTestKeyDown(
        &self,
        wparam: windows::Win32::Foundation::WPARAM,
        _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<BOOL> {
        let active = self.is_active.load(Ordering::Acquire);
        tlog!("[tsf] CtxOnTestKeyDown vk=0x{:x} active={active}", wparam.0 as u32);
        if active { Ok(BOOL(1)) } else { Ok(BOOL(0)) }
    }

    fn OnKeyDown(
        &self,
        wparam: windows::Win32::Foundation::WPARAM,
        _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<BOOL> {
        let vk = wparam.0 as u32;
        tlog!("[tsf] CtxOnKeyDown vk=0x{:x}", vk);
        if !self.is_active.load(Ordering::Acquire) {
            return Ok(BOOL(0));
        }
        if let Some(ref bridge) = *self.bridge.borrow() {
            let modifiers = crate::Modifiers::default();
            let (resp_tx, resp_rx) = crate::oneshot::channel();
            let _ = bridge.req_tx().send(crate::Request::KeyPress { vk, modifiers, response: resp_tx });
            let response = resp_rx.recv();
            match response {
                crate::Response::Consumed | crate::Response::Committed(_) => Ok(BOOL(1)),
                crate::Response::Passthrough => Ok(BOOL(0)),
            }
        } else {
            tlog!("[tsf] CtxOnKeyDown: no bridge, passing through");
            Ok(BOOL(0))
        }
    }

    fn OnTestKeyUp(
        &self, _wparam: windows::Win32::Foundation::WPARAM, _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<BOOL> { Ok(BOOL(0)) }

    fn OnKeyUp(
        &self, _wparam: windows::Win32::Foundation::WPARAM, _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<BOOL> { Ok(BOOL(0)) }
}

impl ITfCompositionSink_Impl for PyrustTip_Impl {
    fn OnCompositionTerminated(&self, _ec_write: u32, _pcomposition: Option<&ITfComposition>) -> WinResult<()> {
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
    fn OnInitDocumentMgr(&self, pdim: Option<&ITfDocumentMgr>) -> WinResult<()> {
        tlog!("[tsf] OnInitDocumentMgr has_doc={}", pdim.is_some());
        *self.current_doc_mgr.borrow_mut() = pdim.cloned();
        Ok(())
    }
    fn OnUninitDocumentMgr(&self, _pdim: Option<&ITfDocumentMgr>) -> WinResult<()> {
        tlog!("[tsf] OnUninitDocumentMgr");
        *self.current_doc_mgr.borrow_mut() = None;
        Ok(())
    }
    fn OnSetFocus(&self, pdim_focus: Option<&ITfDocumentMgr>, pdim_prev: Option<&ITfDocumentMgr>) -> WinResult<()> {
        tlog!("[tsf] OnSetFocus gain={} lost={}", pdim_focus.is_some(), pdim_prev.is_some());
        if pdim_prev.is_some() && pdim_focus.is_none() {
            if let Some(ref bridge) = *self.bridge.borrow() {
                let _ = bridge.req_tx().send(crate::Request::Reset);
            }
        }
        *self.current_doc_mgr.borrow_mut() = pdim_focus.cloned();
        Ok(())
    }
    fn OnPushContext(&self, pic: Option<&ITfContext>) -> WinResult<()> {
        tlog!("[tsf] OnPushContext");
        // Install ITfContextKeyEventSink on the new context
        if let Some(ctx) = pic {
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
    fn OnPopContext(&self, pic: Option<&ITfContext>) -> WinResult<()> {
        tlog!("[tsf] OnPopContext");
        // Uninstall ITfContextKeyEventSink
        if let Some(ctx) = pic {
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
