#!/usr/bin/env python3
"""Convert jieba dictionary to pyrust word list format with pinyin."""

import sys
from pypinyin import pinyin, Style

def is_chinese(text):
    """Check if text contains only Chinese characters."""
    return all('一' <= c <= '鿿' for c in text)

def main():
    jieba_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/jieba_dict.txt"
    limit = int(sys.argv[2]) if len(sys.argv) > 2 else 100_000

    # Read jieba dict: word frequency pos
    entries = []
    with open(jieba_path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            parts = line.split()
            if len(parts) < 2:
                continue
            word, freq = parts[0], parts[1]
            if not is_chinese(word):
                continue
            try:
                freq = int(freq)
            except ValueError:
                continue
            entries.append((word, freq))

    # Sort by frequency descending, take top N
    entries.sort(key=lambda x: x[1], reverse=True)
    entries = entries[:limit]

    # Convert to pinyin and output
    total = len(entries)
    skipped = 0
    for i, (word, freq) in enumerate(entries):
        # pypinyin NORMAL style: ni, hao (no tones, matching engine lookup format)
        py_list = pinyin(word, style=Style.NORMAL, heteronym=False)
        valid = True
        syllables = []
        for p in py_list:
            s = p[0]
            # Skip if pinyin contains non-ASCII (pypinyin returns original char when no mapping)
            if any(ord(c) > 127 for c in s):
                valid = False
                break
            syllables.append(s)
        if not valid:
            skipped += 1
            continue
        pinyin_str = " ".join(syllables)
        weight = 0  # default weight
        print(f"{word}\t{pinyin_str}\t{freq}\t{weight}")

        if (i + 1) % 10000 == 0:
            print(f"# Processed {i + 1}/{total}", file=sys.stderr)

    print(f"# Done: {total - skipped} entries (skipped {skipped} with unknown pinyin)", file=sys.stderr)

if __name__ == "__main__":
    main()
