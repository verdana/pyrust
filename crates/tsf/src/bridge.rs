//! Thread and channel bridge between TSF COM callbacks and the engine worker thread.
//!
//! Reuses the same architecture as `pyrust/src/main.rs`:
//! main (TSF) → worker → UI
//!                  ← action-forwarder

use std::sync::Arc;
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
use ui_crate::UiAction;
use yas_config::Config;

use crate::{PendingEdit, Request, Response};

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
    /// Does NOT start the UI thread — creating an egui window from inside the
    /// TSF COM callback causes COM re-entrancy deadlock → Explorer crash.
    ///
    /// The UI thread can be started later via `start_ui()` when it's safe to
    /// create windows (e.g. after Activate returns).
    pub fn initialize_engine() -> Result<Self> {
        log::info!("[tsf] Initializing bridge (engine only, no UI)...");

        let config = Arc::new(Config::load());

        // Load dictionaries
        let dict_path = &config.dict.base_dict_path;
        let dict: Arc<dyn DictSource> = if let Some(mmap_dict) = MmapDict::open(dict_path) {
            log::info!(
                "Loaded {} entries from mmap dictionary '{}'",
                mmap_dict.entry_count(),
                dict_path
            );
            Arc::new(mmap_dict)
        } else {
            let mut base_dict = BaseDict::new();
            if let Err(e) = base_dict.load_from_file(dict_path) {
                log::warn!("Failed to load base dictionary '{}': {e}", dict_path);
            }
            Arc::new(base_dict)
        };

        let user_dict = UserDict::open(&config.dict.user_dict_path);
        let bigram = BigramModel::load(&config.dict.bigram_data_path);

        // Channels
        let (req_tx, req_rx) = unbounded::<Request>();
        let (ui_tx, _ui_rx) = unbounded::<ui_crate::UiUpdate>();
        let (edit_tx, _edit_rx) = unbounded::<PendingEdit>();
        let (_action_tx, action_rx) = unbounded::<UiAction>();

        // Spawn worker thread
        let worker_dict = Arc::clone(&dict);
        let worker_config = Arc::clone(&config);
        let worker_ui_tx = ui_tx.clone();
        let worker_edit_tx = edit_tx.clone();
        let worker_handle = thread::Builder::new()
            .name("worker".into())
            .spawn(move || {
                let mut engine =
                    EngineCore::new(worker_dict, user_dict, worker_config, bigram);
                worker_loop(&mut engine, &req_rx, &worker_ui_tx, &worker_edit_tx);
            })
            .context("failed to spawn worker thread")?;

        // Action forwarder: UiAction → Request
        let forwarder_req_tx = req_tx.clone();
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
                            let is_modify = matches!(
                                event.kind,
                                EventKind::Modify(_) | EventKind::Create(_)
                            );
                            if is_modify && last_reload.elapsed() >= Duration::from_millis(500)
                            {
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
                    log::error!("Failed to watch config: {e}");
                    return;
                }
                loop {
                    thread::sleep(Duration::from_secs(u64::MAX));
                }
            })
            .context("failed to spawn config-watcher thread")?;

        Ok(Self {
            req_tx: req_tx.clone(),
            edit_tx: edit_tx.clone(),
            _worker: worker_handle,
            _forwarder: forwarder_handle,
            _config_watcher: watcher_handle,
        })
    }

    pub fn req_tx(&self) -> &Sender<Request> {
        &self.req_tx
    }

    /// Queue an edit session request to be processed by TSF.
    /// Called from ITfKeyEventSink after receiving a response from the worker.
    pub fn request_edit(&self, _context: &windows::Win32::UI::TextServices::ITfContext) {
        let _ = self.edit_tx.send(PendingEdit::CommitText(String::new()));
    }

    /// Graceful shutdown: signal worker to flush and exit.
    pub fn shutdown(&mut self) {
        let _ = self.req_tx.send(Request::Shutdown);
    }
}

fn worker_loop(
    engine: &mut EngineCore,
    rx: &Receiver<Request>,
    ui_tx: &Sender<ui_crate::UiUpdate>,
    _edit_tx: &Sender<PendingEdit>,
) {
    for req in rx {
        match req {
            Request::KeyPress {
                vk,
                modifiers,
                response,
            } => {
                let key = KeyEvent {
                    vk,
                    ch: char_from_vk(vk, modifiers),
                    modifiers,
                };
                let action = engine.handle_key(key);
                send_ui_update(engine, ui_tx);
                let resp = match action {
                    engine_core::Action::Passthrough => Response::Passthrough,
                    engine_core::Action::Commit(text) => Response::Committed(text),
                    _ => Response::Consumed,
                };
                response.send(resp);
            }
            Request::SelectCandidate { index, response } => {
                let action = engine.select_candidate(index);
                send_ui_update(engine, ui_tx);
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
                send_ui_update(engine, ui_tx);
            }
            Request::Shutdown => {
                engine.flush_user_dict();
                break;
            }
        }
    }
}

fn send_ui_update(engine: &EngineCore, ui_tx: &Sender<ui_crate::UiUpdate>) {
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
    let update = ui_crate::UiUpdate {
        candidates: ui_candidates,
        pinyin: engine.pinyin_buffer().raw_input().to_string(),
        cursor_position: engine.pinyin_buffer().cursor_position(),
        position: (0, 0),
        visible: !engine.pinyin_buffer().is_empty(),
    };
    if let Err(e) = ui_tx.send(update) {
        log::error!("Failed to send UI update: {e}");
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
