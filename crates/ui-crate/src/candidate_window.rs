use crate::{UiCandidate, UiUpdate};
use yas_config::UiConfig;

/// Candidate window state — rendered by egui on the UI thread.
pub struct CandidateWindow {
    pub visible: bool,
    pub candidates: Vec<UiCandidate>,
    pub pinyin: String,
    pub page: usize,
    pub position: (i32, i32),
    config: UiConfig,
}

impl CandidateWindow {
    pub fn new(config: UiConfig) -> Self {
        Self {
            visible: false,
            candidates: Vec::new(),
            pinyin: String::new(),
            page: 0,
            position: (0, 0),
            config,
        }
    }

    pub fn apply_update(&mut self, update: UiUpdate) {
        self.visible = update.visible;
        self.candidates = update.candidates;
        self.pinyin = update.pinyin;
        self.position = update.position;
        self.page = 0;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.candidates.clear();
        self.pinyin.clear();
    }

    /// Total pages available (0 if no candidates).
    pub fn total_pages(&self) -> usize {
        if self.candidates.is_empty() {
            return 0;
        }
        let per_page = self.config.max_candidates.max(1);
        (self.candidates.len() + per_page - 1) / per_page
    }

    /// Candidates on the current page.
    pub fn page_candidates(&self) -> &[UiCandidate] {
        let per_page = self.config.max_candidates.max(1);
        let start = self.page * per_page;
        let end = (start + per_page).min(self.candidates.len());
        if start >= self.candidates.len() {
            &[]
        } else {
            &self.candidates[start..end]
        }
    }

    pub fn next_page(&mut self) {
        let max = self.total_pages().saturating_sub(1);
        if self.page < max {
            self.page += 1;
        }
    }

    pub fn prev_page(&mut self) {
        if self.page > 0 {
            self.page -= 1;
        }
    }
}
