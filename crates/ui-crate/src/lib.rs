pub mod candidate_window;
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

/// Actions the UI thread sends back to the worker thread.
#[derive(Debug, Clone)]
pub enum UiAction {
    SelectCandidate(usize),
    NextPage,
    PrevPage,
}
