use windows::core::{implement, Error, GUID, HRESULT, Interface, Result as WinResult};
use windows::Win32::Foundation::BOOL;
use windows::Win32::System::Com::{
    CoFreeUnusedLibraries, IClassFactory, IClassFactory_Impl,
};
use windows::Win32::UI::TextServices::{
    ITfCompositionSink, ITfCompositionSink_Impl,
    ITfDisplayAttributeProvider, ITfDisplayAttributeProvider_Impl,
    ITfKeyEventSink, ITfKeyEventSink_Impl,
    ITfTextInputProcessorEx, ITfTextInputProcessorEx_Impl,
    ITfTextInputProcessor, ITfTextInputProcessor_Impl,
    ITfThreadMgrEventSink, ITfThreadMgrEventSink_Impl,
    ITfThreadMgr, ITfDocumentMgr, ITfContext, ITfComposition,
};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::bridge::TsfBridge;

pub(crate) static DLL_REF_COUNT: AtomicI32 = AtomicI32::new(0);

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
        log::info!("[tsf] Activate, tid={}", tid);
        let tm = ptim.ok_or_else(|| Error::new(HRESULT(0x80004003u32 as i32), "No thread mgr"))?;
        *self.thread_mgr.borrow_mut() = Some(tm.clone());
        *self.client_id.borrow_mut() = tid;
        let bridge = TsfBridge::initialize().map_err(|e| {
            log::error!("[tsf] Bridge init failed: {}", e);
            Error::new(HRESULT(0x80004005u32 as i32), "Bridge init failed")
        })?;
        self.is_active.store(true, Ordering::Release);
        *self.bridge.borrow_mut() = Some(bridge);
        log::info!("[tsf] Activation complete");
        Ok(())
    }

    fn Deactivate(&self) -> WinResult<()> {
        log::info!("[tsf] Deactivate");
        *self.composition.borrow_mut() = None;
        self.is_active.store(false, Ordering::Release);
        if let Some(mut b) = self.bridge.borrow_mut().take() {
            b.shutdown();
        }
        *self.thread_mgr.borrow_mut() = None;
        *self.current_doc_mgr.borrow_mut() = None;
        Ok(())
    }
}

impl ITfTextInputProcessorEx_Impl for PyrustTip_Impl {
    fn ActivateEx(&self, ptim: Option<&ITfThreadMgr>, tid: u32, _dwflags: u32) -> WinResult<()> {
        self.Activate(ptim, tid)
    }
}

impl ITfKeyEventSink_Impl for PyrustTip_Impl {
    fn OnSetFocus(&self, _foreground: BOOL) -> WinResult<()> { Ok(()) }

    fn OnTestKeyDown(
        &self,
        _pic: Option<&ITfContext>,
        _wparam: windows::Win32::Foundation::WPARAM,
        _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<BOOL> {
        if self.is_active.load(Ordering::Acquire) { Ok(BOOL(1)) } else { Ok(BOOL(0)) }
    }

    fn OnKeyDown(
        &self,
        pic: Option<&ITfContext>,
        wparam: windows::Win32::Foundation::WPARAM,
        _lparam: windows::Win32::Foundation::LPARAM,
    ) -> WinResult<BOOL> {
        if !self.is_active.load(Ordering::Acquire) {
            return Ok(BOOL(0));
        }
        let vk = wparam.0 as u32;
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

impl ITfCompositionSink_Impl for PyrustTip_Impl {
    fn OnCompositionTerminated(&self, _ec_write: u32, _pcomposition: Option<&ITfComposition>) -> WinResult<()> {
        *self.composition.borrow_mut() = None;
        Ok(())
    }
}

impl ITfDisplayAttributeProvider_Impl for PyrustTip_Impl {
    fn EnumDisplayAttributeInfo(&self) -> WinResult<windows::Win32::UI::TextServices::IEnumTfDisplayAttributeInfo> {
        Err(Error::new(HRESULT(0x80004001u32 as i32), "Not implemented"))
    }
    fn GetDisplayAttributeInfo(&self, _guid: *const GUID) -> WinResult<windows::Win32::UI::TextServices::ITfDisplayAttributeInfo> {
        Err(Error::new(HRESULT(0x80004001u32 as i32), "Not implemented"))
    }
}

impl ITfThreadMgrEventSink_Impl for PyrustTip_Impl {
    fn OnInitDocumentMgr(&self, pdim: Option<&ITfDocumentMgr>) -> WinResult<()> {
        *self.current_doc_mgr.borrow_mut() = pdim.cloned();
        Ok(())
    }
    fn OnUninitDocumentMgr(&self, _pdim: Option<&ITfDocumentMgr>) -> WinResult<()> {
        *self.current_doc_mgr.borrow_mut() = None;
        Ok(())
    }
    fn OnSetFocus(&self, pdim_focus: Option<&ITfDocumentMgr>, pdim_prev: Option<&ITfDocumentMgr>) -> WinResult<()> {
        if pdim_prev.is_some() && pdim_focus.is_none() {
            if let Some(ref bridge) = *self.bridge.borrow() {
                let _ = bridge.req_tx().send(crate::Request::Reset);
            }
        }
        *self.current_doc_mgr.borrow_mut() = pdim_focus.cloned();
        Ok(())
    }
    fn OnPushContext(&self, _pic: Option<&ITfContext>) -> WinResult<()> { Ok(()) }
    fn OnPopContext(&self, _pic: Option<&ITfContext>) -> WinResult<()> { Ok(()) }
}
