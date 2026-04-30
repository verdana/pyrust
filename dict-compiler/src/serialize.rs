use std::collections::HashMap;
use std::io::Write;

use crate::dat_builder::{dat_search, DatBuilder};
use crate::parser::RawEntry;

/// Magic bytes: "INPUTDCT" (8 bytes)
const MAGIC: &[u8; 8] = b"INPUTDCT";
const VERSION: u32 = 1;

/// Serialized dictionary file header (48 bytes total).
#[repr(C)]
struct Header {
    magic: [u8; 8],        // 0-7
    version: u32,          // 8-11
    base_offset: u32,      // 12-15
    check_offset: u32,     // 16-19
    values_offset: u32,    // 20-23
    groups_offset: u32,    // 24-27
    entries_offset: u32,   // 28-31
    node_count: u32,       // 32-35
    entry_count: u32,      // 36-39
    group_count: u32,      // 40-43
    _reserved: [u8; 4],   // 44-47
}

/// Build a binary dictionary file from parsed entries.
///
/// File layout:
/// ```text
/// [Header: 48 bytes]
/// [Base array: node_count × 4 bytes]
/// [Check array: node_count × 4 bytes]
/// [Values array: node_count × 4 bytes]  // -1 = not terminal, >=0 = terminal_id
/// [Group offsets: (group_count + 1) × 4 bytes]  // byte offset into entries_data
/// [Entries data: variable]
///   For each group:
///     entry_count: u32
///     entries[entry_count]:
///       text: null-terminated UTF-8
///       frequency: u32 LE
///       weight: i32 LE
/// ```
pub fn serialize_to_file(
    entries: &[RawEntry],
    output_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // --- Step 1: Group entries by pinyin key ---
    let mut by_pinyin: HashMap<String, Vec<&RawEntry>> = HashMap::new();
    for entry in entries {
        let key = entry.pinyin.join(" ");
        by_pinyin.entry(key).or_default().push(entry);
    }

    println!(
        "  Grouped {} entries into {} unique pinyin keys",
        entries.len(),
        by_pinyin.len()
    );

    // --- Step 2: Build DAT from unique pinyin keys ---
    let mut builder = DatBuilder::new();
    let mut key_to_tid: HashMap<String, u32> = HashMap::new();
    let mut tid_to_key: Vec<String> = Vec::new();

    for (key, _) in &by_pinyin {
        let tid = builder.insert(key);
        key_to_tid.insert(key.clone(), tid);
        if tid as usize >= tid_to_key.len() {
            tid_to_key.resize(tid as usize + 1, String::new());
        }
        tid_to_key[tid as usize] = key.clone();
    }

    let group_count = tid_to_key.len();
    println!("  Built DAT for {group_count} unique keys");

    let (base, check, values) = builder.build();

    // --- Step 3: Serialize entries into byte buffer ---
    let mut entries_buf: Vec<u8> = Vec::new();
    // group_byte_offsets[i] = byte start of group i in entries_buf
    let mut group_byte_offsets: Vec<u32> = Vec::with_capacity(group_count + 1);

    for tid in 0..group_count {
        group_byte_offsets.push(entries_buf.len() as u32);

        let key = &tid_to_key[tid];
        let group_entries = &by_pinyin[key];
        let count = group_entries.len() as u32;

        entries_buf.extend_from_slice(&count.to_le_bytes());

        for entry in group_entries {
            // text: null-terminated UTF-8
            entries_buf.extend_from_slice(entry.text.as_bytes());
            entries_buf.push(0);
            // frequency: u32 LE
            entries_buf.extend_from_slice(&entry.frequency.to_le_bytes());
            // weight: i32 LE
            entries_buf.extend_from_slice(&entry.weight.to_le_bytes());
        }
    }
    group_byte_offsets.push(entries_buf.len() as u32);

    // --- Step 4: Verify DAT integrity ---
    for tid in 0..group_count {
        let key = &tid_to_key[tid];
        let result = dat_search(key, &base, &check, &values);
        assert!(
            result.is_some(),
            "DAT integrity check failed: key '{key}' (tid={tid}) not found"
        );
        assert_eq!(
            result.unwrap().1 as usize, tid,
            "DAT integrity check: tid mismatch for '{key}'"
        );
    }
    println!("  DAT integrity check passed for {group_count} keys");

    // --- Step 5: Write file ---
    let total_nodes = base.len() as u32;
    let header_size = 48u32;

    let base_offset = header_size;
    let check_offset = base_offset + total_nodes * 4;
    let values_offset = check_offset + total_nodes * 4;
    let groups_offset = values_offset + total_nodes * 4;
    let entries_offset = groups_offset + (group_count as u32 + 1) * 4;

    let header = Header {
        magic: *MAGIC,
        version: VERSION,
        base_offset,
        check_offset,
        values_offset,
        groups_offset,
        entries_offset,
        node_count: total_nodes,
        entry_count: entries.len() as u32,
        group_count: group_count as u32,
        _reserved: [0u8; 4],
    };

    let mut file = std::fs::File::create(output_path)?;

    // Write header
    write_header(&mut file, &header)?;

    // Write base array
    for &v in &base {
        file.write_all(&v.to_le_bytes())?;
    }

    // Write check array
    for &v in &check {
        file.write_all(&v.to_le_bytes())?;
    }

    // Write values array
    for &v in &values {
        file.write_all(&v.to_le_bytes())?;
    }

    // Write group offsets
    for &off in &group_byte_offsets {
        file.write_all(&off.to_le_bytes())?;
    }

    // Write entries data
    file.write_all(&entries_buf)?;

    println!(
        "  Wrote {} bytes to '{}'",
        header_size as usize
            + base.len() * 4
            + check.len() * 4
            + values.len() * 4
            + group_byte_offsets.len() * 4
            + entries_buf.len(),
        output_path
    );

    Ok(())
}

fn write_header(file: &mut std::fs::File, header: &Header) -> Result<(), std::io::Error> {
    file.write_all(&header.magic)?;
    file.write_all(&header.version.to_le_bytes())?;
    file.write_all(&header.base_offset.to_le_bytes())?;
    file.write_all(&header.check_offset.to_le_bytes())?;
    file.write_all(&header.values_offset.to_le_bytes())?;
    file.write_all(&header.groups_offset.to_le_bytes())?;
    file.write_all(&header.entries_offset.to_le_bytes())?;
    file.write_all(&header.node_count.to_le_bytes())?;
    file.write_all(&header.entry_count.to_le_bytes())?;
    file.write_all(&header.group_count.to_le_bytes())?;
    file.write_all(&header._reserved)?;
    Ok(())
}
