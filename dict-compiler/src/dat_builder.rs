use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Double-Array Trie Builder
//
// Builds a DAT from a set of string keys. Each key maps to a sequential
// terminal_id (0, 1, 2, ...) assigned at insertion time.
//
// The DAT uses a separate `values` array for terminal node data because
// a node can be both terminal AND have children (e.g. "shi" is a word but
// also a prefix of "shijian"). Encoding terminal_id in base via negation
// would corrupt the transition offset needed for children.
// ---------------------------------------------------------------------------

/// Internal trie node used during DAT construction.
struct TrieNode {
    children: BTreeMap<u8, usize>,
    is_terminal: bool,
    terminal_id: u32,
}

impl TrieNode {
    fn new() -> Self {
        Self {
            children: BTreeMap::new(),
            is_terminal: false,
            terminal_id: 0,
        }
    }
}

/// Builds Double-Array Trie arrays from string keys.
pub struct DatBuilder {
    nodes: Vec<TrieNode>,
}

impl Default for DatBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DatBuilder {
    pub fn new() -> Self {
        Self {
            nodes: vec![TrieNode::new()],
        }
    }

    /// Insert a key and return its terminal_id (sequential, 0-based).
    /// Repeated insert of the same key returns the same ID.
    pub fn insert(&mut self, key: &str) -> u32 {
        let mut node_idx = 0usize;
        for &b in key.as_bytes() {
            let code = char_code(b);
            assert!(code > 0, "invalid character in key: byte=0x{:02x}", b);
            // Two-phase insert to avoid borrow issues with self.nodes
            let has_child = self.nodes[node_idx].children.contains_key(&code);
            if !has_child {
                let idx = self.nodes.len();
                self.nodes.push(TrieNode::new());
                self.nodes[node_idx].children.insert(code, idx);
            }
            node_idx = self.nodes[node_idx].children[&code];
        }

        let terminal_count = self.nodes.iter().filter(|n| n.is_terminal).count() as u32;
        let node = &mut self.nodes[node_idx];
        if !node.is_terminal {
            node.is_terminal = true;
            node.terminal_id = terminal_count;
        }
        self.nodes[node_idx].terminal_id
    }

    /// Convert the intermediate trie to DAT arrays.
    ///
    /// Returns `(base, check, values)`:
    /// - `base[i]` with `abs(base[i])` for child transitions; `base[i] < 0` = terminal
    /// - `check[next] == parent_index` validates a transition
    /// - `values[i]` = terminal_id if `base[i] < 0`, else -1
    pub fn build(&self) -> (Vec<i32>, Vec<i32>, Vec<i32>) {
        let capacity = estimate_capacity(&self.nodes);
        let mut base = vec![0i32; capacity];
        let mut check = vec![-1i32; capacity]; // -1 = free
        let mut values = vec![-1i32; capacity];
        let mut trie_to_dat = vec![0usize; self.nodes.len()]; // trie_idx → dat_pos

        let mut queue = std::collections::VecDeque::new();
        queue.push_back((0usize, 0usize)); // (trie_idx, dat_pos)

        while let Some((trie_idx, dat_pos)) = queue.pop_front() {
            let node = &self.nodes[trie_idx];
            if node.children.is_empty() {
                continue;
            }

            // Collect child character codes
            let codes: Vec<u8> = node.children.keys().copied().collect();

            // Find a free base value
            let b = find_free_base(&codes, &check);

            // --- Assign transitions ---
            base[dat_pos] = b;

            for &code in &codes {
                let next_pos = (b.abs() as usize) + (code as usize);
                if next_pos >= base.len() {
                    let new_len = next_pos.saturating_add(1).next_power_of_two().max(base.len() * 2);
                    base.resize(new_len, 0);
                    check.resize(new_len, -1);
                    values.resize(new_len, -1);
                }
                check[next_pos] = dat_pos as i32;

                let child_trie_idx = node.children[&code];
                trie_to_dat[child_trie_idx] = next_pos;
                queue.push_back((child_trie_idx, next_pos));
            }
        }

        // --- Set terminal flags ---
        for (trie_idx, node) in self.nodes.iter().enumerate() {
            if !node.is_terminal {
                continue;
            }
            let dat_pos = trie_to_dat[trie_idx];
            let orig_base = base[dat_pos];
            if orig_base == 0 {
                base[dat_pos] = -1;
            } else {
                base[dat_pos] = -orig_base.abs();
            }
            values[dat_pos] = node.terminal_id as i32;
        }

        (base, check, values)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Map byte to character code (1-37).
/// a-z → 1-26, space → 27, 0-9 → 28-37.
fn char_code(b: u8) -> u8 {
    match b {
        b'a'..=b'z' => b - b'a' + 1,
        b' ' => 27,
        b'0'..=b'9' => b - b'0' + 28,
        _ => 0,
    }
}

/// Estimate initial capacity for base/check arrays.
/// Uses total child count + 50% overhead to keep arrays compact.
fn estimate_capacity(nodes: &[TrieNode]) -> usize {
    let total_children: usize = nodes.iter().map(|n| n.children.len()).sum();
    let estimated = (total_children + total_children / 2).max(1024);
    estimated.next_power_of_two()
}

/// Find a base value `b` such that `b + code` is a free slot for each child.
/// Returns a base that allows out-of-bounds positions to trigger caller resize.
fn find_free_base(codes: &[u8], check: &[i32]) -> i32 {
    if codes.is_empty() {
        return 0;
    }

    // Sort codes so smaller values (a, b, c...) get placed first
    let max_code = *codes.iter().max().unwrap() as usize;
    let mut base = 1i32;
    'search: loop {
        // If base + max_code would exceed capacity, return this base
        // to let the caller resize — avoids scanning past bounds
        if (base as usize) + max_code >= check.len() {
            return base;
        }
        for &code in codes {
            let pos = (base as usize) + (code as usize);
            if check[pos] >= 0 {
                base += 1;
                continue 'search;
            }
        }
        return base;
    }
}

// ---------------------------------------------------------------------------
// Public search function (used in serialization and tests)
// ---------------------------------------------------------------------------

/// Search a key in DAT arrays. Returns `Some((dat_position, terminal_id))`
/// if found, `None` otherwise.
pub fn dat_search(key: &str, base: &[i32], check: &[i32], values: &[i32]) -> Option<(usize, u32)> {
    let mut pos = 0usize;
    for &b in key.as_bytes() {
        let code = char_code(b);
        if code == 0 {
            return None;
        }
        if pos >= base.len() {
            return None;
        }
        let next = (base[pos].abs() as usize) + (code as usize);
        if next >= base.len() || check[next] as usize != pos {
            return None;
        }
        pos = next;
    }

    if pos < base.len() && base[pos] < 0 {
        let terminal_id = values.get(pos).copied().unwrap_or(-1);
        if terminal_id >= 0 {
            Some((pos, terminal_id as u32))
        } else {
            None
        }
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_char_code() {
        assert_eq!(char_code(b'a'), 1);
        assert_eq!(char_code(b'z'), 26);
        assert_eq!(char_code(b' '), 27);
        assert_eq!(char_code(b'0'), 28);
        assert_eq!(char_code(b'9'), 37);
    }

    #[test]
    fn test_single_key() {
        let mut builder = DatBuilder::new();
        let tid = builder.insert("ni");
        assert_eq!(tid, 0);
        let (base, check, values) = builder.build();
        let result = dat_search("ni", &base, &check, &values);
        assert!(result.is_some());
        assert_eq!(result.unwrap().1, 0);
    }

    #[test]
    fn test_multiple_keys() {
        let mut builder = DatBuilder::new();
        assert_eq!(builder.insert("ni"), 0);
        assert_eq!(builder.insert("hao"), 1);

        let (base, check, values) = builder.build();
        assert_eq!(dat_search("ni", &base, &check, &values).unwrap().1, 0);
        assert_eq!(dat_search("hao", &base, &check, &values).unwrap().1, 1);
    }

    #[test]
    fn test_duplicate_key() {
        let mut builder = DatBuilder::new();
        assert_eq!(builder.insert("shi"), 0);
        assert_eq!(builder.insert("shi"), 0); // same ID

        let (base, check, values) = builder.build();
        assert_eq!(dat_search("shi", &base, &check, &values).unwrap().1, 0);
    }

    #[test]
    fn test_prefix_keys() {
        // "shi" is a prefix of "shijian"
        let mut builder = DatBuilder::new();
        assert_eq!(builder.insert("shi"), 0);
        assert_eq!(builder.insert("shijian"), 1);

        let (base, check, values) = builder.build();
        assert_eq!(dat_search("shi", &base, &check, &values).unwrap().1, 0);
        assert_eq!(dat_search("shijian", &base, &check, &values).unwrap().1, 1);
    }

    #[test]
    fn test_key_with_spaces() {
        let mut builder = DatBuilder::new();
        builder.insert("shu1 ru4 fa3");
        builder.insert("shu1 fu2");

        let (base, check, values) = builder.build();
        let r1 = dat_search("shu1 ru4 fa3", &base, &check, &values);
        let r2 = dat_search("shu1 fu2", &base, &check, &values);
        assert!(r1.is_some());
        assert!(r2.is_some());
        assert_eq!(r1.unwrap().1, 0);
        assert_eq!(r2.unwrap().1, 1);
    }

    #[test]
    fn test_missing_key() {
        let mut builder = DatBuilder::new();
        builder.insert("ni");
        let (base, check, values) = builder.build();
        assert!(dat_search("hao", &base, &check, &values).is_none());
        assert!(dat_search("", &base, &check, &values).is_none());
        assert!(dat_search("n", &base, &check, &values).is_none()); // prefix
    }

    #[test]
    fn test_large_set() {
        let mut builder = DatBuilder::new();
        let keys: Vec<String> = (0..1000)
            .map(|i| format!("test{i}"))
            .collect();
        for (i, key) in keys.iter().enumerate() {
            assert_eq!(builder.insert(key) as usize, i);
        }

        let (base, check, values) = builder.build();
        for (i, key) in keys.iter().enumerate() {
            let result = dat_search(key, &base, &check, &values);
            assert!(result.is_some(), "key '{key}' not found");
            assert_eq!(result.unwrap().1 as usize, i);
        }
    }
}
