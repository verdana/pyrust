pub mod trie;
pub mod pinyin_table;
pub mod base_dict;
pub mod user_dict;
pub mod dat_dict;

/// Trait for dictionary lookup sources.
pub trait DictSource: Send + Sync {
    fn lookup(&self, pinyin: &str) -> Option<Vec<DictEntry>>;
}

#[derive(Debug, Clone)]
pub struct DictEntry {
    pub text: String,
    pub pinyin: Vec<String>,
    pub frequency: u32,
    pub weight: i32,
    pub is_user: bool,
    pub updated_at: u64,
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub text: String,
    pub pinyin: Vec<String>,
    pub score: f64,
}
