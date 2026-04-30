use dict::Candidate;

/// Sort candidates by score descending (higher score = better).
/// For Phase 1, simple frequency-based sort.
/// Phase 3 will add bigram / freshness boost.
pub fn sort_candidates(candidates: &mut [Candidate]) {
    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sort_candidates() {
        let mut candidates = vec![
            Candidate {
                text: "低".into(),
                pinyin: vec!["di1".into()],
                score: 10.0,
            },
            Candidate {
                text: "高".into(),
                pinyin: vec!["gao1".into()],
                score: 100.0,
            },
            Candidate {
                text: "中".into(),
                pinyin: vec!["zhong1".into()],
                score: 50.0,
            },
        ];
        sort_candidates(&mut candidates);
        assert_eq!(candidates[0].text, "高");
        assert_eq!(candidates[1].text, "中");
        assert_eq!(candidates[2].text, "低");
    }

    #[test]
    fn test_sort_empty() {
        let mut candidates: Vec<Candidate> = Vec::new();
        sort_candidates(&mut candidates);
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_sort_single() {
        let mut candidates = vec![Candidate {
            text: "唯一".into(),
            pinyin: vec!["wei2".into(), "yi1".into()],
            score: 42.0,
        }];
        sort_candidates(&mut candidates);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].text, "唯一");
    }
}
