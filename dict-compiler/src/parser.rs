use std::path::Path;

#[allow(dead_code)]
pub struct RawEntry {
    pub text: String,
    pub pinyin: Vec<String>,
    pub frequency: u32,
    pub weight: i32,
}

/// Parse a word list file.
/// Format per line: `text pinyin1 pinyin2 ... [frequency] [weight]`
/// Lines starting with # are comments.
/// Blank lines are skipped.
pub fn parse_word_list(path: &Path) -> Result<Vec<RawEntry>, std::io::Error> {
    let content = std::fs::read_to_string(path)?;
    let mut entries = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(entry) = parse_line(line) {
            entries.push(entry);
        }
    }

    Ok(entries)
}

fn parse_line(line: &str) -> Option<RawEntry> {
    if line.starts_with('#') {
        return None;
    }
    let mut parts = line.split_whitespace();
    let text = parts.next()?;
    let mut pinyin = Vec::new();

    for part in parts.by_ref() {
        if let Ok(freq) = part.parse::<u32>() {
            let weight = parts.next().and_then(|w| w.parse::<i32>().ok()).unwrap_or(0);
            return Some(RawEntry {
                text: text.to_string(),
                pinyin,
                frequency: freq,
                weight,
            });
        }
        pinyin.push(part.to_string());
    }

    Some(RawEntry {
        text: text.to_string(),
        pinyin,
        frequency: 1,
        weight: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_line_full() {
        let entry = parse_line("输入法 shu1 ru4 fa3 50 10").unwrap();
        assert_eq!(entry.text, "输入法");
        assert_eq!(entry.pinyin, vec!["shu1", "ru4", "fa3"]);
        assert_eq!(entry.frequency, 50);
        assert_eq!(entry.weight, 10);
    }

    #[test]
    fn test_parse_line_no_weight() {
        let entry = parse_line("测试 ce4 shi4 100").unwrap();
        assert_eq!(entry.frequency, 100);
        assert_eq!(entry.weight, 0);
    }

    #[test]
    fn test_parse_line_no_freq() {
        let entry = parse_line("测试 ce4 shi4").unwrap();
        assert_eq!(entry.frequency, 1);
        assert_eq!(entry.pinyin, vec!["ce4", "shi4"]);
    }

    #[test]
    fn test_skip_comment() {
        let result = parse_line("# comment");
        assert!(result.is_none());
    }
}
