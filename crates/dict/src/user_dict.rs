use std::collections::HashMap;
use std::num::NonZeroUsize;

use lru::LruCache;
use rusqlite::Connection;

use crate::{Candidate, DictEntry};

/// Maximum pending writes before auto-flush.
const FLUSH_THRESHOLD: usize = 50;
/// LRU cache capacity.
const CACHE_CAPACITY: usize = 1000;

#[derive(Debug)]
enum PendingWrite {
    Upsert {
        text: String,
        pinyin: Vec<String>,
        frequency: u32,
        updated_at: u64,
    },
}

/// Personal dictionary with SQLite persistence.
///
/// Read path (hot):  in-memory HashMap → LRU cache
/// Write path (cold): batched → SQLite transaction (every 50 writes)
pub struct UserDict {
    /// In-memory primary store: pinyin_key -> entries
    entries: HashMap<String, Vec<DictEntry>>,
    /// Hot query cache
    query_cache: LruCache<String, Vec<DictEntry>>,
    /// Writes not yet flushed to SQLite
    pending_writes: Vec<PendingWrite>,
    /// SQLite database path
    db_path: String,
}

impl UserDict {
    /// Open (or create) the SQLite database and load all entries into memory.
    pub fn open(db_path: &str) -> Self {
        let mut dict = Self {
            entries: HashMap::new(),
            query_cache: LruCache::new(
                NonZeroUsize::new(CACHE_CAPACITY).expect("CACHE_CAPACITY > 0"),
            ),
            pending_writes: Vec::new(),
            db_path: db_path.to_string(),
        };

        if let Err(e) = dict.load_from_db() {
            log::warn!("Failed to load user dict from '{}': {e}", db_path);
        }
        dict
    }

    /// Create the SQLite tables if they don't exist.
    fn init_db(&self, conn: &Connection) -> Result<(), rusqlite::Error> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS user_dict (
                id    INTEGER PRIMARY KEY AUTOINCREMENT,
                text  TEXT NOT NULL UNIQUE,
                pinyin TEXT NOT NULL,
                frequency INTEGER DEFAULT 0,
                created_at INTEGER DEFAULT (unixepoch()),
                updated_at INTEGER DEFAULT (unixepoch())
            );
            CREATE INDEX IF NOT EXISTS idx_user_dict_text ON user_dict(text);",
        )?;
        Ok(())
    }

    /// Load all entries from SQLite into the in-memory HashMap.
    fn load_from_db(&mut self) -> Result<(), rusqlite::Error> {
        let conn = Connection::open(&self.db_path)?;
        self.init_db(&conn)?;

        {
            let mut stmt = conn.prepare(
                "SELECT text, pinyin, frequency, updated_at FROM user_dict",
            )?;
            let rows = stmt.query_map([], |row| {
                let text: String = row.get(0)?;
                let pinyin_str: String = row.get(1)?;
                let frequency: u32 = row.get(2)?;
                let updated_at: u64 = row.get(3)?;
                Ok((text, pinyin_str, frequency, updated_at))
            })?;

            for row in rows.flatten() {
                let (text, pinyin_str, frequency, updated_at) = row;
                let pinyin: Vec<String> = pinyin_str.split(' ').map(|s| s.to_string()).collect();
                let key = pinyin.join(" ");

                self.entries.entry(key).or_default().push(DictEntry {
                    text,
                    pinyin,
                    frequency,
                    weight: 10,
                    is_user: true,
                    updated_at,
                });
            }
        }

        conn.close().map_err(|(_, e)| e)?;
        Ok(())
    }

    /// Look up entries by pinyin key (space-separated, e.g. "shu1 ru4 fa3").
    pub fn lookup(&mut self, pinyin_key: &str) -> Option<&Vec<DictEntry>> {
        // 1. Check LRU cache
        if self.query_cache.contains(pinyin_key) {
            return self.query_cache.get(pinyin_key);
        }

        // 2. Check in-memory HashMap
        let result = self.entries.get(pinyin_key)?;
        self.query_cache.put(pinyin_key.to_string(), result.clone());
        self.query_cache.get(pinyin_key)
    }

    /// Record a user selection for auto-learning.
    ///
    /// - `delta`: 1 for commit (space), 2 for explicit selection (number key)
    pub fn learn(&mut self, text: &str, pinyin: Vec<String>, delta: u32) {
        let now = now();
        let key = pinyin.join(" ");

        // Update in-memory entry
        let entries = self.entries.entry(key.clone()).or_default();
        let mut found = false;
        for entry in entries.iter_mut() {
            if entry.text == text {
                entry.frequency += delta;
                entry.updated_at = now;
                found = true;
                break;
            }
        }

        if !found {
            entries.push(DictEntry {
                text: text.to_string(),
                pinyin: pinyin.clone(),
                frequency: delta,
                weight: 10,
                is_user: true,
                updated_at: now,
            });
        }

        // Invalidate LRU cache for this key
        self.query_cache.pop(&key);

        // Queue write
        let freq = entries
            .iter()
            .find(|e| e.text == text)
            .map(|e| e.frequency)
            .unwrap_or(delta);
        self.pending_writes.push(PendingWrite::Upsert {
            text: text.to_string(),
            pinyin: pinyin.clone(),
            frequency: freq,
            updated_at: now,
        });

        // Auto-flush at threshold
        if self.pending_writes.len() >= FLUSH_THRESHOLD {
            if let Err(e) = self.flush() {
                log::warn!("Failed to flush user dict: {e}");
            }
        }
    }

    /// Flush all pending writes to SQLite in a single transaction.
    pub fn flush(&mut self) -> Result<(), rusqlite::Error> {
        if self.pending_writes.is_empty() {
            return Ok(());
        }

        let mut conn = Connection::open(&self.db_path)?;
        self.init_db(&conn)?;

        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO user_dict (text, pinyin, frequency, updated_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(text) DO UPDATE SET
                    frequency = excluded.frequency,
                    pinyin = excluded.pinyin,
                    updated_at = excluded.updated_at",
            )?;

            for write in &self.pending_writes {
                match write {
                    PendingWrite::Upsert {
                        text,
                        pinyin,
                        frequency,
                        updated_at,
                    } => {
                        let pinyin_str = pinyin.join(" ");
                        stmt.execute(rusqlite::params![text, pinyin_str, frequency, updated_at])?;
                    }
                }
            }
        }
        tx.commit()?;
        conn.close().map_err(|(_, e)| e)?;

        self.pending_writes.clear();
        Ok(())
    }

    /// Search entries by hanzi substring (fallback).
    pub fn search_by_text(&self, query: &str) -> Vec<Candidate> {
        self.entries
            .values()
            .flatten()
            .filter(|e| e.text.contains(query))
            .map(|e| Candidate {
                text: e.text.clone(),
                pinyin: e.pinyin.clone(),
                score: e.frequency as f64,
            })
            .collect()
    }

    /// Number of entries in memory.
    pub fn entry_count(&self) -> usize {
        self.entries.values().map(|v| v.len()).sum()
    }

    /// Number of pending writes.
    pub fn pending_count(&self) -> usize {
        self.pending_writes.len()
    }

    /// Whether there are pending writes to flush.
    pub fn needs_flush(&self) -> bool {
        !self.pending_writes.is_empty()
    }
}

impl Drop for UserDict {
    fn drop(&mut self) {
        if self.needs_flush() {
            if let Err(e) = self.flush() {
                log::warn!("UserDict final flush failed: {e}");
            }
        }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static DB_COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_db_path() -> String {
        let n = DB_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = format!("/tmp/test_user_dict_{n}.db");
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn test_open_creates_db() {
        let path = temp_db_path();
        let dict = UserDict::open(&path);
        assert_eq!(dict.entry_count(), 0);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_learn_and_lookup() {
        let path = temp_db_path();
        let mut dict = UserDict::open(&path);

        dict.learn("输入法", vec!["shu1".into(), "ru4".into(), "fa3".into()], 1);
        let result = dict.lookup("shu1 ru4 fa3");
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 1);
        assert_eq!(result.unwrap()[0].text, "输入法");
        assert_eq!(result.unwrap()[0].frequency, 1);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_learn_increments_frequency() {
        let path = temp_db_path();
        let mut dict = UserDict::open(&path);

        dict.learn("测试", vec!["ce4".into(), "shi4".into()], 1);
        dict.learn("测试", vec!["ce4".into(), "shi4".into()], 2);

        let result = dict.lookup("ce4 shi4").unwrap();
        assert_eq!(result[0].frequency, 3); // 1 + 2

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_flush_persistence() {
        let path = temp_db_path();
        {
            let mut dict = UserDict::open(&path);
            dict.learn("例子", vec!["li4".into(), "zi5".into()], 1);
            dict.flush().unwrap();
        }
        {
            let mut dict = UserDict::open(&path);
            let result = dict.lookup("li4 zi5");
            assert!(result.is_some());
            assert_eq!(result.unwrap()[0].text, "例子");
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_search_by_text() {
        let path = temp_db_path();
        let mut dict = UserDict::open(&path);
        dict.learn("输入法", vec!["shu1".into(), "ru4".into(), "fa3".into()], 1);
        dict.learn("舒服", vec!["shu1".into(), "fu2".into()], 1);

        let results = dict.search_by_text("输");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].text, "输入法");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_auto_flush_at_threshold() {
        let path = temp_db_path();
        let mut dict = UserDict::open(&path);

        // Learn 51 times to trigger auto-flush (threshold is 50)
        for i in 0..51 {
            dict.learn(
                &format!("词{i}"),
                vec!["ci2".into(), format!("{i}")],
                1,
            );
        }

        assert!(dict.needs_flush()); // might still have some pending
        dict.flush().unwrap();

        {
            let dict2 = UserDict::open(&path);
            assert!(dict2.entry_count() >= 51);
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_drop_flushes() {
        let path = temp_db_path();
        {
            let mut dict = UserDict::open(&path);
            dict.learn("临时", vec!["lin2".into(), "shi2".into()], 1);
            // Drop flushes automatically
        }
        {
            let mut dict = UserDict::open(&path);
            let result = dict.lookup("lin2 shi2");
            assert!(result.is_some());
            assert_eq!(result.unwrap()[0].text, "临时");
        }
        std::fs::remove_file(&path).ok();
    }
}
