use std::fs::File;
use std::io::Write;
use std::path::Path;

use memmap2::Mmap;

use crate::DictEntry;

fn dict_log(msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("C:\\Users\\Verdana\\pyrust_tsf.log")
    {
        let _ = writeln!(f, "[dict] {msg}");
        let _ = f.flush();
    }
}

/// Magic bytes for the binary dictionary format.
const MAGIC: &[u8; 8] = b"INPUTDCT";

// ---------------------------------------------------------------------------
// MmapDict — mmap-based Double-Array Trie dictionary reader
// ---------------------------------------------------------------------------

/// Mmap-based Double-Array Trie dictionary reader.
///
/// Stores only the mmap handle and offsets — no self-referential pointers.
/// Slices are computed on access from the mmap base pointer.
pub struct MmapDict {
    mmap: Mmap,
    // Offsets into mmap (populated from header)
    base_off: usize,
    check_off: usize,
    values_off: usize,
    groups_off: usize,
    entries_off: usize,
    node_count: usize,
    entry_count: usize,
    group_count: usize,
}

impl MmapDict {
    /// Open a binary dictionary file via mmap.
    ///
    /// Returns `None` if the file doesn't exist or the format is invalid.
    pub fn open<P: AsRef<Path>>(path: P) -> Option<Self> {
        let file = File::open(path.as_ref()).ok()?;
        let mmap = unsafe { Mmap::map(&file).ok()? };

        if mmap.len() < 48 {
            return None;
        }

        // Validate magic
        if &mmap[0..8] != MAGIC {
            log::warn!("Invalid magic in dictionary file");
            return None;
        }

        // Parse header (little-endian)
        let version = u32::from_le_bytes(mmap[8..12].try_into().ok()?);
        if version != 1 {
            log::warn!("Unsupported dictionary version: {version}");
            return None;
        }

        let base_off = u32::from_le_bytes(mmap[12..16].try_into().ok()?) as usize;
        let check_off = u32::from_le_bytes(mmap[16..20].try_into().ok()?) as usize;
        let values_off = u32::from_le_bytes(mmap[20..24].try_into().ok()?) as usize;
        let groups_off = u32::from_le_bytes(mmap[24..28].try_into().ok()?) as usize;
        let entries_off = u32::from_le_bytes(mmap[28..32].try_into().ok()?) as usize;
        let node_count = u32::from_le_bytes(mmap[32..36].try_into().ok()?) as usize;
        let entry_count = u32::from_le_bytes(mmap[36..40].try_into().ok()?) as usize;
        let group_count = u32::from_le_bytes(mmap[40..44].try_into().ok()?) as usize;

        Some(Self {
            mmap,
            base_off,
            check_off,
            values_off,
            groups_off,
            entries_off,
            node_count,
            entry_count,
            group_count,
        })
    }

    /// Get the base array as a slice.
    #[inline]
    fn base(&self) -> &[i32] {
        unsafe {
            let ptr = self.mmap.as_ptr().add(self.base_off) as *const i32;
            std::slice::from_raw_parts(ptr, self.node_count)
        }
    }

    /// Get the check array as a slice.
    #[inline]
    fn check(&self) -> &[i32] {
        unsafe {
            let ptr = self.mmap.as_ptr().add(self.check_off) as *const i32;
            std::slice::from_raw_parts(ptr, self.node_count)
        }
    }

    /// Get the values array as a slice.
    #[inline]
    fn values(&self) -> &[i32] {
        unsafe {
            let ptr = self.mmap.as_ptr().add(self.values_off) as *const i32;
            std::slice::from_raw_parts(ptr, self.node_count)
        }
    }

    /// Get the group offsets array as a slice.
    #[inline]
    fn group_offsets(&self) -> &[u32] {
        unsafe {
            let ptr = self.mmap.as_ptr().add(self.groups_off) as *const u32;
            std::slice::from_raw_parts(ptr, self.group_count + 1)
        }
    }

    /// Get the entry data as a slice.
    #[inline]
    fn entry_data(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                self.mmap.as_ptr().add(self.entries_off),
                self.mmap.len() - self.entries_off,
            )
        }
    }

    /// Look up entries by pinyin key (space-separated, e.g. "shu1 ru4 fa3").
    pub fn lookup(&self, pinyin_key: &str) -> Option<Vec<DictEntry>> {
        let tid = self.search(pinyin_key)?;
        Some(self.read_group(tid))
    }

    /// Search DAT for a key, return terminal_id.
    fn search(&self, key: &str) -> Option<u32> {
        let base = self.base();
        let check = self.check();
        let values = self.values();

        dict_log(&format!(
            "search('{}'): node_count={}, base[0]={}, check[0]={}",
            key, self.node_count, base[0], check[0]
        ));

        let mut state = 0usize;
        for &b in key.as_bytes() {
            let code = char_code(b);
            if code == 0 {
                dict_log(&format!("search('{}'): char_code=0 for byte 0x{:02x}", key, b));
                return None;
            }
            if state >= self.node_count {
                dict_log(&format!("search('{}'): state {} >= node_count {}", key, state, self.node_count));
                return None;
            }
            let next = (base[state].unsigned_abs() as usize) + (code as usize);
            if next >= self.node_count {
                dict_log(&format!("search('{}'): next {} >= node_count {}", key, next, self.node_count));
                return None;
            }
            if check[next] as usize != state {
                dict_log(&format!(
                    "search('{}'): check[{}]={} != state {}",
                    key, next, check[next], state
                ));
                return None;
            }
            state = next;
        }
        if state < self.node_count && base[state] < 0 {
            let tid = values.get(state).copied().unwrap_or(-1);
            dict_log(&format!("search('{}'): terminal state={}, tid={}", key, state, tid));
            if tid >= 0 {
                return Some(tid as u32);
            }
        } else {
            dict_log(&format!(
                "search('{}'): not terminal — state={}, base[state]={}",
                key, state, base[state]
            ));
        }
        None
    }

    /// Read all entries for a terminal group.
    fn read_group(&self, tid: u32) -> Vec<DictEntry> {
        let tid = tid as usize;
        if tid >= self.group_count {
            return Vec::new();
        }
        let group_offsets = self.group_offsets();
        let start = group_offsets[tid] as usize;
        let end = group_offsets[tid + 1] as usize;
        if start >= end || end > self.entry_data().len() {
            return Vec::new();
        }

        let entry_data = self.entry_data();
        let data = &entry_data[start..end];
        parse_entries(data)
    }

    pub fn entry_count(&self) -> usize {
        self.entry_count
    }

    pub fn node_count(&self) -> usize {
        self.node_count
    }

    pub fn group_count(&self) -> usize {
        self.group_count
    }

    pub fn is_empty(&self) -> bool {
        self.entry_count == 0
    }
}

impl crate::DictSource for MmapDict {
    fn lookup(&self, pinyin: &str) -> Option<Vec<crate::DictEntry>> {
        self.lookup(pinyin)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Map byte to character code (1-37).
fn char_code(b: u8) -> usize {
    match b {
        b'a'..=b'z' => (b - b'a' + 1) as usize,
        b' ' => 27,
        b'0'..=b'9' => (b - b'0' + 28) as usize,
        _ => 0,
    }
}

/// Parse entries from binary group data.
fn parse_entries(data: &[u8]) -> Vec<DictEntry> {
    if data.len() < 4 {
        return Vec::new();
    }
    let count = u32::from_le_bytes(data[0..4].try_into().unwrap_or([0; 4])) as usize;
    if count == 0 || count > 1000 {
        // Sanity check: max 1000 entries per key
        return Vec::new();
    }

    let mut entries = Vec::with_capacity(count);
    let mut offset = 4usize;

    for _ in 0..count {
        // Read null-terminated text
        let text_start = offset;
        while offset < data.len() && data[offset] != 0 {
            offset += 1;
        }
        if offset >= data.len() {
            break;
        }
        let text =
            std::str::from_utf8(&data[text_start..offset]).unwrap_or("").to_string();
        offset += 1; // skip null

        // Read frequency (u32 LE)
        if offset + 4 > data.len() {
            break;
        }
        let frequency = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap_or([0; 4]));
        offset += 4;

        // Read weight (i32 LE)
        if offset + 4 > data.len() {
            break;
        }
        let weight = i32::from_le_bytes(data[offset..offset + 4].try_into().unwrap_or([0; 4]));
        offset += 4;

        entries.push(DictEntry {
            text,
            pinyin: Vec::new(), // filled in by caller
            frequency,
            weight,
            is_user: false,
            updated_at: 0,
        });
    }

    entries
}

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
    fn test_parse_entries_single() {
        // Build: count=1, text="测试\0", freq=100, weight=0
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_le_bytes()); // count
        data.extend_from_slice("测试\0".as_bytes()); // text
        data.extend_from_slice(&100u32.to_le_bytes()); // frequency
        data.extend_from_slice(&0i32.to_le_bytes()); // weight

        let entries = parse_entries(&data);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "测试");
        assert_eq!(entries[0].frequency, 100);
        assert_eq!(entries[0].weight, 0);
    }

    #[test]
    fn test_parse_entries_multiple() {
        let mut data = Vec::new();
        data.extend_from_slice(&2u32.to_le_bytes());
        // Entry 1
        data.extend_from_slice("输入法\0".as_bytes());
        data.extend_from_slice(&50u32.to_le_bytes());
        data.extend_from_slice(&10i32.to_le_bytes());
        // Entry 2
        data.extend_from_slice("舒服\0".as_bytes());
        data.extend_from_slice(&20u32.to_le_bytes());
        data.extend_from_slice(&5i32.to_le_bytes());

        let entries = parse_entries(&data);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "输入法");
        assert_eq!(entries[1].text, "舒服");
        assert_eq!(entries[0].frequency, 50);
        assert_eq!(entries[1].weight, 5);
    }

    #[test]
    fn test_mmap_lookup_real_dict() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/base.dict");
        if !path.exists() {
            eprintln!("Real dictionary not found at {:?}, skipping", path);
            return;
        }
        let dict = MmapDict::open(&path).expect("failed to open real dictionary");
        assert_eq!(dict.entry_count(), 99999);

        // "a" should have 5 entries
        let entries = dict.lookup("a").expect("'a' should return entries");
        assert_eq!(entries.len(), 5, "expected 5 entries for 'a'");
        assert_eq!(entries[0].text, "啊");

        // "ni" should have 25 entries
        let entries = dict.lookup("ni").expect("'ni' should return entries");
        assert_eq!(entries.len(), 25, "expected 25 entries for 'ni'");
        assert_eq!(entries[0].text, "你");

        // "shi" should have 91 entries
        let entries = dict.lookup("shi").expect("'shi' should return entries");
        assert_eq!(entries.len(), 91, "expected 91 entries for 'shi'");
        assert_eq!(entries[0].text, "是");

        // "f" is not a valid syllable — should return None
        assert!(dict.lookup("f").is_none(), "'f' should not be a terminal node");
    }

    #[test]
    fn test_missing_file() {
        let dict = MmapDict::open("/tmp/nonexistent.dict");
        assert!(dict.is_none());
    }

    #[test]
    fn test_open_real_file() {
        // Use the test file created by dict-compiler
        let path = "/tmp/test_base.dict";
        if !std::path::Path::new(path).exists() {
            eprintln!("Test file not found, skipping");
            return;
        }
        let dict = MmapDict::open(path);
        assert!(dict.is_some());
        let dict = dict.unwrap();
        assert!(!dict.is_empty());
        assert_eq!(dict.entry_count(), 8);

        // Test lookups
        let entries = dict.lookup("ce4 shi4");
        assert!(entries.is_some());
        assert_eq!(entries.unwrap()[0].text, "测试");

        let entries = dict.lookup("shu1 ru4 fa3");
        assert!(entries.is_some());
        assert_eq!(entries.unwrap()[0].text, "输入法");

        // Missing key
        assert!(dict.lookup("bucunzai").is_none());
    }
}
