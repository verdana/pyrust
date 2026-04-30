mod logger;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use crossbeam_channel::{unbounded, Receiver, Sender};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use dict::base_dict::BaseDict;
use dict::dat_dict::MmapDict;
use dict::user_dict::UserDict;
use dict::DictSource;
use engine_core::{BigramModel, EngineCore, KeyEvent, Modifiers};
use yas_config::Config;

// Channel types for the 3-thread architecture:
//   platform thread → worker:   oneshot (Request + Response)
//   worker → ui thread:          unbounded mpsc (fire-and-forget)

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

pub enum Response {
    Consumed,
    Passthrough,
}

/// Lock-free oneshot channel for synchronous handoff.
/// The platform thread blocks on `recv()` with a spin-loop (<1µs expected).
mod oneshot {
    use std::sync::{Arc, Mutex};

    pub struct Sender<T> {
        slot: Arc<Mutex<Option<T>>>,
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
                if let Some(value) = self.slot.lock().expect("oneshot recv: lock poisoned").take()
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

fn main() -> Result<()> {
    logger::init();

    log::info!("inputd v{}", env!("CARGO_PKG_VERSION"));

    let config = Arc::new(Config::load());

    // Load base dictionary — try mmap format first, then text format
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
        } else {
            log::info!(
                "Loaded {} entries from text dictionary '{}'",
                base_dict.entry_count(),
                dict_path
            );
        }
        Arc::new(base_dict)
    };

    // Open user dictionary
    let user_dict = UserDict::open(&config.dict.user_dict_path);

    // Channels
    let (req_tx, req_rx) = unbounded::<Request>();
    let (ui_tx, ui_rx) = unbounded::<ui_crate::UiUpdate>();

    // Start config hot-reload watcher
    let config_path = config_path();
    if let Err(e) = start_config_watcher(&config_path, &req_tx) {
        log::warn!("Failed to start config watcher: {e}");
    }

    // Spawn worker thread
    let worker_config = Arc::clone(&config);
    let worker_dict = Arc::clone(&dict);
    let worker_user_dict = user_dict;
    let bigram_path = &config.dict.bigram_data_path;
    let bigram = BigramModel::load(bigram_path);
    let worker_ui_tx = ui_tx.clone();
    let worker_handle = thread::Builder::new()
        .name("worker".into())
        .spawn(move || {
            let mut engine = EngineCore::new(worker_dict, worker_user_dict, worker_config, bigram);
            worker_loop(&mut engine, &req_rx, &worker_ui_tx);
        })
        .context("failed to spawn worker thread")?;

    // Spawn UI thread (stub for Phase 1)
    let ui_config = Arc::clone(&config);
    let ui_handle = thread::Builder::new()
        .name("ui".into())
        .spawn(move || {
            ui_loop(&ui_rx, ui_config);
        })
        .context("failed to spawn UI thread")?;

    // Platform thread (main thread)
    log::info!("inputd ready. Type 'quit' to exit.");
    dev_input_loop(&req_tx);

    // Shutdown
    let _ = req_tx.send(Request::Shutdown);
    worker_handle.join().map_err(|_| anyhow!("worker thread panic"))?;
    ui_handle.join().map_err(|_| anyhow!("UI thread panic"))?;
    log::info!("inputd shutdown complete");
    Ok(())
}

fn worker_loop(
    engine: &mut EngineCore,
    rx: &Receiver<Request>,
    ui_tx: &Sender<ui_crate::UiUpdate>,
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
                    _ => Response::Consumed,
                };
                response.send(resp);
            }
            Request::SelectCandidate { index, response } => {
                let _action = engine.select_candidate(index);
                send_ui_update(engine, ui_tx);
                response.send(Response::Consumed);
            }
            Request::Reset => {
                engine.reset();
            }
            Request::ConfigReload => {
                let new_config = Arc::new(Config::load());
                engine.update_config(new_config);
                log::info!("Configuration reloaded");
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

fn ui_loop(rx: &Receiver<ui_crate::UiUpdate>, config: Arc<Config>) {
    log::info!("UI thread starting...");
    let ui_config = yas_config::UiConfig {
        font_size: config.ui.font_size,
        font_family: config.ui.font_family.clone(),
        theme: config.ui.theme,
        max_candidates: config.ui.max_candidates,
        vertical: config.ui.vertical,
    };
    log::info!("UI config created, calling run_ui_window...");
    ui_crate::window::run_ui_window(ui_config, rx.clone());
    log::info!("UI thread exiting");
}

fn dev_input_loop(tx: &Sender<Request>) {
    use std::io::{self, BufRead, Write};

    let stdin = io::stdin();

    loop {
        print!("inputd> ");
        let _ = io::stdout().flush();

        let mut line = String::new();
        if stdin.lock().read_line(&mut line).is_err() || line.trim() == "quit" {
            break;
        }
        let line = line.trim().to_lowercase();

        if line == "reset" {
            let _ = tx.send(Request::Reset);
            continue;
        }

        if line == "zh" || line == "en" {
            let _ = tx.send(Request::ToggleMode);
            continue;
        }

        let send_key = |vk: u32| -> bool {
            let (resp_tx, resp_rx) = oneshot::channel();
            if tx
                .send(Request::KeyPress {
                    vk,
                    modifiers: Modifiers::default(),
                    response: resp_tx,
                })
                .is_err()
            {
                return false;
            }
            let _ = resp_rx.recv();
            true
        };

        // Reset engine before new input
        let _ = tx.send(Request::Reset);

        for ch in line.chars() {
            let vk = match ch {
                'a'..='z' => ch as u32 - 0x20,
                ' ' => 0x20,
                '1'..='9' => ch as u32,
                _ => continue,
            };
            if !send_key(vk) {
                return;
            }
        }

        // Brief pause for worker to process
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Get the config file path.
fn config_path() -> PathBuf {
    let mut path = if let Some(base) = directories::BaseDirs::new() {
        base.config_dir().to_path_buf()
    } else {
        PathBuf::from(".")
    };
    path.push("inputd");
    path.push("config.toml");
    path
}

/// Start a background file watcher for config changes.
/// Sends `Request::ConfigReload` when the config file changes.
fn start_config_watcher(path: &Path, req_tx: &Sender<Request>) -> Result<(), anyhow::Error> {
    log::info!("Watching config file: {}", path.display());
    let path = path.to_owned();
    let tx = req_tx.clone();

    thread::Builder::new()
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
                        if is_modify
                            && last_reload.elapsed() >= Duration::from_millis(500)
                        {
                            last_reload = std::time::Instant::now();
                            // Brief delay for file write to complete
                            thread::sleep(Duration::from_millis(100));
                            let _ = tx.send(Request::ConfigReload);
                        }
                    }
                },
                notify::Config::default(),
            )
            .expect("failed to create config watcher");

            if let Err(e) = watcher.watch(&path, RecursiveMode::NonRecursive) {
                log::error!("Failed to watch config file: {e}");
                return;
            }

            // Keep the thread alive (watcher is alive as long as this closure runs)
            loop {
                thread::sleep(Duration::from_secs(u64::MAX));
            }
        })
        .context("failed to spawn config-watcher thread")?;

    Ok(())
}

fn char_from_vk(vk: u32, modifiers: Modifiers) -> Option<char> {
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
