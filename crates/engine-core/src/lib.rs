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
    /// Track quote pairing state (true = next quote is opening).
    last_quote_was_open: bool,
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
    CommitRaw(String),
    UpdateCandidates,
    UpdatePreedit { text: String, cursor: usize },
    ClearPreedit,
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
            last_quote_was_open: false,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if !self.zh_mode {
            return Action::Passthrough;
        }

        // Punctuation: commit pending pinyin (if any) + punctuation character.
        if let Some(punct) = self.handle_punctuation(key.vk, key.modifiers.shift) {
            if self.pinyin_buffer.is_empty() {
                self.state.transition_to(state_machine::State::Idle);
                return Action::Commit(punct.to_string());
            }
            // Composing state: commit candidate text + punctuation together.
            if let Some(candidate) = self.candidates.first() {
                let text = format!("{}{}", candidate.text, punct);
                let pinyin = candidate.pinyin.clone();
                self.last_committed = Some(candidate.text.clone());
                self.user_dict.learn(&candidate.text, pinyin, 1);
                self.pinyin_buffer.clear();
                self.candidates.clear();
                self.state.transition_to(state_machine::State::Idle);
                return Action::Commit(text);
            }
            // Pending (no candidates yet): commit raw pinyin + punctuation.
            let raw = self.pinyin_buffer.raw_input().to_string();
            let text = format!("{}{}", raw, punct);
            self.pinyin_buffer.clear();
            self.candidates.clear();
            self.state.transition_to(state_machine::State::Idle);
            return Action::Commit(text);
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
                // Enter: commit raw pinyin text (e.g., "nihao" → "nihao")
                let raw = self.pinyin_buffer.raw_input().to_string();
                if raw.is_empty() {
                    Action::Passthrough
                } else {
                    self.pinyin_buffer.clear();
                    self.candidates.clear();
                    self.state.transition_to(state_machine::State::Idle);
                    Action::CommitRaw(raw)
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

    /// Returns the Chinese punctuation character for the given VK code, or None
    /// if the key is not a punctuation key.
    fn handle_punctuation(&mut self, vk: u32, shift: bool) -> Option<char> {
        match vk {
            0xBC if shift => Some('《'), // <
            0xBC => Some('，'),           // ,
            0xBE if shift => Some('》'), // >
            0xBE => Some('。'),           // .
            0xBA => Some('；'),           // ;
            0xBF => Some('？'),           // ?
            0x31 if shift => Some('！'), // Shift+1 = !
            0xBB => Some('＝'),           // =
            0xBD => Some('\u{2014}'),       // - → — (em dash)
            0xDC => Some('、'),           // \
            0xDE => {
                // ' or " — paired quotes
                let ch = if shift {
                    let ch = if self.last_quote_was_open { '\u{201D}' } else { '\u{201C}' };
                    self.last_quote_was_open = !self.last_quote_was_open;
                    ch
                } else {
                    let ch = if self.last_quote_was_open { '\u{2019}' } else { '\u{2018}' };
                    self.last_quote_was_open = !self.last_quote_was_open;
                    ch
                };
                Some(ch)
            }
            0xDB => Some(if shift { '【' } else { '（' }), // [ or {
            0xDD => Some(if shift { '】' } else { '）' }), // ] or }
            _ => None,
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

        let mut syllables = self.syllabler.best_segmentation(input);
        if syllables.is_empty() {
            syllables = self.syllabler.greedy_segmentation(input);
        }
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

        // Fallback: for multi-syllable input when no exact match found.
        if syllables.len() > 1 && all_entries.is_empty() {
            // Strategy 1: single-char composition — combine top candidate from each
            // syllable to form an N-char result matching input length.
            // e.g., "ju ju ju ju" → "据据据据" (4 chars for 4 syllables)
            let mut chars: Vec<String> = Vec::with_capacity(syllables.len());
            for syl in &syllables {
                if let Some(entries) = self.dict.lookup(syl) {
                    if let Some(first) = entries.first() {
                        chars.push(first.text.clone());
                        continue;
                    }
                }
                chars.clear();
                break;
            }
            if chars.len() == syllables.len() {
                all_entries.push(dict::DictEntry {
                    text: chars.join(""),
                    pinyin: syllables.clone(),
                    frequency: 100,
                    weight: 0,
                    is_user: false,
                    updated_at: 0,
                });
            }

            // Strategy 2: shorter n-gram windows (only if single-char failed).
            // e.g., "ju ju ju" → try "ju ju" windows
            if all_entries.is_empty() {
                for n in (1..syllables.len()).rev() {
                    for window in syllables.windows(n) {
                        let key = self.syllabler.syllables_to_key(window);
                        if let Some(entries) = self.dict.lookup(&key) {
                            all_entries.extend(entries);
                        }
                        if let Some(entries) = self.user_dict.lookup(&key) {
                            all_entries.extend(entries.iter().cloned());
                        }
                    }
                    if !all_entries.is_empty() {
                        break;
                    }
                }
            }
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
