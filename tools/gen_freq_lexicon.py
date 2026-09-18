#!/usr/bin/env python3
"""Generate screen_context/freq_lexicon.txt from wordfreq.

Languages match GladiaFlow EUROPEAN_LANGUAGES (see src/lib/languages.ts).

- Core OCR/UI langs (en, fr, es, de, it, pt): zipf >= 3.0 (catches UI verbs).
- Other European langs: top 15k types (keeps the embed size bounded).

Do not hand-edit the output file. Re-run this script to refresh.
Requires: pip install wordfreq
"""
from __future__ import annotations

import unicodedata
from pathlib import Path

from wordfreq import iter_wordlist, top_n_list, zipf_frequency

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "src-tauri/src/screen_context/freq_lexicon.txt"
MIN_ZIPF_CORE = 3.0
TOP_N_OTHER = 15_000

# App language codes → wordfreq codes (Norwegian Bokmål = nb).
CORE_ZIPF_LANGS: list[tuple[str, str]] = [
    ("en", "en"),
    ("es", "es"),
    ("fr", "fr"),
    ("de", "de"),
    ("it", "it"),
    ("pt", "pt"),
]
TOP_N_LANGS: list[tuple[str, str]] = [
    ("nl", "nl"),
    ("pl", "pl"),
    ("cs", "cs"),
    ("da", "da"),
    ("sv", "sv"),
    ("no", "nb"),
    ("fi", "fi"),
    ("ro", "ro"),
    ("hu", "hu"),
    ("el", "el"),
    ("tr", "tr"),
    ("uk", "uk"),
    ("ru", "ru"),
]


def fold(s: str) -> str:
    s = s.lower()
    s = unicodedata.normalize("NFD", s)
    return "".join(c for c in s if unicodedata.category(c) != "Mn")


def add_word(words: set[str], w: str) -> None:
    if not w.isalpha() or not (2 <= len(w) <= 32):
        return
    words.add(w.lower())
    words.add(fold(w))


def main() -> None:
    words: set[str] = set()
    for app_code, wf_code in CORE_ZIPF_LANGS:
        kept = 0
        for w in iter_wordlist(wf_code, wordlist="best"):
            if zipf_frequency(w, wf_code) < MIN_ZIPF_CORE:
                continue
            before = len(words)
            add_word(words, w)
            if len(words) > before:
                kept += 1
        print(f"{app_code} (zipf>={MIN_ZIPF_CORE}): ~{kept} new")

    for app_code, wf_code in TOP_N_LANGS:
        kept = 0
        for w in top_n_list(wf_code, TOP_N_OTHER):
            before = len(words)
            add_word(words, w.strip())
            if len(words) > before:
                kept += 1
        print(f"{app_code} (top {TOP_N_OTHER}): {kept} new")

    all_codes = ",".join(c for c, _ in CORE_ZIPF_LANGS + TOP_N_LANGS)
    body = [
        f"# AUTO-GENERATED from wordfreq ({all_codes}).",
        f"# Core en/es/fr/de/it/pt: zipf>={MIN_ZIPF_CORE}; others: top {TOP_N_OTHER}.",
        "# Do not edit by hand. Regenerate: python3 tools/gen_freq_lexicon.py",
        *[w for w in sorted(words) if w.isalpha() and 2 <= len(w) <= 32],
    ]
    OUT.write_text("\n".join(body) + "\n", encoding="utf-8")
    print(f"wrote {len(body) - 3} entries → {OUT}")


if __name__ == "__main__":
    main()
