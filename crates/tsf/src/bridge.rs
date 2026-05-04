//! Thread and channel bridge between TSF COM callbacks and the engine worker thread.
//!
//! The UI thread is global — it starts once and never exits. This avoids the
//! "winit EventLoop can't be recreated" error when the DLL is unloaded/reloaded.

use std::sync::Arc;
use std::sync::Once;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use crossbeam_channel::{unbounded, Receiver, Sender};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use dict::base_dict::BaseDict;
use dict::dat_dict::MmapDict;
use dict::user_dict::UserDict;
use dict::DictSource;
use engine_core::{BigramModel, EngineCore, KeyEvent};
use ui_crate::{UiAction, UiUpdate};
use yas_config::Config;

#[allow(unused_imports)]
use crate::tlog;
use crate::{PendingEdit, Request, Response};

/// Global UI channel — initialized once, persists across DLL reloads.
static GLOBAL_UI_TX: std::sync::Mutex<Option<Sender<UiUpdate>>> = std::sync::Mutex::new(None);
static GLOBAL_ACTION_RX: std::sync::Mutex<Option<Receiver<UiAction>>> = std::sync::Mutex::new(None);
static GLOBAL_UI_INIT: Once = Once::new();

/// Holds all channels and thread handles for the TSF bridge.
pub struct TsfBridge {
    req_tx: Sender<Request>,
    edit_tx: Sender<PendingEdit>,
    _worker: thread::JoinHandle<()>,
    _forwarder: thread::JoinHandle<()>,
    _config_watcher: thread::JoinHandle<()>,
}

impl TsfBridge {
    /// Initialize engine-only threads (worker + forwarder + config watcher).
    /// The UI thread is started globally on first call and never exits.
    pub fn initialize_engine() -> Result<Self> {
        tlog!("[tsf] Initializing bridge (engine only, UI deferred)...");

        let config = Arc::new(Config::load());

        // Load dictionaries
        let dict_path = &config.dict.base_dict_path;
        let dict: Arc<dyn DictSource> = if let Some(mmap_dict) = MmapDict::open(dict_path) {
            tlog!(
                "[tsf] Loaded {} entries from mmap dictionary '{}'",
                mmap_dict.entry_count(),
                dict_path
            );
            Arc::new(mmap_dict)
        } else {
            let mut base_dict = BaseDict::new();
            if let Err(e) = base_dict.load_from_file(dict_path) {
                tlog!(
                    "[tsf] WARN: Failed to load base dictionary '{}': {e}",
                    dict_path
                );
            }
            Arc::new(base_dict)
        };

        let user_dict = UserDict::open(&config.dict.user_dict_path);
        let bigram = BigramModel::load(&config.dict.bigram_data_path);

        // Initialize global UI channel (once) — the UI thread never exits
        GLOBAL_UI_INIT.call_once(|| {
            let (ui_tx, action_rx) = ui_crate::window::init_global_ui(&config.ui);
            *GLOBAL_UI_TX
                .lock()
                .expect("GLOBAL_UI_TX poisoned during init") = Some(ui_tx);
            *GLOBAL_ACTION_RX
                .lock()
                .expect("GLOBAL_ACTION_RX poisoned during init") = Some(action_rx);
            tlog!("[tsf] Global UI initialized (Win32 + GDI)");
        });

        let ui_tx = GLOBAL_UI_TX
            .lock()
            .expect("GLOBAL_UI_TX poisoned")
            .clone()
            .expect("GLOBAL_UI_TX not initialized — init_global_ui was never called");

        // Channels for this bridge instance
        let (req_tx, req_rx) = unbounded::<Request>();
        let (edit_tx, _edit_rx) = unbounded::<PendingEdit>(); // TODO: wire up edit channel

        // Spawn worker thread
        let worker_dict = Arc::clone(&dict);
        let worker_config = Arc::clone(&config);
        let worker_ui_tx = ui_tx.clone();
        let worker_edit_tx = edit_tx.clone();
        let worker_handle = thread::Builder::new()
            .name("worker".into())
            .spawn(move || {
                let engine = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    EngineCore::new(worker_dict, user_dict, worker_config, bigram)
                }));
                match engine {
                    Ok(mut engine) => {
                        worker_loop(&mut engine, &req_rx, &worker_ui_tx, &worker_edit_tx);
                    }
                    Err(e) => {
                        let msg = if let Some(s) = e.downcast_ref::<String>() {
                            s.clone()
                        } else if let Some(s) = e.downcast_ref::<&str>() {
                            s.to_string()
                        } else {
                            "unknown panic".to_string()
                        };
                        tlog!("[tsf] WORKER PANIC during init: {msg}");
                    }
                }
            })
            .context("failed to spawn worker thread")?;

        // Config watcher thread
        let watcher_req_tx = req_tx.clone();
        let config_path = config_path();
        let watcher_handle = thread::Builder::new()
            .name("config-watcher".into())
            .spawn(move || {
                let mut last_reload = std::time::Instant::now();
                let mut watcher: RecommendedWatcher = Watcher::new(
                    move |event: Result<notify::Event, notify::Error>| {
                        if let Ok(event) = event {
                            let is_modify =
                                matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_));
                            if is_modify && last_reload.elapsed() >= Duration::from_millis(500) {
                                last_reload = std::time::Instant::now();
                                thread::sleep(Duration::from_millis(100));
                                let _ = watcher_req_tx.send(Request::ConfigReload);
                            }
                        }
                    },
                    notify::Config::default(),
                )
                .expect("failed to create config watcher");
                if let Err(e) = watcher.watch(&config_path, RecursiveMode::NonRecursive) {
                    tlog!("[tsf] WARN: Failed to watch config: {e}");
                    return;
                }
                // Park thread until process exit — no CPU usage.
                // The config watcher callback keeps the watcher alive.
                std::thread::park();
            })
            .context("failed to spawn config-watcher thread")?;

        // Action forwarder thread (reads from global action channel)
        let forwarder_req_tx = req_tx.clone();
        let action_rx = GLOBAL_ACTION_RX
            .lock()
            .expect("GLOBAL_ACTION_RX poisoned")
            .clone()
            .expect("GLOBAL_ACTION_RX not initialized — init_global_ui was never called");
        let forwarder_handle = thread::Builder::new()
            .name("action-forwarder".into())
            .spawn(move || {
                for action in action_rx {
                    match action {
                        UiAction::SelectCandidate(idx) => {
                            let (resp_tx, _) = crate::oneshot::channel();
                            let _ = forwarder_req_tx.send(Request::SelectCandidate {
                                index: idx,
                                response: resp_tx,
                            });
                        }
                        UiAction::NextPage | UiAction::PrevPage => {}
                    }
                }
            })
            .context("failed to spawn action-forwarder thread")?;

        Ok(Self {
            req_tx,
            edit_tx,
            _worker: worker_handle,
            _forwarder: forwarder_handle,
            _config_watcher: watcher_handle,
        })
    }

    pub fn req_tx(&self) -> &Sender<Request> {
        &self.req_tx
    }

    /// Queue an edit session request to be processed by TSF.
    /// TODO: implement actual edit forwarding — currently a no-op.
    pub fn request_edit(&self, _context: &windows::Win32::UI::TextServices::ITfContext) {
        // let _ = self.edit_tx.send(PendingEdit::CommitText(text));
    }

    /// Graceful shutdown: signal worker to flush and exit.
    pub fn shutdown(&mut self) {
        let _ = self.req_tx.send(Request::Shutdown);
    }
}

fn worker_loop(
    engine: &mut EngineCore,
    rx: &Receiver<Request>,
    ui_tx: &Sender<UiUpdate>,
    _edit_tx: &Sender<PendingEdit>,
) {
    tlog!("[tsf] worker_loop started, waiting for requests...");
    for req in rx {
        match req {
            Request::KeyPress {
                vk,
                modifiers,
                response,
            } => {
                tlog!("[tsf] worker: KeyPress vk=0x{vk:x}");
                let key = KeyEvent {
                    vk,
                    ch: char_from_vk(vk, modifiers),
                    modifiers,
                };
                let action = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    engine.handle_key(key)
                })) {
                    Ok(a) => a,
                    Err(e) => {
                        let msg = e
                            .downcast_ref::<String>()
                            .map(|s| s.as_str())
                            .or_else(|| e.downcast_ref::<&str>().copied())
                            .unwrap_or("unknown");
                        tlog!("[tsf] worker PANIC in handle_key: {msg}");
                        response.send(Response::Passthrough);
                        continue;
                    }
                };
                tlog!("[tsf] worker: handle_key action={:?}", action);
                let update = build_ui_update(engine);
                let _ = ui_tx.send(update);
                let resp = match action {
                    engine_core::Action::Passthrough => Response::Passthrough,
                    engine_core::Action::Commit(text) => Response::Committed(text),
                    _ => Response::Consumed,
                };
                tlog!("[tsf] worker: sending response");
                response.send(resp);
            }
            Request::SelectCandidate { index, response } => {
                let action = engine.select_candidate(index);
                let _ = ui_tx.send(build_ui_update(engine));
                let resp = match action {
                    engine_core::Action::Commit(text) => Response::Committed(text),
                    _ => Response::Consumed,
                };
                response.send(resp);
            }
            Request::Reset => engine.reset(),
            Request::ConfigReload => {
                engine.update_config(Arc::new(Config::load()));
            }
            Request::ToggleMode => {
                engine.toggle_mode();
                let _ = ui_tx.send(build_ui_update(engine));
            }
            Request::Shutdown => {
                engine.flush_user_dict();
                break;
            }
        }
    }
}

fn build_ui_update(engine: &EngineCore) -> UiUpdate {
    let candidates = engine.candidates();
    let ui_candidates: Vec<ui_crate::UiCandidate> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| ui_crate::UiCandidate {
            text: c.text.clone(),
            pinyin: c.pinyin.join(" "),
            index: i,
        })
        .collect();
    UiUpdate {
        candidates: ui_candidates,
        pinyin: engine.pinyin_buffer().raw_input().to_string(),
        cursor_position: engine.pinyin_buffer().cursor_position(),
        position: (0, 0),
        visible: !engine.pinyin_buffer().is_empty(),
    }
}

fn config_path() -> std::path::PathBuf {
    let mut path = if let Some(base) = directories::BaseDirs::new() {
        base.config_dir().to_path_buf()
    } else {
        std::path::PathBuf::from(".")
    };
    path.push("pyrust");
    path.push("config.toml");
    path
}

fn char_from_vk(vk: u32, modifiers: engine_core::Modifiers) -> Option<char> {
    match vk {
        0x20 => Some(' '),
        0x30..=0x39 => Some(if modifiers.shift {
            ")!@#$%^&*(".chars().nth((vk - 0x30) as usize)?
        } else {
            char::from_u32(vk)?
        }),
        0x41..=0x5A => Some(if modifiers.shift {
            char::from_u32(vk)?
        } else {
            char::from_u32(vk + 0x20)?
        }),
        _ => None,
    }
}
