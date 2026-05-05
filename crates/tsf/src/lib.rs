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

// Re-export main Request/Response types from the binary
// These match pyrust/src/main.rs
pub use engine_core::{Action, KeyEvent, Modifiers};

/// Synchronous oneshot channel using Condvar (no busy-wait).
pub mod oneshot {
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Duration;

    struct Inner<T> {
        slot: Mutex<Option<T>>,
        condvar: Condvar,
    }

    pub struct Sender<T> {
        inner: Arc<Inner<T>>,
    }

    impl<T> std::fmt::Debug for Sender<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Sender").finish_non_exhaustive()
        }
    }

    impl<T> Sender<T> {
        pub fn send(self, value: T) {
            let mut lock = self.inner.slot.lock().expect("oneshot send: lock poisoned");
            *lock = Some(value);
            self.inner.condvar.notify_one();
        }
    }

    pub struct Receiver<T> {
        inner: Arc<Inner<T>>,
    }

    impl<T> Receiver<T> {
        /// Receive the value. Returns `None` if the sender was dropped or
        /// the 200ms timeout elapsed (worker thread likely stalled).
        pub fn recv(&self) -> Option<T> {
            let mut lock = self.inner.slot.lock().expect("oneshot recv: lock poisoned");
            while lock.is_none() {
                let result = self
                    .inner
                    .condvar
                    .wait_timeout(lock, Duration::from_millis(200))
                    .expect("oneshot recv: lock poisoned");
                lock = result.0;
                if lock.is_none() {
                    return None; // actual timeout
                }
            }
            lock.take()
        }
    }

    pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
        let inner = Arc::new(Inner {
            slot: Mutex::new(None),
            condvar: Condvar::new(),
        });
        (
            Sender { inner: inner.clone() },
            Receiver { inner },
        )
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
    /// Pinyin buffer updated — carries current preedit text for composition.
    ConsumedWithText(String),
    Passthrough,
    Committed(String),
    /// Text committed + new preedit started (e.g., letter while composing).
    CommittedWithPreedit(String, String),
}
