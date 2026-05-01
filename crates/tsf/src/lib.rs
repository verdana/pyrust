//! TSF (Text Services Framework) COM integration for pyrust.
//!
//! This crate implements the Windows TSF interfaces needed to register
//! pyrust as a system-level text input processor (TIP).

pub mod bridge;
pub mod composition;
pub mod display_attrs;
pub mod dll_exports;
pub mod edit_session;
pub mod key_sink;
pub mod registry;
pub mod thread_mgr_events;
pub mod tip;

/// Pending edit operation queued from the worker thread.
#[derive(Debug, Clone)]
pub enum PendingEdit {
    CommitText(String),
    SetComposition { text: String, cursor: usize },
    ClearComposition,
}

// Re-export main Request/Response types from the binary
// These match pyrust/src/main.rs
pub use engine_core::{Action, KeyEvent, Modifiers};

/// The oneshot channel for synchronous Request→Response handoff.
pub mod oneshot {
    use std::sync::{Arc, Mutex};

    pub struct Sender<T> {
        slot: Arc<Mutex<Option<T>>>,
    }

    impl<T> std::fmt::Debug for Sender<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Sender").finish_non_exhaustive()
        }
    }

    impl<T> Sender<T> {
        pub fn send(self, value: T) {
            *self.slot.lock().expect("oneshot send: lock poisoned") = Some(value);
        }
    }

    pub struct Receiver<T> {
        slot: Arc<Mutex<Option<T>>>,
    }

    impl<T> Receiver<T> {
        pub fn recv(&self) -> T {
            loop {
                if let Some(value) =
                    self.slot.lock().expect("oneshot recv: lock poisoned").take()
                {
                    return value;
                }
                std::hint::spin_loop();
            }
        }
    }

    pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
        let slot = Arc::new(Mutex::new(None));
        (Sender { slot: slot.clone() }, Receiver { slot })
    }
}

/// Request types matching pyrust/src/main.rs
#[derive(Debug)]
pub enum Request {
    KeyPress {
        vk: u32,
        modifiers: Modifiers,
        response: oneshot::Sender<Response>,
    },
    SelectCandidate {
        index: usize,
        response: oneshot::Sender<Response>,
    },
    ConfigReload,
    ToggleMode,
    Reset,
    Shutdown,
}

#[derive(Debug)]
pub enum Response {
    Consumed,
    Passthrough,
    Committed(String),
}
