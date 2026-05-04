use dict::pinyin_table::PinyinTable;

// ---------------------------------------------------------------------------
// PinyinBuffer — manages the raw input text and cursor position
// ---------------------------------------------------------------------------
#[derive(Debug, Clone)]
pub struct PinyinBuffer {
    raw_input: String,
    cursor_position: usize,
}

impl PinyinBuffer {
    pub fn new() -> Self {
        Self {
            raw_input: String::new(),
            cursor_position: 0,
        }
    }

    pub fn insert_at_cursor(&mut self, ch: char) {
        self.raw_input.insert(self.cursor_position, ch);
        self.cursor_position += ch.len_utf8();
    }

    pub fn delete_before_cursor(&mut self) {
        if self.cursor_position == 0 {
            return;
        }
        let prev = self.raw_input[..self.cursor_position]
            .char_indices()
            .last()
            .map(|(i, _c)| i)
            .unwrap_or(0);
        self.raw_input.remove(prev);
        self.cursor_position = prev;
    }

    pub fn move_cursor(&mut self, delta: isize) {
        if delta < 0 {
            let n = (-delta) as usize;
            for _ in 0..n {
                if self.cursor_position == 0 {
                    break;
                }
                let prev = self.raw_input[..self.cursor_position]
                    .char_indices()
                    .last()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                self.cursor_position = prev;
            }
        } else {
            let n = delta as usize;
            for _ in 0..n {
                if self.cursor_position >= self.raw_input.len() {
                    break;
                }
                self.cursor_position += self.raw_input[self.cursor_position..]
                    .chars()
                    .next()
                    .map(|c| c.len_utf8())
                    .unwrap_or(0);
            }
        }
    }

    pub fn raw_input(&self) -> &str {
        &self.raw_input
    }

    pub fn cursor_position(&self) -> usize {
        self.cursor_position
    }

    pub fn is_empty(&self) -> bool {
        self.raw_input.is_empty()
    }

    pub fn clear(&mut self) {
        self.raw_input.clear();
        self.cursor_position = 0;
    }

    pub fn before_cursor(&self) -> &str {
        &self.raw_input[..self.cursor_position]
    }

    pub fn after_cursor(&self) -> &str {
        &self.raw_input[self.cursor_position..]
    }
}

impl Default for PinyinBuffer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// PinyinSyllabler — pinyin string segmentation using DAG + DP
// ---------------------------------------------------------------------------
pub struct PinyinSyllabler {
    table: PinyinTable,
}

impl PinyinSyllabler {
    pub fn new() -> Self {
        Self {
            table: PinyinTable::new(),
        }
    }

    /// Generate all possible segmentation paths (DAG).
    pub fn all_segmentations(&self, input: &str) -> Vec<Vec<String>> {
        let mut results = Vec::new();
        let mut current = Vec::new();
        self.backtrack(input, 0, &mut current, &mut results);
        results
    }

    fn backtrack(
        &self,
        input: &str,
        start: usize,
        current: &mut Vec<String>,
        results: &mut Vec<Vec<String>>,
    ) {
        if start >= input.len() {
            results.push(current.clone());
            return;
        }

        let matches = self.table.prefixes(input, start);
        if matches.is_empty() {
            // No valid syllable at this position — skip this path
            return;
        }

        for (end, _) in &matches {
            current.push(input[start..*end].to_string());
            self.backtrack(input, *end, current, results);
            current.pop();
        }
    }

    /// Find the best segmentation using DP:
    /// - Minimize syllable count (fewer syllables = better)
    /// - Tie-break: prefer longer first syllables ("xian" over "xi"+"an")
    pub fn best_segmentation(&self, input: &str) -> Vec<String> {
        if input.is_empty() {
            return Vec::new();
        }

        let n = input.len();
        // dp[i] = (syllable_count, prev_pos, first_syllable_len)
        // We minimize syllable_count; tie-break by maximizing first_syllable_len
        let mut dp: Vec<Option<(usize, usize, usize)>> = vec![None; n + 1];
        dp[0] = Some((0, 0, 0));

        for i in 0..n {
            let cur = match dp[i] {
                Some(c) => c,
                None => continue,
            };

            let matches = self.table.prefixes(input, i);
            for (end, _) in &matches {
                let new_count = cur.0 + 1;
                let first_len = match (cur.1 == 0, i == 0) {
                    (true, true) => *end,       // first segment: spans 0..*end
                    (true, false) => i,          // first segment: spans 0..i
                    (false, _) => cur.2,         // inherit first segment length
                };

                let candidate = (new_count, i, first_len);
                match dp[*end] {
                    None => dp[*end] = Some(candidate),
                    Some((existing_count, _, existing_first)) => {
                        if new_count < existing_count
                            || (new_count == existing_count && first_len > existing_first)
                        {
                            dp[*end] = Some(candidate);
                        }
                    }
                }
            }
        }

        // Backtrace to build the best segmentation
        let mut pos = n;
        let mut segments = Vec::new();

        while pos > 0 {
            let (_, prev, _) = match dp[pos] {
                Some(val) => val,
                None => return Vec::new(), // no valid segmentation
            };
            segments.push(input[prev..pos].to_string());
            pos = prev;
        }

        segments.reverse();
        segments
    }

    /// Greedy left-to-right segmentation for fallback when `best_segmentation` fails.
    /// At each position, takes the longest valid syllable. If no syllable starts at
    /// the current position, uses `shortest_syllable_for_char` as a proxy (e.g., 'j' → "ji").
    pub fn greedy_segmentation(&self, input: &str) -> Vec<String> {
        let mut result = Vec::new();
        let mut pos = 0;
        let bytes = input.as_bytes();

        while pos < bytes.len() {
            let matches = self.table.prefixes(input, pos);
            if let Some(&(end, _)) = matches.last() {
                result.push(input[pos..end].to_string());
                pos = end;
            } else if let Some(proxy) = self.table.shortest_syllable_for_char(bytes[pos] as char) {
                result.push(proxy);
                pos += 1;
            } else {
                pos += 1;
            }
        }
        result
    }

    /// Convert a list of syllable strings to a dictionary lookup key.
    pub fn syllables_to_key(&self, syllables: &[String]) -> String {
        syllables.join(" ")
    }

    /// Get all possible ambiguous syllable splits at current position.
    pub fn ambiguous_syllables(&self, input: &str) -> Vec<Vec<String>> {
        if input.is_empty() {
            return Vec::new();
        }
        let mut result = Vec::new();
        let matches = self.table.prefixes(input, 0);
        for (end, _) in &matches {
            let first = input[0..*end].to_string();
            let rest = input[*end..].to_string();
            if !rest.is_empty() {
                let rest_segments = self.best_segmentation(&rest);
                if rest_segments.is_empty() {
                    continue; // skip — rest can't be fully segmented
                }
                let mut combined = vec![first];
                combined.extend(rest_segments);
                result.push(combined);
            } else {
                result.push(vec![first]);
            }
        }
        result
    }
}

impl Default for PinyinSyllabler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pinyin_buffer_basic() {
        let mut buf = PinyinBuffer::new();
        buf.insert_at_cursor('n');
        buf.insert_at_cursor('i');
        buf.insert_at_cursor('h');
        buf.insert_at_cursor('a');
        buf.insert_at_cursor('o');
        assert_eq!(buf.raw_input(), "nihao");
    }

    #[test]
    fn test_pinyin_buffer_insert_delete() {
        let mut buf = PinyinBuffer::new();
        buf.insert_at_cursor('h');
        assert_eq!(buf.raw_input(), "h");
        assert_eq!(buf.cursor_position(), 1);

        buf.insert_at_cursor('e');
        assert_eq!(buf.raw_input(), "he");
        assert_eq!(buf.cursor_position(), 2);

        buf.move_cursor(-1);
        assert_eq!(buf.cursor_position(), 1);

        buf.insert_at_cursor('a');
        assert_eq!(buf.raw_input(), "hae");
        assert_eq!(buf.cursor_position(), 2);

        buf.delete_before_cursor();
        assert_eq!(buf.raw_input(), "he");
    }

    #[test]
    fn test_pinyin_buffer_cursor_edge() {
        let mut buf = PinyinBuffer::new();
        buf.insert_at_cursor('a');
        buf.insert_at_cursor('b');
        // Cursor at end: "ab"
        buf.move_cursor(-10);
        assert_eq!(buf.cursor_position(), 0);
        buf.move_cursor(10);
        assert_eq!(buf.cursor_position(), 2);
    }

    #[test]
    fn test_best_segmentation_simple() {
        let s = PinyinSyllabler::new();
        let result = s.best_segmentation("ni");
        assert_eq!(result, vec!["ni"]);
    }

    #[test]
    fn test_best_segmentation_xian() {
        // "xian" should segment as ["xian"], not ["xi", "an"]
        let s = PinyinSyllabler::new();
        let result = s.best_segmentation("xian");
        assert_eq!(result, vec!["xian"]);
    }

    #[test]
    fn test_best_segmentation_ni_hao() {
        let s = PinyinSyllabler::new();
        let result = s.best_segmentation("nihao");
        assert_eq!(result, vec!["ni", "hao"]);
    }

    #[test]
    fn test_best_segmentation_fangan() {
        let s = PinyinSyllabler::new();
        let result = s.best_segmentation("fangan");
        // Should prefer ["fang", "an"] over ["fan", "gan"]
        assert_eq!(result, vec!["fang", "an"]);
    }

    #[test]
    fn test_best_segmentation_jiandanzhijie() {
        let s = PinyinSyllabler::new();
        let result = s.best_segmentation("jiandanzhijie");
        assert_eq!(result, vec!["jian", "dan", "zhi", "jie"]);
    }

    #[test]
    fn test_syllables_to_key() {
        let s = PinyinSyllabler::new();
        let syllables = vec!["shu1".to_string(), "ru4".to_string(), "fa3".to_string()];
        assert_eq!(s.syllables_to_key(&syllables), "shu1 ru4 fa3");
    }

    #[test]
    fn test_ambiguous_syllables() {
        let s = PinyinSyllabler::new();
        let ambig = s.ambiguous_syllables("xian");
        // Should include at least ["xian"] and potentially ["xi", "an"]
        assert!(ambig.iter().any(|v| v.join("") == "xian"));
    }

    #[test]
    fn test_pinyin_buffer_before_after_cursor() {
        let mut buf = PinyinBuffer::new();
        buf.insert_at_cursor('s');
        buf.insert_at_cursor('h');
        assert_eq!(buf.before_cursor(), "sh");
        assert_eq!(buf.after_cursor(), "");
        buf.insert_at_cursor('i');
        assert_eq!(buf.raw_input(), "shi");
    }

    #[test]
    fn test_greedy_segmentation_jjjj() {
        let s = PinyinSyllabler::new();
        // "j" alone is not a valid syllable; greedy maps each to shortest j-syllable
        let result = s.greedy_segmentation("jjjj");
        assert_eq!(result.len(), 4);
        // Each proxy syllable should be a valid 2-char syllable starting with 'j'
        for syl in &result {
            assert!(syl.starts_with('j'));
            assert!(syl.len() >= 2);
        }
    }

    #[test]
    fn test_greedy_segmentation_asdf() {
        let s = PinyinSyllabler::new();
        // "a" is valid, "s"→"si", "d"→"de"/"di", "f"→"fo"/"fu"
        let result = s.greedy_segmentation("asdf");
        assert!(!result.is_empty());
        assert_eq!(result[0], "a"); // "a" is a complete syllable
    }

    #[test]
    fn test_greedy_segmentation_valid_input() {
        let s = PinyinSyllabler::new();
        // valid input should produce same result as best_segmentation
        let result = s.greedy_segmentation("nihao");
        assert_eq!(result, vec!["ni", "hao"]);
    }

    #[test]
    fn test_greedy_segmentation_mixed() {
        let s = PinyinSyllabler::new();
        // "wox" → "wo" is valid, "x" → proxy
        let result = s.greedy_segmentation("wox");
        assert_eq!(result[0], "wo");
        assert!(result.len() >= 2);
    }
}
