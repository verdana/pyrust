pub mod candidate_window;
pub mod renderer;
pub mod theme;
pub mod window;

/// Message sent from worker thread to UI thread (fire-and-forget).
#[derive(Debug, Clone)]
pub struct UiUpdate {
    pub candidates: Vec<UiCandidate>,
    pub pinyin: String,
    pub cursor_position: usize,
    pub position: (i32, i32),
    pub visible: bool,
}

#[derive(Debug, Clone)]
pub struct UiCandidate {
    pub text: String,
    pub pinyin: String,
    pub index: usize,
}

pub enum UiCommand {
    Show(UiUpdate),
    Hide,
    Quit,
}
