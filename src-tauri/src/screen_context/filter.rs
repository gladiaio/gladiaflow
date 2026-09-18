//! Pure on-screen text → vocabulary term extraction (no Accessibility I/O).
//!
//! Rejection is lexicon-driven, not a hand-maintained chrome list:
//! - [`stop-words`] crate (ISO/NLTK) for GladiaFlow European languages
//! - [`freq_lexicon.txt`] auto-generated from `wordfreq` (see `tools/gen_freq_lexicon.py`)
//! Structural rules only for OCR junk (apostrophes, code identifiers, digit
//! prefixes, password-like secrets, near-lexicon misspellings).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// High-frequency EN+FR types from wordfreq — AUTO-GENERATED, do not hand-edit.
const FREQ_LEXICON: &str = include_str!("freq_lexicon.txt");

/// Tunables for the capitalisation + dictionary filter.
#[derive(Debug, Clone)]
pub struct FilterConfig {
    pub max_terms: usize,
    pub min_acronym_len: usize,
    pub min_word_len: usize,
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            max_terms: 40,
            // Drop 2-letter OCR crumbs (IB, TC) — keep API/SRE-style (3+).
            min_acronym_len: 3,
            min_word_len: 3,
        }
    }
}

/// Stop-words only (no frequency lexicon) — used to reject "The Commarieu" bigrams.
fn stopword_set() -> &'static HashSet<String> {
    static SET: OnceLock<HashSet<String>> = OnceLock::new();
    SET.get_or_init(|| {
        let mut set: HashSet<String> = HashSet::with_capacity(8_000);
        for lang in [
            "en", "es", "fr", "de", "it", "pt", "nl", "pl", "cs", "da", "sv", "no", "fi", "ro",
            "hu", "el", "tr", "uk", "ru",
        ] {
            let words = std::panic::catch_unwind(|| stop_words::get(lang)).ok();
            let Some(words) = words else {
                continue;
            };
            for w in words {
                set.insert(normalize_for_dict(w));
            }
        }
        set
    })
}

fn is_stopword(normalized: &str) -> bool {
    stopword_set().contains(normalized)
}

/// Letter-ish token that can appear in a person-name bigram (`Laurent`, `Commarieu`).
fn is_person_name_part(token: &str) -> bool {
    if is_noise_token(token) || looks_like_secret(token) || is_lower_camel_identifier(token) {
        return false;
    }
    if is_all_caps_acronym(token) {
        return false;
    }
    let alpha_len = token.chars().filter(|c| c.is_alphabetic()).count();
    if !(3..=24).contains(&alpha_len) {
        return false;
    }
    token
        .chars()
        .all(|c| c.is_alphabetic() || c == '-')
}

/// Title Case / hyphenated name casing (`Laurent`, `Jean-Louis`).
fn is_title_case_name(token: &str) -> bool {
    let parts: Vec<&str> = token.split('-').collect();
    if parts.is_empty() {
        return false;
    }
    parts.iter().all(|part| {
        let mut chars = part.chars();
        match chars.next() {
            Some(first) if first.is_uppercase() => chars.all(|c| c.is_lowercase()),
            _ => false,
        }
    })
}

/// Union of stop-words (GladiaFlow European languages) and the wordfreq lexicon.
fn common_word_set() -> &'static HashSet<String> {
    static SET: OnceLock<HashSet<String>> = OnceLock::new();
    SET.get_or_init(|| {
        let mut set: HashSet<String> = HashSet::with_capacity(400_000);
        // Same set as src/lib/languages.ts (ISO 639-1). Skip unknown codes safely.
        for lang in [
            "en", "es", "fr", "de", "it", "pt", "nl", "pl", "cs", "da", "sv", "no", "fi", "ro",
            "hu", "el", "tr", "uk", "ru",
        ] {
            let words = std::panic::catch_unwind(|| stop_words::get(lang)).ok();
            let Some(words) = words else {
                log::warn!("[screen_context] stop-words missing language code={lang}");
                continue;
            };
            for w in words {
                set.insert(normalize_for_dict(w));
            }
        }
        for line in FREQ_LEXICON.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            set.insert(normalize_for_dict(line));
        }
        set
    })
}

/// Lowercase + strip combining marks so `Québec` matches `quebec` in the dict.
fn normalize_for_dict(input: &str) -> String {
    let lower: String = input.chars().flat_map(char::to_lowercase).collect();
    strip_diacritics(&lower)
}

fn strip_diacritics(input: &str) -> String {
    // Minimal NFD-style strip without pulling in unicode-normalization:
    // replace common Latin accented letters, drop leftover combining marks.
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => out.push('a'),
            'ç' => out.push('c'),
            'è' | 'é' | 'ê' | 'ë' => out.push('e'),
            'ì' | 'í' | 'î' | 'ï' => out.push('i'),
            'ñ' => out.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => out.push('o'),
            'ù' | 'ú' | 'û' | 'ü' => out.push('u'),
            'ý' | 'ÿ' => out.push('y'),
            'æ' => out.push_str("ae"),
            'œ' => out.push_str("oe"),
            c if (('\u{0300}'..='\u{036f}').contains(&c)) => {}
            c => out.push(c),
        }
    }
    out
}

fn has_significant_capital(token: &str) -> bool {
    token.chars().any(|c| c.is_uppercase())
}

fn is_all_caps_acronym(token: &str) -> bool {
    let letters: Vec<char> = token.chars().filter(|c| c.is_alphabetic()).collect();
    letters.len() >= 2 && letters.iter().all(|c| c.is_uppercase())
}

/// CamelCase identifier (JS-style): starts lowercase, has internal capital
/// (`setTimeout`, `screenContextDebugTimerRef`). These are code, not vocab.
fn is_lower_camel_identifier(token: &str) -> bool {
    let chars: Vec<char> = token.chars().filter(|c| c.is_alphabetic()).collect();
    if chars.len() < 3 {
        return false;
    }
    chars[0].is_lowercase() && chars.iter().skip(1).any(|c| c.is_uppercase())
}

/// PascalCase product/name: starts uppercase, mixed case with internal capital
/// (`GladiaFlow`, `OpenPII`).
fn is_pascal_product(token: &str) -> bool {
    let chars: Vec<char> = token.chars().filter(|c| c.is_alphabetic()).collect();
    if chars.len() < 3 {
        return false;
    }
    let has_lower = chars.iter().any(|c| c.is_lowercase());
    chars[0].is_uppercase() && has_lower && chars.iter().skip(1).any(|c| c.is_uppercase())
}

/// Password / API-token shape: long pure ASCII alnum with mixed case **and**
/// digits interleaved (not just a trailing year like `MacBookPro16`).
/// Example: `KhbBlxlL5yff3R5y`.
fn looks_like_secret(token: &str) -> bool {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() < 12 {
        return false;
    }
    if !chars.iter().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    let has_upper = chars.iter().any(|c| c.is_ascii_uppercase());
    let has_lower = chars.iter().any(|c| c.is_ascii_lowercase());
    let has_digit = chars.iter().any(|c| c.is_ascii_digit());
    if !(has_upper && has_lower && has_digit) {
        return false;
    }
    // Trailing digit run only (product + year/size) → keep; mid-token digits → secret.
    let trailing_digits = chars.iter().rev().take_while(|c| c.is_ascii_digit()).count();
    chars
        .iter()
        .take(chars.len() - trailing_digits)
        .any(|c| c.is_ascii_digit())
}

fn is_noise_token(token: &str) -> bool {
    // Full emails are harvested separately; bare @ / URL fragments stay noise.
    if token.contains('@') || token.contains("://") || token.starts_with("www.") {
        return true;
    }
    let alnum = token.chars().filter(|c| c.is_alphanumeric()).count();
    if alnum == 0 {
        return true;
    }
    // Pure digits / mostly numeric ids.
    if token.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',') {
        return true;
    }
    // OCR speech crumbs: "J'ai", "You're", "C'est", "L'employé", "Slack's".
    if token.contains('\'') || token.contains('’') {
        return true;
    }
    // "Veux-tu", "Dis-moi" — hyphenated pronoun tags, not names.
    if is_hyphenated_clitic(token) {
        return true;
    }
    // Folder crumbs like "01-Finances" from Finder/Slack file trees.
    if token
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit())
    {
        return true;
    }
    false
}

/// `Veux-tu` / `Dis-moi` style OCR — second half is a clitic/pronoun.
fn is_hyphenated_clitic(token: &str) -> bool {
    let Some((left, right)) = token.split_once('-') else {
        return false;
    };
    if left.is_empty() || right.is_empty() || token.matches('-').count() != 1 {
        return false;
    }
    const CLITICS: &[&str] = &[
        "tu", "je", "il", "elle", "on", "nous", "vous", "ils", "elles", "me", "te", "se",
        "le", "la", "les", "lui", "y", "en", "moi", "toi", "soi", "ce", "ci", "la",
    ];
    let r = normalize_for_dict(right);
    CLITICS.contains(&r.as_str())
}

/// Practical email matcher: `local@domain.tld` (allows `.`, `_`, `+`, `-` in local).
fn is_email_candidate(s: &str) -> bool {
    let s = s.trim_matches(|c: char| !c.is_alphanumeric() && c != '@' && c != '.' && c != '_' && c != '+' && c != '-');
    let Some((local, domain)) = s.split_once('@') else {
        return false;
    };
    if local.is_empty() || domain.is_empty() || local.len() > 64 || domain.len() > 253 {
        return false;
    }
    if !local
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
    {
        return false;
    }
    if local.starts_with('.') || local.ends_with('.') || local.contains("..") {
        return false;
    }
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    let tld = labels.last().copied().unwrap_or("");
    if tld.len() < 2 || !tld.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    labels.iter().all(|label| {
        !label.is_empty()
            && label
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    })
}

/// Pull `user@domain.tld` spans out of raw UI text (before word tokenisation).
fn extract_emails(text: &str) -> Vec<String> {
    let mut emails = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '@' {
            i += 1;
            continue;
        }
        // Walk left for local-part.
        let mut start = i;
        while start > 0 {
            let c = chars[start - 1];
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-') {
                start -= 1;
            } else {
                break;
            }
        }
        // Walk right for domain.
        let mut end = i + 1;
        while end < chars.len() {
            let c = chars[end];
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                end += 1;
            } else {
                break;
            }
        }
        if start < i && end > i + 1 {
            let candidate: String = chars[start..end].iter().collect();
            if is_email_candidate(&candidate) {
                emails.push(candidate);
            }
        }
        i = end.max(i + 1);
    }
    emails
}

fn known_file_extension(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        // docs / office
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "csv" | "tsv" | "txt" | "md"
            | "rtf" | "odt" | "ods" | "odp" | "pages" | "numbers" | "key" | "epub" | "mobi"
        // code / config
            | "json" | "yaml" | "yml" | "toml" | "xml" | "html" | "htm" | "css" | "scss" | "sass"
            | "less" | "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "py" | "rb" | "go" | "rs"
            | "java" | "kt" | "kts" | "swift" | "m" | "mm" | "c" | "cc" | "cpp" | "cxx" | "h"
            | "hh" | "hpp" | "cs" | "php" | "sql" | "sh" | "bash" | "zsh" | "fish" | "ps1"
            | "bat" | "cmd" | "vue" | "svelte" | "astro" | "graphql" | "gql" | "proto" | "prisma"
            | "tf" | "hcl" | "ini" | "cfg" | "conf" | "env" | "lock" | "plist" | "gradle" | "cmake"
            | "makefile" | "ipynb" | "rmd" | "tex" | "bib" | "r" | "jl" | "scala" | "clj" | "ex"
            | "exs" | "erl" | "hs" | "lua" | "pl" | "pm" | "dart" | "zig" | "nim" | "v" | "sv"
        // media
            | "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "icns" | "heic" | "tif"
            | "tiff" | "bmp" | "psd" | "ai" | "fig" | "mp3" | "mp4" | "m4a" | "aac" | "wav"
            | "flac" | "ogg" | "mov" | "avi" | "mkv" | "webm" | "m4v"
        // archives / packages
            | "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" | "dmg" | "pkg" | "ipa"
            | "apk" | "deb" | "rpm" | "msi" | "exe" | "app" | "iso"
        // data / ml
            | "parquet" | "arrow" | "feather" | "avro" | "orc" | "db" | "sqlite" | "sqlite3"
            | "onnx" | "pt" | "pth" | "ckpt" | "h5" | "hdf5" | "npz" | "npy" | "pkl" | "pickle"
            | "safetensors" | "gguf" | "ggml" | "bin" | "dat" | "log" | "wasm" | "so" | "dylib"
            | "dll" | "a" | "o" | "class" | "jar" | "war"
        // fonts / misc
            | "ttf" | "otf" | "woff" | "woff2" | "map" | "patch" | "diff" | "gitignore" | "dockerignore"
    )
}

fn is_filename_candidate(s: &str) -> bool {
    if s.contains('@') || s.contains("://") || s.starts_with('.') || s.ends_with('.') {
        return false;
    }
    let Some((stem, ext)) = s.rsplit_once('.') else {
        return false;
    };
    if stem.is_empty() || ext.is_empty() || ext.len() > 12 {
        return false;
    }
    // Reject pure version-like `1.2` / `3.14`.
    if stem.chars().all(|c| c.is_ascii_digit()) && ext.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    // IDE source-tab noise (App.tsx, main.rs) — not useful vs Slack people/files.
    let ext_l = ext.to_ascii_lowercase();
    if matches!(
        ext_l.as_str(),
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "rs" | "go" | "java" | "kt" | "swift"
            | "vue" | "svelte" | "css" | "scss" | "sass" | "less" | "html" | "htm" | "map"
    ) {
        return false;
    }
    if !stem
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+'))
    {
        return false;
    }
    if !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    if known_file_extension(ext) {
        return true;
    }
    false
}

/// Pull `name.ext` filenames (regex-ish `*.*`) out of raw UI text.
fn extract_filenames(text: &str) -> Vec<String> {
    let mut files = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '.' {
            i += 1;
            continue;
        }
        // Walk left for stem (allow nested dots: archive.tar.gz → take last ext only via rsplit).
        let mut start = i;
        while start > 0 {
            let c = chars[start - 1];
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+') {
                start -= 1;
            } else {
                break;
            }
        }
        let mut end = i + 1;
        while end < chars.len() {
            let c = chars[end];
            if c.is_ascii_alphanumeric() {
                end += 1;
            } else {
                break;
            }
        }
        if start < i && end > i + 1 {
            let candidate: String = chars[start..end].iter().collect();
            // Skip if this span is part of an email (has @ to the left before whitespace).
            let left_ctx: String = chars[start.saturating_sub(40)..start].iter().collect();
            if left_ctx.contains('@') && !left_ctx.chars().rev().any(|c| c.is_whitespace()) {
                i = end;
                continue;
            }
            if is_filename_candidate(&candidate) {
                files.push(candidate);
            }
        }
        i = end.max(i + 1);
    }
    files
}

fn token_length_ok(token: &str, config: &FilterConfig) -> bool {
    let alpha_len = token.chars().filter(|c| c.is_alphabetic()).count();
    if is_all_caps_acronym(token) {
        alpha_len >= config.min_acronym_len
    } else {
        alpha_len >= config.min_word_len
    }
}

fn should_keep_token(token: &str, config: &FilterConfig) -> bool {
    if is_noise_token(token) || !token_length_ok(token, config) {
        return false;
    }
    // Random secrets / tokens from password fields or clipboard chrome.
    if looks_like_secret(token) {
        return false;
    }
    // JS/TS identifiers from IDE AX trees — never useful as dictation vocab.
    if is_lower_camel_identifier(token) {
        return false;
    }
    // Path-like AX chrome: Users-jlqueguiner-dev-…
    if token.matches('-').count() >= 3 {
        return false;
    }
    // Acronyms (API, SRE) — keep even when the lowercase form is a common word.
    if is_all_caps_acronym(token) {
        return true;
    }
    let normalized = normalize_for_dict(token);
    // Frequency / stopword lexicon — proper nouns are typically absent.
    // Also drop OCR/typo near-misses (`Setings` → settings, `Brouilons` → brouillon).
    if is_common_or_near(&normalized) {
        return false;
    }
    // Prefer PascalCase products / capitalised proper nouns.
    if is_pascal_product(token) {
        return true;
    }
    if has_significant_capital(token) {
        return true;
    }
    false
}

fn in_common_lexicon(normalized: &str) -> bool {
    let set = common_word_set();
    if set.contains(normalized) {
        return true;
    }
    // Light plural fold so OCR "Brouillons" / "Documents" match lexicon stems.
    if normalized.len() > 4 && normalized.ends_with('s') {
        let stem = &normalized[..normalized.len() - 1];
        if set.contains(stem) {
            return true;
        }
    }
    if normalized.len() > 5 && normalized.ends_with("es") {
        let stem = &normalized[..normalized.len() - 2];
        if set.contains(stem) {
            return true;
        }
    }
    false
}

fn is_common_or_near(normalized: &str) -> bool {
    in_common_lexicon(normalized) || near_common_lexicon(normalized)
}

/// True when `normalized` is one indel / transposition (or one substitution if
/// long) away from a lexicon word. Catches OCR typos of UI chrome without a
/// hand list. Skips short tokens so rare short names survive.
fn near_common_lexicon(normalized: &str) -> bool {
    let chars: Vec<char> = normalized.chars().collect();
    let n = chars.len();
    if n < 6 {
        return false;
    }
    // Latin OCR only — Greek/Cyrillic rely on exact lexicon membership.
    if !chars.iter().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }

    // Deletion: `Settiings` → `settings`.
    for i in 0..n {
        let variant: String = chars
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, c)| *c)
            .collect();
        if variant.len() >= 5 && in_common_lexicon(&variant) {
            return true;
        }
    }

    // Insertion: `Setings` → `settings`, `Brouilons` → `brouillons`.
    for i in 0..=n {
        for b in b'a'..=b'z' {
            let mut variant = String::with_capacity(n + 1);
            variant.extend(chars[..i].iter().copied());
            variant.push(b as char);
            variant.extend(chars[i..].iter().copied());
            if in_common_lexicon(&variant) {
                return true;
            }
        }
    }

    // Adjacent transposition: `Settigns` → `settings`.
    for i in 0..n.saturating_sub(1) {
        let mut swapped = chars.clone();
        swapped.swap(i, i + 1);
        let variant: String = swapped.into_iter().collect();
        if in_common_lexicon(&variant) {
            return true;
        }
    }

    // Substitution only for longer tokens (avoids Franco/France-style clashes).
    if n >= 8 {
        for i in 0..n {
            for b in b'a'..=b'z' {
                let ch = b as char;
                if ch == chars[i] {
                    continue;
                }
                let mut variant = String::with_capacity(n);
                variant.extend(chars[..i].iter().copied());
                variant.push(ch);
                variant.extend(chars[i + 1..].iter().copied());
                if in_common_lexicon(&variant) {
                    return true;
                }
            }
        }
    }

    false
}

/// True if a harvested / NER term should never enter Gladia custom vocabulary.
pub fn is_vocab_noise(term: &str) -> bool {
    let trimmed = term.trim();
    if trimmed.is_empty() {
        return true;
    }
    // Keep structured forms even when lowercase.
    if is_email_candidate(trimmed) {
        return false;
    }
    if extract_filenames(trimmed)
        .iter()
        .any(|f| f.eq_ignore_ascii_case(trimmed))
    {
        return false;
    }
    // Multi-word: drop if every token is in the frequency/stop lexicon.
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.len() > 1 {
        let all_noise = parts.iter().all(|p| {
            let n = normalize_for_dict(
                p.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '\''),
            );
            n.is_empty() || is_common_or_near(&n)
        });
        return all_noise;
    }
    !should_keep_token(trimmed, &FilterConfig::default())
}

/// Split raw UI text into candidate tokens (keeps internal hyphens / apostrophes).
/// Emails and URL-like spans are dropped before tokenisation.
fn tokenize(text: &str) -> Vec<String> {
    let scrubbed = scrub_emails_and_urls(text);
    let mut tokens = Vec::new();
    let mut current = String::new();

    let flush = |buf: &mut String, out: &mut Vec<String>| {
        let trimmed = buf.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
        buf.clear();
        if !trimmed.is_empty() {
            out.push(trimmed);
        }
    };

    for ch in scrubbed.chars() {
        if ch.is_alphanumeric() || ch == '\'' || ch == '-' || ch == '’' {
            current.push(ch);
        } else {
            flush(&mut current, &mut tokens);
        }
    }
    flush(&mut current, &mut tokens);
    tokens
}

fn scrub_emails_and_urls(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // URL: http(s)://... or www.
        if looks_like_url_start(&chars, i) {
            while i < chars.len() && !chars[i].is_whitespace() {
                i += 1;
            }
            out.push(' ');
            continue;
        }
        // Email: local@domain — drop local + @ + domain.
        if chars[i] == '@' {
            while !out.is_empty() && !out.ends_with(|c: char| c.is_whitespace()) {
                out.pop();
            }
            i += 1;
            while i < chars.len() && !chars[i].is_whitespace() {
                i += 1;
            }
            out.push(' ');
            continue;
        }
        // Filename `stem.ext` — drop so tokenisation does not emit bare extensions.
        if chars[i] == '.' {
            let mut start = out.len();
            while start > 0 {
                let c = out.as_bytes()[start - 1] as char;
                if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+') {
                    start -= 1;
                } else {
                    break;
                }
            }
            let mut end = i + 1;
            while end < chars.len() && chars[end].is_ascii_alphanumeric() {
                end += 1;
            }
            if start < out.len() && end > i + 1 {
                let stem: String = out[start..].to_string();
                let ext: String = chars[i + 1..end].iter().collect();
                let candidate = format!("{stem}.{ext}");
                if is_filename_candidate(&candidate) {
                    out.truncate(start);
                    out.push(' ');
                    i = end;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn looks_like_url_start(chars: &[char], i: usize) -> bool {
    let rest: String = chars[i..].iter().take(8).collect::<String>().to_lowercase();
    rest.starts_with("http://") || rest.starts_with("https://") || rest.starts_with("www.")
}

fn remember_term(
    token: String,
    counts: &mut HashMap<String, usize>,
    first_form: &mut HashMap<String, String>,
    order: &mut Vec<String>,
) {
    let key = token.to_lowercase();
    if let std::collections::hash_map::Entry::Vacant(e) = first_form.entry(key.clone()) {
        e.insert(token);
        order.push(key.clone());
    }
    *counts.entry(key).or_insert(0) += 1;
}

/// Keep `Given Surname` when at least one side is a rare proper noun.
///
/// Common given names (`Laurent`) sit in the frequency lexicon and would
/// otherwise never enter custom vocabulary, even when clearly on-screen next
/// to a rare surname (`Commarieu`).
fn harvest_person_name_bigrams(
    text: &str,
    config: &FilterConfig,
    counts: &mut HashMap<String, usize>,
    first_form: &mut HashMap<String, String>,
    order: &mut Vec<String>,
) {
    let tokens = tokenize(text);
    for pair in tokens.windows(2) {
        let a = &pair[0];
        let b = &pair[1];
        if !is_person_name_part(a) || !is_person_name_part(b) {
            continue;
        }
        let a_rare = should_keep_token(a, config);
        let b_rare = should_keep_token(b, config);
        if !a_rare && !b_rare {
            continue;
        }
        if a_rare && b_rare {
            let phrase = format!("{a} {b}");
            remember_term(phrase, counts, first_form, order);
            *counts.entry(format!("{a} {b}").to_lowercase()).or_insert(0) += 20;
            continue;
        }
        // Exactly one rare: partner must look like a given name, not "The"/"Une".
        let (rare, partner) = if a_rare { (a, b) } else { (b, a) };
        // Skip product/code tokens paired with a common word (`OpenAI Platform`).
        if is_pascal_product(rare) {
            continue;
        }
        let partner_norm = normalize_for_dict(partner);
        if is_stopword(&partner_norm) {
            continue;
        }
        let partner_ok = is_title_case_name(partner)
            || (!has_significant_capital(partner) && in_common_lexicon(&partner_norm));
        if !partner_ok {
            continue;
        }
        // Prefer Title-case display when the rare side is Title and partner is lower.
        let phrase = if is_title_case_name(rare) && !has_significant_capital(partner) {
            let mut chars = partner.chars();
            let titled = match chars.next() {
                Some(c) => {
                    let mut s = c.to_uppercase().collect::<String>();
                    s.extend(chars.flat_map(|x| x.to_lowercase()));
                    s
                }
                None => partner.clone(),
            };
            if a_rare {
                format!("{rare} {titled}")
            } else {
                format!("{titled} {rare}")
            }
        } else {
            format!("{a} {b}")
        };
        let key = phrase.to_lowercase();
        remember_term(phrase, counts, first_form, order);
        *counts.entry(key).or_insert(0) += 30;
    }
}

/// Spoken variants so Gladia can map collapsed speech (`bqdriftsync.py`) to the
/// on-screen spelling (`bq_drift_sync.py`).
pub fn pronunciations_for_term(value: &str) -> Option<Vec<String>> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    let mut variants: Vec<String> = Vec::new();

    if is_filename_candidate(value) {
        let (stem, ext) = value.rsplit_once('.').unwrap_or((value, ""));
        let spaced = stem.replace(['_', '-'], " ");
        let collapsed = stem.replace(['_', '-'], "");
        // How people say the stem.
        push_unique(&mut variants, spaced.clone());
        push_unique(&mut variants, collapsed.clone());
        // Stem + extension as spoken words.
        if !ext.is_empty() {
            push_unique(&mut variants, format!("{spaced} {ext}"));
            push_unique(&mut variants, format!("{spaced} dot {ext}"));
            push_unique(&mut variants, format!("{spaced} point {ext}"));
            push_unique(&mut variants, format!("{collapsed}{ext}"));
            push_unique(&mut variants, format!("{collapsed}.{ext}"));
            push_unique(&mut variants, format!("{collapsed} {ext}"));
            push_unique(&mut variants, format!("{collapsed} dot {ext}"));
            push_unique(&mut variants, format!("{collapsed} point {ext}"));
        }
        if stem.contains('_') {
            let underscored = stem.replace('_', " underscore ");
            push_unique(&mut variants, format!("{underscored}.{ext}"));
            push_unique(&mut variants, format!("{underscored} dot {ext}"));
        }
    } else if is_email_candidate(value) {
        if let Some((local, domain)) = value.split_once('@') {
            let spaced_local = local.replace(['.', '_', '+', '-'], " ");
            let collapsed_local = local.replace(['.', '_', '+', '-'], "");
            push_unique(&mut variants, format!("{spaced_local} at {domain}"));
            push_unique(&mut variants, format!("{collapsed_local}@{domain}"));
            push_unique(&mut variants, format!("{collapsed_local} at {domain}"));
            push_unique(
                &mut variants,
                format!("{} arobase {}", spaced_local, domain.replace('.', " point ")),
            );
        }
    } else if value.contains('_') || value.contains('-') {
        // Camel-free identifiers that slipped through (rare).
        let spaced = value.replace(['_', '-'], " ");
        let collapsed = value.replace(['_', '-'], "");
        push_unique(&mut variants, spaced);
        push_unique(&mut variants, collapsed);
    }

    // Drop trivial / identical-to-value forms.
    variants.retain(|p| {
        let t = p.trim();
        !t.is_empty() && !t.eq_ignore_ascii_case(value)
    });
    if variants.is_empty() {
        None
    } else {
        Some(variants)
    }
}

fn push_unique(out: &mut Vec<String>, s: String) {
    let trimmed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.is_empty() {
        return;
    }
    if out.iter().any(|e| e.eq_ignore_ascii_case(&trimmed)) {
        return;
    }
    out.push(trimmed);
}

/// Extract ranked vocabulary terms from raw window text.
pub fn extract_vocabulary_terms(text: &str, config: &FilterConfig) -> Vec<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut first_form: HashMap<String, String> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    // Emails first: keep the full `local@domain.tld` (capital filter would drop them).
    for email in extract_emails(text) {
        let key = email.to_lowercase();
        remember_term(email, &mut counts, &mut first_form, &mut order);
        *counts.entry(key).or_insert(0) += 50;
    }
    // Filenames `name.ext` (often all-lowercase in Finder / IDE tabs).
    for file in extract_filenames(text) {
        let key = file.to_lowercase();
        remember_term(file, &mut counts, &mut first_form, &mut order);
        *counts.entry(key).or_insert(0) += 50;
    }

    for token in tokenize(text) {
        if !should_keep_token(&token, config) {
            continue;
        }
        remember_term(token, &mut counts, &mut first_form, &mut order);
    }

    // Person-name bigrams: common first name + rare surname (`Laurent Commarieu`).
    // Alone, "Laurent" is filtered as a frequent wordfreq type — keep the pair.
    harvest_person_name_bigrams(text, config, &mut counts, &mut first_form, &mut order);

    let mut ranked: Vec<(String, usize, usize)> = order
        .into_iter()
        .enumerate()
        .map(|(idx, key)| {
            let count = *counts.get(&key).unwrap_or(&1);
            (key, count, idx)
        })
        .collect();

    // Higher frequency first; stable by first appearance.
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2)));

    ranked
        .into_iter()
        .take(config.max_terms)
        .filter_map(|(key, _, _)| first_form.remove(&key))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract(text: &str) -> Vec<String> {
        extract_vocabulary_terms(text, &FilterConfig::default())
    }

    #[test]
    fn product_name_kept() {
        assert_eq!(extract("GladiaFlow is ready"), vec!["GladiaFlow"]);
    }

    #[test]
    fn all_lowercase_sentence_yields_nothing() {
        assert!(extract("bonjour le monde").is_empty());
    }

    #[test]
    fn sentence_start_stopword_rejected() {
        assert!(extract("The microphone is on").is_empty());
    }

    #[test]
    fn french_sentence_start_rejected() {
        assert!(extract("Une réunion commence").is_empty());
    }

    #[test]
    fn acronym_kept() {
        assert_eq!(extract("Call the API now"), vec!["API"]);
    }

    #[test]
    fn camel_case_code_identifiers_rejected() {
        // Lower-camel JS identifiers from IDE AX trees must not enter vocab.
        assert!(extract("useState hook").is_empty());
        assert!(extract("setTimeout clear").is_empty());
        assert!(extract("screenContextDebugTimerRef here").is_empty());
    }

    #[test]
    fn password_like_secrets_rejected() {
        assert!(extract("KhbBlxlL5yff3R5y").is_empty());
        assert!(is_vocab_noise("KhbBlxlL5yff3R5y"));
        assert!(is_vocab_noise("aB3xK9mQ2pL7nR4"));
        // Trailing size/year on a product name is fine.
        assert!(!looks_like_secret("MacBookPro16"));
        assert!(!looks_like_secret("GladiaFlow"));
    }

    #[test]
    fn pascal_product_kept() {
        assert_eq!(extract("GladiaFlow is ready"), vec!["GladiaFlow"]);
        assert!(extract("OpenPII model").contains(&"OpenPII".to_string()));
    }

    #[test]
    fn common_capitalized_label_rejected() {
        assert!(extract("Settings").is_empty());
        assert!(extract("File Edit View").is_empty());
    }

    #[test]
    fn misspelled_common_words_rejected() {
        assert!(extract("Setings").is_empty());
        assert!(extract("Settngs").is_empty());
        assert!(extract("Settigns").is_empty());
        assert!(extract("Documets").is_empty());
        assert!(extract("Brouilons").is_empty());
        assert!(extract("Disponiblee").is_empty());
        assert!(extract("Repondree").is_empty());
        // Rare surnames must not be collapsed into nearby common words.
        assert!(extract("Visit Commarieu soon")
            .iter()
            .any(|t| t == "Commarieu"));
        assert!(!is_vocab_noise("Commarieu"));
    }

    #[test]
    fn common_given_name_with_rare_surname_kept_as_bigram() {
        // "Laurent" alone is a frequent French given name → lexicon reject.
        assert!(!extract("Laurent is here").iter().any(|t| t == "Laurent"));
        let terms = extract("laurent Commarieu — Ouvert");
        assert!(
            terms.iter().any(|t| t == "Commarieu"),
            "surname alone, got {terms:?}"
        );
        assert!(
            terms
                .iter()
                .any(|t| t.eq_ignore_ascii_case("Laurent Commarieu")),
            "full name bigram, got {terms:?}"
        );
        assert!(!is_vocab_noise("Laurent Commarieu"));
    }

    #[test]
    fn french_slack_ui_chrome_rejected() {
        let terms = extract(
            "Répondre Envoyer Fichiers Brouillons Disponible Détails Adresse Appels Trouver Desk Billing One Org",
        );
        assert!(
            terms.is_empty(),
            "expected no Slack UI chrome, got {terms:?}"
        );
    }

    #[test]
    fn proper_names_kept_among_chrome() {
        let terms = extract("Répondre Commarieu Envoyer Desk");
        assert!(terms.iter().any(|t| t == "Commarieu"));
        assert!(!terms.iter().any(|t| t.eq_ignore_ascii_case("Répondre")));
        assert!(!terms.iter().any(|t| t.eq_ignore_ascii_case("Desk")));
        // Common first names (e.g. Laurent) sit in wordfreq → filtered by design.
    }

    #[test]
    fn contractions_and_clitics_rejected() {
        assert!(extract("J'ai faim").is_empty() || !extract("J'ai").iter().any(|t| t.contains('\'')));
        assert!(is_vocab_noise("J'ai"));
        assert!(is_vocab_noise("J'aime"));
        assert!(is_vocab_noise("J'etais"));
        assert!(is_vocab_noise("You're"));
        assert!(is_vocab_noise("C'est"));
        assert!(is_vocab_noise("L'employé"));
        assert!(is_vocab_noise("Slack's"));
        assert!(is_vocab_noise("Veux-tu"));
        // Frequent lexicon (wordfreq), not a length ban:
        assert!(is_vocab_noise("Pro"));
        assert!(is_vocab_noise("Hub"));
        assert!(!is_vocab_noise("Resy"));
        assert!(!is_vocab_noise("Commarieu"));
    }

    #[test]
    fn names_still_kept() {
        let terms = extract("Commarieu and Lafaye met Jean-Louis");
        assert!(terms.iter().any(|t| t == "Commarieu"));
        assert!(terms.iter().any(|t| t == "Lafaye"));
        assert!(terms.iter().any(|t| t == "Jean-Louis"));
    }

    #[test]
    fn multi_token_keeps_proper_nouns() {
        let terms = extract("OpenAI Platform docs");
        assert!(terms.contains(&"OpenAI".to_string()));
        assert!(!terms.iter().any(|t| t.eq_ignore_ascii_case("Platform")));
    }

    #[test]
    fn hyphenated_name_kept() {
        let terms = extract("Meet Jean-Louis today");
        assert!(terms.iter().any(|t| t == "Jean-Louis"));
    }

    #[test]
    fn emails_kept_urls_skipped() {
        let terms = extract("email support@gladia.io please");
        assert!(terms.iter().any(|t| t == "support@gladia.io"));
        assert!(!terms.iter().any(|t| t.eq_ignore_ascii_case("Gladia")));

        let plus = extract("Write jean.louis+dev@Gladia.io today");
        assert!(plus.iter().any(|t| t == "jean.louis+dev@Gladia.io"));

        assert!(extract("see https://Gladia.io/docs").is_empty());
    }

    #[test]
    fn filenames_kept() {
        let terms = extract("Open meeting-notes.pdf and config.toml now");
        assert!(terms.iter().any(|t| t == "meeting-notes.pdf"));
        assert!(terms.iter().any(|t| t == "config.toml"));
        // Bare extension should not leak as its own term.
        assert!(!terms.iter().any(|t| t.eq_ignore_ascii_case("pdf")));

        let nested = extract("ship model.safetensors today");
        assert!(nested.iter().any(|t| t == "model.safetensors"));

        // Version-like decimals are not filenames.
        assert!(!extract("use version 1.2 please")
            .iter()
            .any(|t| t == "1.2"));

        let snake = extract("edit bq_drift_sync.py please");
        assert!(snake.iter().any(|t| t == "bq_drift_sync.py"));
    }

    #[test]
    fn filename_pronunciations_cover_collapsed_speech() {
        let p = pronunciations_for_term("bq_drift_sync.py").expect("pronunciations");
        assert!(p.iter().any(|s| s == "bqdriftsync.py" || s == "bqdriftsync py"));
        assert!(p.iter().any(|s| s.contains("bq drift sync")));
        assert!(p.iter().any(|s| s.contains("dot py") || s.contains("point py")));
    }

    #[test]
    fn dedupes_case_insensitive() {
        let terms = extract("GladiaFlow then gladiaflow again GladiaFlow");
        assert_eq!(terms, vec!["GladiaFlow"]);
    }

    #[test]
    fn respects_max_terms() {
        let mut text = String::new();
        for i in 0..60 {
            // Long enough Titlecase / Pascal so the stricter length gate still keeps them.
            text.push_str(&format!("ProperName{i} "));
        }
        let terms = extract_vocabulary_terms(&text, &FilterConfig {
            max_terms: 10,
            ..FilterConfig::default()
        });
        assert_eq!(terms.len(), 10);
    }

    #[test]
    fn rare_surname_kept_via_lexicon_gap() {
        // Common place names (Québec) are in wordfreq; rare surnames are not.
        let terms = extract("Visit Commarieu soon");
        assert!(terms.iter().any(|t| t == "Commarieu"));
        assert!(is_vocab_noise("Québec") || extract("Visit Québec soon").is_empty());
    }
}
