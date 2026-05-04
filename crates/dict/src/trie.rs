use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct TrieNode<V> {
    pub children: HashMap<char, TrieNode<V>>,
    pub values: Vec<V>,
}

impl<V> TrieNode<V> {
    fn new() -> Self {
        Self {
            children: HashMap::new(),
            values: Vec::new(),
        }
    }

    pub fn is_end(&self) -> bool {
        !self.values.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct Trie<V> {
    root: TrieNode<V>,
}

impl<V> Trie<V> {
    pub fn new() -> Self {
        Self {
            root: TrieNode::new(),
        }
    }

    pub fn insert(&mut self, key: &str, value: V) {
        let mut node = &mut self.root;
        for ch in key.chars() {
            node = node.children.entry(ch).or_insert_with(TrieNode::new);
        }
        node.values.push(value);
    }

    /// Walk character by character following `key`, return final node.
    pub fn traverse(&self, key: &str) -> Option<&TrieNode<V>> {
        let mut node = &self.root;
        for ch in key.chars() {
            node = node.children.get(&ch)?;
        }
        Some(node)
    }

    /// Check if `key` is a complete entry (has values).
    pub fn contains(&self, key: &str) -> bool {
        self.traverse(key).map_or(false, |n| n.is_end())
    }

    pub fn get(&self, key: &str) -> Option<&Vec<V>> {
        self.traverse(key).and_then(|n| {
            if n.is_end() {
                Some(&n.values)
            } else {
                None
            }
        })
    }

    /// Starting from position `start` in `input`, walk the trie and find all
    /// complete entries. Returns (end_byte_offset, &values) for each match.
    pub fn prefixes(&self, input: &str, start: usize) -> Vec<(usize, &Vec<V>)> {
        let mut results = Vec::new();
        let mut node = &self.root;
        for (i, ch) in input[start..].char_indices() {
            match node.children.get(&ch) {
                Some(next) => {
                    node = next;
                    if node.is_end() {
                        results.push((start + i + ch.len_utf8(), &node.values));
                    }
                }
                None => break,
            }
        }
        results
    }

    pub fn is_empty(&self) -> bool {
        self.root.children.is_empty()
    }

    pub fn root(&self) -> &TrieNode<V> {
        &self.root
    }
}

impl<V> Default for Trie<V> {
    fn default() -> Self {
        Self::new()
    }
}
