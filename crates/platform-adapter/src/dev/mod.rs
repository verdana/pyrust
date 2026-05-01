/// Dev backend — for development/testing on non-macOS/non-Windows platforms.
use crate::{ImeBackend, ImeError};
use engine_core::{Action, Modifiers};

pub struct DevAdapter;

impl ImeBackend for DevAdapter {
    fn initialize(&mut self) -> Result<(), ImeError> {
        eprintln!("[pyrust][dev] Initialized dev IME backend");
        Ok(())
    }

    fn handle_key_event(&mut self, vk: u32, modifiers: Modifiers) -> Action {
        eprintln!("[pyrust][dev] key: vk=0x{vk:02x}, shift={}, ctrl={}, alt={}",
            modifiers.shift as u8, modifiers.ctrl as u8, modifiers.alt as u8);
        Action::Passthrough
    }

    fn commit(&self, text: &str) {
        eprintln!("[pyrust][dev] commit: {text}");
    }

    fn set_candidate_position(&self, x: i32, y: i32) {
        eprintln!("[pyrust][dev] candidate position: ({x}, {y})");
    }
}
