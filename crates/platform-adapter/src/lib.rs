use engine_core::{Action, Modifiers};

pub trait ImeBackend: Send {
    // --- Existing (required) ---
    fn initialize(&mut self) -> Result<(), ImeError>;
    fn handle_key_event(&mut self, vk: u32, modifiers: Modifiers) -> Action;
    fn commit(&self, text: &str);
    fn set_candidate_position(&self, x: i32, y: i32);

    // --- New: Composition / Preedit (Phase 2) ---
    fn update_preedit(&self, _text: &str, _cursor_pos: usize) {}
    fn clear_preedit(&self) {}

    // --- New: Activation lifecycle ---
    fn activate(&mut self) -> Result<(), ImeError> { Ok(()) }
    fn deactivate(&mut self) -> Result<(), ImeError> { Ok(()) }
    fn is_active(&self) -> bool { false }

    // --- New: Focus / Context ---
    fn on_focus_change(&mut self, _gained: bool) {}
    fn is_password_field(&self) -> bool { false }
}

#[derive(Debug, thiserror::Error)]
pub enum ImeError {
    #[error("IME initialization failed: {0}")]
    InitializationFailed(String),
    #[error("IME registration failed: {0}")]
    RegistrationFailed(String),
    #[error("Platform error: {0}")]
    PlatformError(String),
}

// --- Windows stub ---
#[cfg(windows)]
pub mod win;

#[cfg(windows)]
pub type PlatformBackend = win::WinAdapter;

// --- macOS stub ---
#[cfg(target_os = "macos")]
pub mod mac;

#[cfg(target_os = "macos")]
pub type PlatformBackend = mac::MacAdapter;

// --- Fallback (development/testing) ---
#[cfg(not(any(windows, target_os = "macos")))]
pub mod dev;

#[cfg(not(any(windows, target_os = "macos")))]
pub type PlatformBackend = dev::DevAdapter;
