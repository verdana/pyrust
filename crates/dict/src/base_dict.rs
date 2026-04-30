use std::collections::HashMap;
use std::io::{BufRead, BufReader};

use crate::{Candidate, DictEntry};

/// Base dictionary — in-memory for Phase 1, mmap-based DAT in Phase 3.
pub struct BaseDict {
    /// pinyin_string -> Vec<entries>
    entries: HashMap<String, Vec<DictEntry>>,
}

impl BaseDict {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Load from a simple text file format (one entry per line):
    /// `text pinyin1 pinyin2 ... frequency weight`
    pub fn load_from_file(&mut self, path: &str) -> Result<(), std::io::Error> {
        let file = std::fs::File::open(path)?;
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = line?.trim().to_string();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(entry) = parse_entry(&line) {
                let key = entry.pinyin.join(" ");
                self.entries.entry(key).or_default().push(entry);
            }
        }
        Ok(())
    }

    /// Lookup entries by pinyin string (space-separated, e.g. "shu1 ru4 fa3").
    pub fn lookup(&self, pinyin: &str) -> Option<&Vec<DictEntry>> {
        self.entries.get(pinyin)
    }

    /// Get all entries matching a prefix (for partial/incremental lookup).
    /// For Phase 1 this is a linear scan — will be replaced by Trie in Phase 3.
    pub fn prefix_lookup(&self, prefix: &str) -> Vec<&DictEntry> {
        self.entries
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .flat_map(|(_, v)| v)
            .collect()
    }

    /// Search by hanzi text (for fallback / confirmation).
    pub fn search_by_text(&self, text: &str) -> Vec<Candidate> {
        self.entries
            .values()
            .flatten()
            .filter(|e| e.text.contains(text))
            .map(|e| Candidate {
                text: e.text.clone(),
                pinyin: e.pinyin.clone(),
                score: e.frequency as f64 + e.weight as f64,
            })
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entry_count(&self) -> usize {
        self.entries.values().map(|v| v.len()).sum()
    }
}

impl crate::DictSource for BaseDict {
    fn lookup(&self, pinyin: &str) -> Option<Vec<crate::DictEntry>> {
        self.entries.get(pinyin).cloned()
    }
}

fn parse_entry(line: &str) -> Option<DictEntry> {
    let mut parts = line.split_whitespace();
    let text = parts.next()?;
    let mut pinyin = Vec::new();
    for p in parts.by_ref() {
        if let Ok(freq) = p.parse::<u32>() {
            // Once we hit a number, it's frequency, rest is weight
            let weight = parts.next().and_then(|w| w.parse::<i32>().ok()).unwrap_or(0);
            return Some(DictEntry {
                text: text.to_string(),
                pinyin,
                frequency: freq,
                weight,
                is_user: false,
                updated_at: 0,
            });
        }
        pinyin.push(p.to_string());
    }
    // Entry without frequency defaults
    Some(DictEntry {
        text: text.to_string(),
        pinyin,
        frequency: 1,
        weight: 0,
        is_user: false,
        updated_at: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_entry() {
        let e = parse_entry("输入法 shu1 ru4 fa3 50 10").unwrap();
        assert_eq!(e.text, "输入法");
        assert_eq!(e.pinyin, vec!["shu1", "ru4", "fa3"]);
        assert_eq!(e.frequency, 50);
        assert_eq!(e.weight, 10);
    }

    #[test]
    fn test_parse_entry_no_weight() {
        let e = parse_entry("测试 ce4 shi4 100").unwrap();
        assert_eq!(e.frequency, 100);
        assert_eq!(e.weight, 0);
    }

    #[test]
    fn test_parse_entry_no_freq() {
        let e = parse_entry("测试 ce4 shi4").unwrap();
        assert_eq!(e.frequency, 1);
        assert_eq!(e.pinyin, vec!["ce4", "shi4"]);
    }

    #[test]
    fn test_load_and_lookup() {
        let content = "# comment\n测试 ce4 shi4 100\n例子 li4 zi5 50\n";
        let path = "/tmp/test_base_dict.txt";
        std::fs::write(path, content).unwrap();
        let mut dict = BaseDict::new();
        dict.load_from_file(path).unwrap();
        std::fs::remove_file(path).unwrap();

        let result = dict.lookup("ce4 shi4");
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 1);
        assert_eq!(result.unwrap()[0].text, "测试");
    }

    #[test]
    fn test_empty_dict() {
        let dict = BaseDict::new();
        assert!(dict.is_empty());
        assert_eq!(dict.entry_count(), 0);
    }
}
