//! TSF (Text Services Framework) COM integration for pyrust.
//!
//! This crate implements the Windows TSF interfaces needed to register
//! pyrust as a system-level text input processor (TIP).

use std::io::{BufWriter, Write};
use std::sync::Mutex;

/// Shared buffered log writer — keeps the file handle open across calls
/// to avoid the cost of open/close on every keystroke.
static LOG_WRITER: Mutex<Option<BufWriter<std::fs::File>>> = Mutex::new(None);

/// Write a diagnostic message to the log file on disk.
/// In a TSF DLL there is no console; this is the only way to see what happens.
pub(crate) fn tsf_log(msg: &str) {
    let mut guard = match LOG_WRITER.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    if guard.is_none() {
        if let Ok(f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("C:\\Users\\Verdana\\pyrust_tsf.log")
        {
            *guard = Some(BufWriter::new(f));
        }
    }
    if let Some(ref mut writer) = *guard {
        let _ = writeln!(writer, "{msg}");
        let _ = writer.flush();
    }
}

macro_rules! tlog {
    ($($arg:tt)*) => { $crate::tsf_log(&format!($($arg)*)) };
}
pub(crate) use tlog;

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
        /// Receive the value. Returns `None` if the sender was dropped or
        /// the 5-second timeout elapsed (worker thread likely crashed).
        pub fn recv(&self) -> Option<T> {
            use std::time::{Duration, Instant};
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(value) = self
                    .slot
                    .lock()
                    .expect("oneshot recv: lock poisoned")
                    .take()
                {
                    return Some(value);
                }
                if Instant::now() > deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(1));
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
        caret_pos: Option<(i32, i32)>,
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
