#!/usr/bin/env python3
"""
Generate bigram frequency data for inputd from jieba dictionary.

Strategy:
  1. Segment multi-character words (4+ chars) into known sub-words, count adjacency
  2. Pair top-5k common words with grammatical particles
  3. Filter by minimum frequency

Output format: prev_word next_word frequency
"""

import sys
from collections import Counter


def is_chinese(text):
    return all("一" <= c <= "鿿" for c in text)


def load_jieba_dict(path):
    entries = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            parts = line.strip().split()
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
    entries.sort(key=lambda x: x[1], reverse=True)
    return entries


def segment_word(word, known_words):
    """Greedy longest-match segmentation of a Chinese word into known sub-words."""
    if len(word) < 3:
        return []
    result = []
    i = 0
    while i < len(word):
        best = None
        for j in range(min(len(word), i + 4), i + 1, -1):
            sub = word[i:j]
            if sub in known_words and len(sub) >= 2:
                best = sub
                break
        if best:
            result.append(best)
            i += len(best)
        else:
            i += 1
    return result if len(result) >= 2 else []


def main():
    jieba_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/jieba_dict.txt"
    output_path = sys.argv[2] if len(sys.argv) > 2 else None
    top_n = int(sys.argv[3]) if len(sys.argv) > 3 else 50000

    print(f"Loading jieba dict...", file=sys.stderr)
    all_entries = load_jieba_dict(jieba_path)
    entries = all_entries[:top_n]
    known_words = set(w for w, _ in all_entries[:30000])
    print(f"Loaded {len(entries)} words, {len(known_words)} known words", file=sys.stderr)

    bigrams = Counter()

    # --- Source 1: Decompose multi-char words ---
    count = 0
    for word, freq in entries:
        if len(word) < 4:
            continue
        sub = segment_word(word, known_words)
        if len(sub) >= 2:
            for i in range(len(sub) - 1):
                bigrams[(sub[i], sub[i + 1])] += freq
            count += 1
    print(f"  Decomposed {count} words into bigrams", file=sys.stderr)

    # --- Source 2: Common particles with top words ---
    # Particles that commonly PRECEDE  or FOLLOW content words
    post_particles = ["的", "了", "吗", "呢", "吧", "啊", "嘛", "呀"]   # after word
    pre_particles = ["在", "是", "和", "与", "或", "对", "从", "到", "被", "把", "让", "用", "为", "给"]  # before word
    suffixes = ["们", "性", "化", "家", "者", "员", "子", "头", "儿"]  # word suffix

    top_words = entries[:5000]  # Only use top 5k for quality
    for word, freq in top_words:
        if len(word) < 2:
            continue
        for p in post_particles:
            bigrams[(word, p)] += freq // 100 + 1
        for p in pre_particles:
            bigrams[(p, word)] += freq // 100 + 1
    print(f"  Added particle combinations for {len(top_words)} words", file=sys.stderr)

    # --- Filter ---
    min_count = max(5, len(entries) // 5000)
    filtered = [(w1, w2, c) for (w1, w2), c in bigrams.items() if c >= min_count]
    filtered.sort(key=lambda x: x[2], reverse=True)

    out = open(output_path, "w", encoding="utf-8") if output_path else sys.stdout
    with out:
        out.write(f"# Bigram data for inputd — {len(filtered)} entries\n")
        for w1, w2, c in filtered:
            out.write(f"{w1} {w2} {c}\n")

    print(f"Wrote {len(filtered)} bigrams (min_count={min_count})", file=sys.stderr)


if __name__ == "__main__":
    main()
