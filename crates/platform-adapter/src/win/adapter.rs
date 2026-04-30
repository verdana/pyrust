use crate::{ImeBackend, ImeError};
use engine_core::{Action, Modifiers};

pub struct WinAdapter;

impl ImeBackend for WinAdapter {
    fn initialize(&mut self) -> Result<(), ImeError> {
        Ok(())
    }

    fn handle_key_event(&mut self, _vk: u32, _modifiers: Modifiers) -> Action {
        Action::Passthrough
    }

    fn commit(&self, _text: &str) {}

    fn set_candidate_position(&self, _x: i32, _y: i32) {}
}
