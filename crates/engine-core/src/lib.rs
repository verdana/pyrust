pub mod bigram;
pub mod fuzzy_pinyin;
pub mod pinyin;
pub mod state_machine;
pub mod sorter;

use std::sync::Arc;
use dict::{user_dict::UserDict, Candidate, DictSource};
use yas_config::{Config, InputMode};

pub use bigram::BigramModel;
pub use fuzzy_pinyin::FuzzyPinyin;
pub use pinyin::{PinyinBuffer, PinyinSyllabler};
pub use state_machine::StateMachine;

/// The core pinyin input engine state.
pub struct EngineCore {
    pinyin_buffer: PinyinBuffer,
    syllabler: PinyinSyllabler,
    dict: Arc<dyn DictSource>,
    user_dict: UserDict,
    config: Arc<Config>,
    candidates: Vec<Candidate>,
    state: StateMachine,
    zh_mode: bool,
    bigram: BigramModel,
    fuzzy: FuzzyPinyin,
    /// The last committed word, used for bigram context boost.
    last_committed: Option<String>,
}

#[derive(Debug, Clone)]
pub struct KeyEvent {
    pub vk: u32,
    pub ch: Option<char>,
    pub modifiers: Modifiers,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Passthrough,
    Commit(String),
    UpdateCandidates,
    Noop,
    ToggleMode,
}

impl EngineCore {
    pub fn new(
        dict: Arc<dyn DictSource>,
        user_dict: UserDict,
        config: Arc<Config>,
        bigram: BigramModel,
    ) -> Self {
        Self {
            pinyin_buffer: PinyinBuffer::new(),
            syllabler: PinyinSyllabler::new(),
            dict,
            user_dict,
            config,
            candidates: Vec::new(),
            state: state_machine::State::Idle.into(),
            zh_mode: true,
            bigram,
            fuzzy: FuzzyPinyin::new(),
            last_committed: None,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if !self.zh_mode {
            return Action::Passthrough;
        }

        let state = self.state.current();
        match state {
            state_machine::State::Idle => self.handle_idle(key),
            state_machine::State::Pending => self.handle_pending(key),
            state_machine::State::Composing => self.handle_composing(key),
        }
    }

    pub fn flush_user_dict(&mut self) {
        if self.user_dict.needs_flush() {
            if let Err(e) = self.user_dict.flush() {
                log::warn!("flush_user_dict failed: {e}");
            }
        }
    }

    fn handle_idle(&mut self, key: KeyEvent) -> Action {
        match key.ch {
            Some(c @ 'a'..='z') => {
                self.pinyin_buffer.insert_at_cursor(c);
                self.update_candidates();
                self.state.transition_to(state_machine::State::Pending);
                Action::UpdateCandidates
            }
            _ => Action::Passthrough,
        }
    }

    fn handle_pending(&mut self, key: KeyEvent) -> Action {
        match key.ch {
            Some(c @ 'a'..='z') => {
                self.pinyin_buffer.insert_at_cursor(c);
                self.update_candidates();
                Action::UpdateCandidates
            }
            Some(' ') => self.commit_current(0, 1),
            Some(n @ '1'..='9') => {
                let idx = (n as usize) - ('1' as usize);
                self.commit_current(idx, 2)
            }
            _ => self.handle_vk_key(key.vk),
        }
    }

    fn handle_vk_key(&mut self, vk: u32) -> Action {
        match vk {
            0x0D => {
                // Enter: commit first candidate or clear buffer
                if self.candidates.is_empty() {
                    self.pinyin_buffer.clear();
                    self.candidates.clear();
                    self.state.transition_to(state_machine::State::Idle);
                    Action::UpdateCandidates
                } else {
                    self.commit_current(0, 1)
                }
            }
            0x08 => {
                self.pinyin_buffer.delete_before_cursor();
                if self.pinyin_buffer.is_empty() {
                    self.candidates.clear();
                    self.state.transition_to(state_machine::State::Idle);
                    Action::UpdateCandidates
                } else {
                    self.update_candidates();
                    Action::UpdateCandidates
                }
            }
            0x1B => {
                self.pinyin_buffer.clear();
                self.candidates.clear();
                self.state.transition_to(state_machine::State::Idle);
                Action::UpdateCandidates
            }
            0x25..=0x28 => {
                if vk == 0x25 {
                    self.pinyin_buffer.move_cursor(-1);
                } else if vk == 0x27 {
                    self.pinyin_buffer.move_cursor(1);
                }
                self.update_candidates();
                Action::UpdateCandidates
            }
            _ => Action::Passthrough,
        }
    }

    fn commit_current(&mut self, index: usize, weight: u32) -> Action {
        if let Some(candidate) = self.candidates.get(index) {
            let text = candidate.text.clone();
            let pinyin = candidate.pinyin.clone();
            self.last_committed = Some(text.clone());
            self.user_dict.learn(&text, pinyin, weight);
            self.pinyin_buffer.clear();
            self.candidates.clear();
            self.state.transition_to(state_machine::State::Idle);
            Action::Commit(text)
        } else {
            Action::Noop
        }
    }

    fn handle_composing(&mut self, key: KeyEvent) -> Action {
        match key.ch {
            Some(c @ 'a'..='z') => {
                let committed = self.candidates.first().map(|candidate| {
                    let text = candidate.text.clone();
                    let pinyin = candidate.pinyin.clone();
                    self.last_committed = Some(text.clone());
                    self.user_dict.learn(&text, pinyin, 1);
                    text
                });
                self.pinyin_buffer.clear();
                self.candidates.clear();
                self.pinyin_buffer.insert_at_cursor(c);
                self.update_candidates();
                self.state.transition_to(state_machine::State::Pending);
                if let Some(text) = committed {
                    Action::Commit(text)
                } else {
                    Action::UpdateCandidates
                }
            }
            Some(' ') => {
                if let Some(best) = self.candidates.first() {
                    let text = best.text.clone();
                    let pinyin = best.pinyin.clone();
                    self.last_committed = Some(text.clone());
                    self.user_dict.learn(&text, pinyin, 1);
                    self.update_candidates();
                    Action::Commit(text)
                } else {
                    Action::Noop
                }
            }
            _ => Action::Passthrough,
        }
    }

    pub fn select_candidate(&mut self, index: usize) -> Action {
        self.commit_current(index, 2)
    }

    pub fn reset(&mut self) {
        self.pinyin_buffer.clear();
        self.candidates.clear();
        self.last_committed = None;
        self.state.transition_to(state_machine::State::Idle);
    }

    pub fn toggle_mode(&mut self) {
        self.zh_mode = !self.zh_mode;
        if !self.zh_mode {
            self.reset();
        }
    }

    /// Apply a (re)loaded config. Called on hot-reload.
    pub fn update_config(&mut self, config: Arc<Config>) {
        self.zh_mode = config.general.mode == InputMode::Zh;
        self.config = config;
    }

    pub fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }

    pub fn pinyin_buffer(&self) -> &PinyinBuffer {
        &self.pinyin_buffer
    }

    pub fn is_zh_mode(&self) -> bool {
        self.zh_mode
    }

    fn update_candidates(&mut self) {
        let input = self.pinyin_buffer.raw_input();
        if input.is_empty() {
            self.candidates.clear();
            return;
        }

        let syllables = self.syllabler.best_segmentation(input);
        let pinyin_key = self.syllabler.syllables_to_key(&syllables);

        // Collect entries from exact match and fuzzy variants
        let mut all_entries: Vec<dict::DictEntry> = Vec::new();

        // Exact match (always query)
        if let Some(entries) = self.dict.lookup(&pinyin_key) {
            all_entries.extend(entries);
        }
        if let Some(entries) = self.user_dict.lookup(&pinyin_key) {
            all_entries.extend(entries.iter().cloned());
        }

        // Fuzzy variants (when enabled)
        if self.config.engine.fuzzy_pinyin {
            let variants = self.fuzzy.key_variants(&pinyin_key);
            for variant in variants {
                if variant == pinyin_key {
                    continue; // already queried
                }
                if let Some(entries) = self.dict.lookup(&variant) {
                    all_entries.extend(entries);
                }
                if let Some(entries) = self.user_dict.lookup(&variant) {
                    all_entries.extend(entries.iter().cloned());
                }
            }
        }

        let now = now();
        let bigram_enabled = self.config.engine.enable_bigram;

        // Merge: dedup by text (keep higher score)
        let mut result: Vec<Candidate> = all_entries
            .into_iter()
            .map(|e| {
                let mut score = e.frequency as f64 + e.weight as f64;
                // Freshness boost: +30% for user entries updated within 7 days
                if e.is_user && e.updated_at > 0 && now - e.updated_at < 604_800 {
                    score *= 1.3;
                }
                // Bigram boost: boost candidates that commonly follow the last committed word
                if bigram_enabled {
                    if let Some(ref prev) = self.last_committed {
                        let boost = self.bigram.get_boost(prev, &e.text);
                        if boost > 1.0 {
                            score *= boost;
                        }
                    }
                }
                Candidate {
                    text: e.text.clone(),
                    pinyin: e.pinyin.clone(),
                    score,
                }
            })
            .fold(Vec::new(), |mut acc, c| {
                let text = c.text.clone();
                if let Some(existing) = acc.iter_mut().find(|e: &&mut Candidate| e.text == text) {
                    if c.score > existing.score {
                        *existing = c;
                    }
                } else {
                    acc.push(c);
                }
                acc
            });

        sorter::sort_candidates(&mut result);
        self.candidates = result;
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
