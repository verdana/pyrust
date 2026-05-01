use crate::{UiCandidate, UiUpdate};
use yas_config::UiConfig;

/// Candidate window state — rendered by egui on the UI thread.
pub struct CandidateWindow {
    pub visible: bool,
    pub candidates: Vec<UiCandidate>,
    pub pinyin: String,
    pub page: usize,
    pub position: (i32, i32),
    pub hovered_index: Option<usize>,
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
            hovered_index: None,
            config,
        }
    }

    pub fn apply_update(&mut self, update: UiUpdate) {
        self.visible = update.visible;
        self.candidates = update.candidates;
        self.pinyin = update.pinyin;
        self.position = update.position;
        self.page = 0;
        self.hovered_index = None;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.candidates.clear();
        self.pinyin.clear();
    }

    pub fn set_hovered(&mut self, index: Option<usize>) {
        self.hovered_index = index;
    }

    pub fn clear_hover(&mut self) {
        self.hovered_index = None;
    }

    pub fn per_page(&self) -> usize {
        self.config.max_candidates.max(1)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(max: usize) -> UiConfig {
        UiConfig {
            max_candidates: max,
            ..UiConfig::default()
        }
    }

    fn make_candidate(text: &str, idx: usize) -> UiCandidate {
        UiCandidate { text: text.into(), pinyin: String::new(), index: idx }
    }

    fn make_update(candidates: Vec<UiCandidate>, visible: bool) -> UiUpdate {
        UiUpdate {
            candidates,
            pinyin: "test".into(),
            cursor_position: 0,
            position: (100, 200),
            visible,
        }
    }

    #[test]
    fn new_is_empty() {
        let w = CandidateWindow::new(UiConfig::default());
        assert!(!w.visible);
        assert!(w.candidates.is_empty());
        assert_eq!(w.page, 0);
        assert_eq!(w.hovered_index, None);
    }

    #[test]
    fn apply_update_sets_fields() {
        let mut w = CandidateWindow::new(make_config(5));
        let candidates = vec![make_candidate("你好", 0)];
        w.page = 2;
        w.hovered_index = Some(0);
        w.apply_update(make_update(candidates.clone(), true));
        assert!(w.visible);
        assert_eq!(w.candidates.len(), 1);
        assert_eq!(w.pinyin, "test");
        assert_eq!(w.position, (100, 200));
        assert_eq!(w.page, 0);
        assert_eq!(w.hovered_index, None);
    }

    #[test]
    fn apply_update_resets_page() {
        let mut w = CandidateWindow::new(make_config(5));
        let candidates: Vec<_> = (0..10).map(|i| make_candidate("x", i)).collect();
        w.apply_update(make_update(candidates.clone(), true));
        w.next_page();
        assert_eq!(w.page, 1);
        w.apply_update(make_update(candidates, true));
        assert_eq!(w.page, 0);
    }

    #[test]
    fn apply_update_resets_hover() {
        let mut w = CandidateWindow::new(make_config(5));
        w.hovered_index = Some(2);
        let candidates = vec![make_candidate("你好", 0)];
        w.apply_update(make_update(candidates, true));
        assert_eq!(w.hovered_index, None);
    }

    #[test]
    fn page_candidates_empty() {
        let w = CandidateWindow::new(make_config(5));
        assert!(w.page_candidates().is_empty());
    }

    #[test]
    fn page_candidates_single_page() {
        let mut w = CandidateWindow::new(make_config(5));
        let candidates: Vec<_> = (0..3).map(|i| make_candidate("x", i)).collect();
        w.apply_update(make_update(candidates, true));
        assert_eq!(w.page_candidates().len(), 3);
    }

    #[test]
    fn page_candidates_multi_page_first() {
        let mut w = CandidateWindow::new(make_config(5));
        let candidates: Vec<_> = (0..10).map(|i| make_candidate("x", i)).collect();
        w.apply_update(make_update(candidates, true));
        assert_eq!(w.page_candidates().len(), 5);
    }

    #[test]
    fn page_candidates_multi_page_second() {
        let mut w = CandidateWindow::new(make_config(5));
        let candidates: Vec<_> = (0..10).map(|i| make_candidate("x", i)).collect();
        w.apply_update(make_update(candidates, true));
        w.next_page();
        assert_eq!(w.page_candidates().len(), 5);
    }

    #[test]
    fn total_pages_empty() {
        let w = CandidateWindow::new(make_config(5));
        assert_eq!(w.total_pages(), 0);
    }

    #[test]
    fn total_pages_exact() {
        let mut w = CandidateWindow::new(make_config(5));
        let candidates: Vec<_> = (0..10).map(|i| make_candidate("x", i)).collect();
        w.apply_update(make_update(candidates, true));
        assert_eq!(w.total_pages(), 2);
    }

    #[test]
    fn total_pages_partial() {
        let mut w = CandidateWindow::new(make_config(5));
        let candidates: Vec<_> = (0..11).map(|i| make_candidate("x", i)).collect();
        w.apply_update(make_update(candidates, true));
        assert_eq!(w.total_pages(), 3);
    }

    #[test]
    fn next_page_at_end_stays() {
        let mut w = CandidateWindow::new(make_config(5));
        let candidates: Vec<_> = (0..10).map(|i| make_candidate("x", i)).collect();
        w.apply_update(make_update(candidates, true));
        w.next_page();
        w.next_page();
        assert_eq!(w.page, 1);
    }

    #[test]
    fn prev_page_at_start_stays() {
        let mut w = CandidateWindow::new(make_config(5));
        let candidates: Vec<_> = (0..10).map(|i| make_candidate("x", i)).collect();
        w.apply_update(make_update(candidates, true));
        w.prev_page();
        assert_eq!(w.page, 0);
    }

    #[test]
    fn next_prev_roundtrip() {
        let mut w = CandidateWindow::new(make_config(5));
        let candidates: Vec<_> = (0..10).map(|i| make_candidate("x", i)).collect();
        w.apply_update(make_update(candidates, true));
        w.next_page();
        assert_eq!(w.page, 1);
        w.prev_page();
        assert_eq!(w.page, 0);
    }

    #[test]
    fn set_hovered_works() {
        let mut w = CandidateWindow::new(make_config(5));
        w.set_hovered(Some(2));
        assert_eq!(w.hovered_index, Some(2));
    }

    #[test]
    fn clear_hover_works() {
        let mut w = CandidateWindow::new(make_config(5));
        w.set_hovered(Some(0));
        w.clear_hover();
        assert_eq!(w.hovered_index, None);
    }

    #[test]
    fn max_candidates_zero_treated_as_one() {
        let mut w = CandidateWindow::new(make_config(0));
        let candidates: Vec<_> = (0..3).map(|i| make_candidate("x", i)).collect();
        w.apply_update(make_update(candidates, true));
        // max_candidates=0 is guarded by max(1), so per_page=1
        assert_eq!(w.page_candidates().len(), 1);
        assert_eq!(w.total_pages(), 3);
    }
}
