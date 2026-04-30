use std::collections::HashMap;

/// Bigram model for context-based candidate re-ranking.
///
/// Stores frequency data mapping (prev_word, current_word) pairs,
/// used to boost candidates that commonly follow the previously
/// committed word when composing multi-word phrases.
pub struct BigramModel {
    /// prev_word -> Vec<(next_word, normalized_freq)> where
    /// normalized_freq is in [0, 1] relative to the max frequency for that prev_word
    data: HashMap<String, Vec<(String, f64)>>,
}

impl BigramModel {
    /// Empty model — no bigram boost applied.
    pub fn empty() -> Self {
        Self {
            data: HashMap::new(),
        }
    }

    /// Load bigram data from a text file.
    ///
    /// Format: one `prev_word next_word frequency` triple per line.
    /// Lines starting with `#` are skipped as comments.
    pub fn load(path: &str) -> Self {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                log::info!("No bigram data at '{}': {e}", path);
                return Self::empty();
            }
        };

        let mut raw: HashMap<String, Vec<(String, f64)>> = HashMap::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.splitn(3, ' ').collect();
            if parts.len() < 3 {
                continue;
            }
            let prev = parts[0].to_string();
            let next = parts[1].to_string();
            let freq: f64 = match parts[2].parse() {
                Ok(f) if f > 0.0 => f,
                _ => continue,
            };
            raw.entry(prev).or_default().push((next, freq));
        }

        // Normalize each prev_word's frequencies to [0, 1]
        for entries in raw.values_mut() {
            let max_freq = entries.iter().map(|(_, f)| *f).fold(0.0_f64, f64::max);
            if max_freq > 0.0 {
                for (_, f) in entries.iter_mut() {
                    *f /= max_freq;
                }
            }
        }

        log::info!("Loaded bigram model: {} context words", raw.len());
        Self { data: raw }
    }

    /// Return a score multiplier for `current` given `prev`.
    ///
    /// Returns `1.0` (no boost) when no bigram data exists for the pair.
    /// Max multiplier is `1.0 + BIGRAM_BOOST_MAX` (currently 3.0x).
    pub fn get_boost(&self, prev: &str, current: &str) -> f64 {
        const BIGRAM_BOOST_MAX: f64 = 2.0;

        if let Some(entries) = self.data.get(prev) {
            for (word, norm_freq) in entries {
                if word == current {
                    return 1.0 + norm_freq * BIGRAM_BOOST_MAX;
                }
            }
        }
        1.0
    }

    /// Number of context words in the model.
    pub fn len(&self) -> usize {
        self.data.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_model() {
        let model = BigramModel::empty();
        assert_eq!(model.len(), 0);
        assert!((model.get_boost("测试", "输入") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_load_from_content() {
        let path = "/tmp/test_bigram.txt";
        let content = "测试 输入 10\n测试 方法 5\n你好 世界 20\n";
        std::fs::write(path, content).unwrap();

        let model = BigramModel::load(path);
        assert_eq!(model.len(), 2);
        std::fs::remove_file(path).unwrap();

        // "输入" follows "测试" with freq 10/10 = normalized 1.0 → boost = 1.0 + 1.0*2.0 = 3.0
        let boost = model.get_boost("测试", "输入");
        assert!((boost - 3.0).abs() < f64::EPSILON);

        // "方法" follows "测试" with freq 5/10 = normalized 0.5 → boost = 1.0 + 0.5*2.0 = 2.0
        let boost = model.get_boost("测试", "方法");
        assert!((boost - 2.0).abs() < f64::EPSILON);

        // Unknown pair → no boost
        let boost = model.get_boost("测试", "未知");
        assert!((boost - 1.0).abs() < f64::EPSILON);

        // Unknown prev word → no boost
        let boost = model.get_boost("未知", "输入");
        assert!((boost - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_load_with_comments() {
        let path = "/tmp/test_bigram_comments.txt";
        let content = "# This is a comment\n测试 输入 10\n\n# Another comment\n";
        std::fs::write(path, content).unwrap();

        let model = BigramModel::load(path);
        assert_eq!(model.len(), 1);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn test_load_nonexistent_file() {
        let model = BigramModel::load("/tmp/nonexistent_bigram.txt");
        assert_eq!(model.len(), 0);
    }

    #[test]
    fn test_get_boost_bounds() {
        let path = "/tmp/test_bigram_bounds.txt";
        let content = "a b 100\n";
        std::fs::write(path, content).unwrap();

        let model = BigramModel::load(path);
        std::fs::remove_file(path).unwrap();

        // Max boost should be 3.0
        let boost = model.get_boost("a", "b");
        assert!((boost - 3.0).abs() < f64::EPSILON);

        // Min boost (unknown) should be 1.0
        let boost = model.get_boost("a", "c");
        assert!((boost - 1.0).abs() < f64::EPSILON);

        // Boost should be in [1.0, 3.0]
        assert!(boost >= 1.0 && boost <= 3.0);
        assert!(model.get_boost("a", "b") >= 1.0 && model.get_boost("a", "b") <= 3.0);
    }
}
