use std::collections::HashSet;

/// Fuzzy pinyin mapping rules.
///
/// When enabled, generates alternative pinyin spellings to match
/// common pronunciation variations (e.g., zh↔z, an↔ang, l↔n).
pub struct FuzzyPinyin {
    /// Mapping from syllable to its fuzzy equivalents.
    /// Each entry maps "original" → ["fuzzy1", "fuzzy2", ...]
    rules: Vec<(String, Vec<String>)>,
}

impl FuzzyPinyin {
    /// Create with default fuzzy rules for standard Mandarin variations.
    pub fn new() -> Self {
        let rules = vec![
            // 翘舌音 ↔ 平舌音
            ("zh".into(), vec!["z".into()]),
            ("ch".into(), vec!["c".into()]),
            ("sh".into(), vec!["s".into()]),
            // 前后鼻音
            ("an".into(), vec!["ang".into()]),
            ("en".into(), vec!["eng".into()]),
            ("in".into(), vec!["ing".into()]),
            // 声母混淆
            ("l".into(), vec!["n".into()]),
            ("n".into(), vec!["l".into()]),
            ("f".into(), vec!["h".into()]),
            ("h".into(), vec!["f".into()]),
            ("r".into(), vec!["l".into()]),
            // 带鼻音韵母
            ("ian".into(), vec!["iang".into()]),
            ("uan".into(), vec!["uang".into()]),
        ];
        Self { rules }
    }

    /// Generate all fuzzy variants of a single syllable.
    ///
    /// Returns a set including the original syllable and all fuzzy equivalents.
    /// Each fuzzy match applies exactly one rule transformation.
    pub fn variants(&self, syllable: &str) -> HashSet<String> {
        let mut result = HashSet::new();
        result.insert(syllable.to_string());

        for (pattern, replacements) in &self.rules {
            if syllable.starts_with(pattern) {
                let rest = &syllable[pattern.len()..];
                for rep in replacements {
                    result.insert(format!("{}{}", rep, rest));
                }
            }
            // Also check if the syllable is a fuzzy version of the pattern
            for rep in replacements {
                if syllable.starts_with(rep) {
                    let rest = &syllable[rep.len()..];
                    result.insert(format!("{}{}", pattern, rest));
                }
            }
        }

        result
    }

    /// Generate all fuzzy variants of a pinyin key (space-separated syllables).
    ///
    /// For each syllable position, we can either use the original or a fuzzy variant.
    /// This generates the Cartesian product of all combinations.
    ///
    /// Example: "ni hao" → ["ni hao", "li hao", "ni hao", ...]
    pub fn key_variants(&self, key: &str) -> Vec<String> {
        let syllables: Vec<&str> = key.split(' ').collect();
        if syllables.is_empty() {
            return vec![key.to_string()];
        }

        // Get variants for each syllable
        let all_variants: Vec<Vec<String>> = syllables
            .iter()
            .map(|s| {
                let v = self.variants(s);
                let mut sorted: Vec<String> = v.into_iter().collect();
                sorted.sort();
                sorted
            })
            .collect();

        // Cartesian product
        let mut result = Vec::new();
        let mut current = vec![String::new(); syllables.len()];
        self.cartesian_product(&all_variants, 0, &mut current, &mut result);
        result
    }

    fn cartesian_product(
        &self,
        variants: &[Vec<String>],
        pos: usize,
        current: &mut [String],
        result: &mut Vec<String>,
    ) {
        if pos == variants.len() {
            result.push(current.join(" "));
            return;
        }

        for variant in &variants[pos] {
            current[pos] = variant.clone();
            self.cartesian_product(variants, pos + 1, current, result);
        }
    }

    /// Check if two syllables are fuzzy-equivalent.
    pub fn is_equivalent(&self, a: &str, b: &str) -> bool {
        if a == b {
            return true;
        }
        self.variants(a).contains(b)
    }
}

impl Default for FuzzyPinyin {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variants_zh() {
        let fp = FuzzyPinyin::new();
        let v = fp.variants("zhuang");
        assert!(v.contains("zhuang"));
        assert!(v.contains("zuang")); // zh → z
    }

    #[test]
    fn test_variants_an() {
        let fp = FuzzyPinyin::new();
        let v = fp.variants("an");
        assert!(v.contains("an"));
        assert!(v.contains("ang")); // an → ang
    }

    #[test]
    fn test_variants_ni() {
        let fp = FuzzyPinyin::new();
        let v = fp.variants("ni");
        assert!(v.contains("ni"));
        assert!(v.contains("li")); // n ↔ l
    }

    #[test]
    fn test_variants_no_match() {
        let fp = FuzzyPinyin::new();
        let v = fp.variants("ba");
        assert_eq!(v.len(), 1);
        assert!(v.contains("ba"));
    }

    #[test]
    fn test_key_variants_single() {
        let fp = FuzzyPinyin::new();
        let vars = fp.key_variants("ni");
        assert!(vars.contains(&"ni".to_string()));
        assert!(vars.contains(&"li".to_string()));
    }

    #[test]
    fn test_key_variants_multiple() {
        let fp = FuzzyPinyin::new();
        let vars = fp.key_variants("ni hao");
        // Original
        assert!(vars.contains(&"ni hao".to_string()));
        // ni → li
        assert!(vars.contains(&"li hao".to_string()));
    }

    #[test]
    fn test_key_variants_with_zh() {
        let fp = FuzzyPinyin::new();
        let vars = fp.key_variants("zhuang");
        assert!(vars.contains(&"zhuang".to_string()));
        assert!(vars.contains(&"zuang".to_string()));
    }

    #[test]
    fn test_is_equivalent() {
        let fp = FuzzyPinyin::new();
        assert!(fp.is_equivalent("zhong", "zhong"));
        assert!(fp.is_equivalent("zhong", "zong"));
        assert!(fp.is_equivalent("zong", "zhong"));
        assert!(!fp.is_equivalent("zhong", "bang"));
    }

    #[test]
    fn test_key_variants_empty() {
        let fp = FuzzyPinyin::new();
        let vars = fp.key_variants("");
        assert_eq!(vars, vec![""]);
    }
}
