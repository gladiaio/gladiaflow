//! Post-ASR rewrite using on-screen vocabulary (filenames / emails / names).
//!
//! Gladia custom vocabulary is phonetic and often:
//! - leaves underscores as the spoken word "underscore"
//! - collapses `bq_drift_sync.py` → `bqdrift.sync.py`
//! - near-misses surnames (`Comarieux` vs on-screen `Commarieu`)
//! When we know the canonical on-screen form, rewrite the transcript locally.

/// Rewrite spoken separators and map near ASR forms to screen terms.
pub fn rewrite_transcript(text: &str, screen_terms: &[String]) -> String {
    let mut out = rewrite_spoken_underscores(text);
    let mut terms: Vec<&String> = screen_terms.iter().collect();
    terms.sort_by(|a, b| b.len().cmp(&a.len()));
    for term in &terms {
        if term.len() < 5 {
            continue;
        }
        // Filenames / emails / snake_ids: alphanumeric skeleton match.
        if term.contains('.') || term.contains('@') || term.contains('_') {
            out = replace_alnum_equivalent(&out, term);
        }
    }
    // Proper nouns: fuzzy token replace (Comarieux → Commarieu).
    out = replace_fuzzy_name_tokens(&out, &terms);
    out
}

/// `bq underscore drift underscore sync.py` → `bq_drift_sync.py`
fn rewrite_spoken_underscores(text: &str) -> String {
    let probe = text.to_ascii_lowercase();
    if !probe.contains("underscore")
        && !probe.contains("sous-tiret")
        && !probe.contains("sous tiret")
        && !probe.contains("tiret du bas")
    {
        return text.to_string();
    }

    let mut chars: Vec<char> = text.chars().collect();
    loop {
        let lower_chars: Vec<char> = chars.iter().map(|c| c.to_ascii_lowercase()).collect();
        let Some((start_pat, len_pat)) = find_spoken_underscore(&lower_chars) else {
            break;
        };

        let mut left = start_pat;
        while left > 0 {
            let c = chars[left - 1];
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+') {
                left -= 1;
            } else if c.is_whitespace() {
                left -= 1;
            } else {
                break;
            }
        }
        while left < start_pat && chars[left].is_whitespace() {
            left += 1;
        }

        let mut right = start_pat + len_pat;
        while right < chars.len() && chars[right].is_whitespace() {
            right += 1;
        }
        let right_start = right;
        while right < chars.len() {
            let c = chars[right];
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+') {
                right += 1;
            } else {
                break;
            }
        }

        let left_tok: String = chars[left..start_pat]
            .iter()
            .collect::<String>()
            .trim()
            .to_string();
        let right_tok: String = chars[right_start..right]
            .iter()
            .collect::<String>()
            .trim()
            .to_string();

        if left_tok.is_empty() || right_tok.is_empty() {
            chars.drain(start_pat..start_pat + len_pat);
            collapse_spaces(&mut chars);
            continue;
        }

        let merged = format!("{left_tok}_{right_tok}");
        chars.splice(left..right, merged.chars());
    }
    chars.into_iter().collect()
}

fn find_spoken_underscore(lower: &[char]) -> Option<(usize, usize)> {
    let patterns: &[&[char]] = &[
        &['u', 'n', 'd', 'e', 'r', 's', 'c', 'o', 'r', 'e'],
        &['s', 'o', 'u', 's', '-', 't', 'i', 'r', 'e', 't'],
        &['s', 'o', 'u', 's', ' ', 't', 'i', 'r', 'e', 't'],
        &['t', 'i', 'r', 'e', 't', ' ', 'd', 'u', ' ', 'b', 'a', 's'],
    ];
    for pat in patterns {
        if let Some(start) = find_subslice(lower, pat) {
            let prev_ok = start == 0 || !lower[start - 1].is_ascii_alphanumeric();
            let end = start + pat.len();
            let next_ok = end >= lower.len() || !lower[end].is_ascii_alphanumeric();
            if prev_ok && next_ok {
                return Some((start, pat.len()));
            }
        }
    }
    None
}

fn find_subslice(hay: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    for i in 0..=hay.len() - needle.len() {
        if &hay[i..i + needle.len()] == needle {
            return Some(i);
        }
    }
    None
}

fn collapse_spaces(chars: &mut Vec<char>) {
    let mut i = 0;
    while i + 1 < chars.len() {
        if chars[i].is_whitespace() && chars[i + 1].is_whitespace() {
            chars.remove(i + 1);
        } else {
            i += 1;
        }
    }
}

fn alnum_lower(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_ident_sep(c: char) -> bool {
    matches!(c, '.' | '_' | '-' | '+')
}

/// Replace a span whose alphanumeric skeleton matches `canonical`
/// (`bqdrift.sync.py` / `bqdriftsync.py` → `bq_drift_sync.py`).
fn replace_alnum_equivalent(text: &str, canonical: &str) -> String {
    let needle = alnum_lower(canonical);
    if needle.len() < 6 || text.contains(canonical) {
        return text.to_string();
    }

    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    let mut out = String::new();

    while i < chars.len() {
        if !chars[i].is_ascii_alphanumeric() {
            out.push(chars[i]);
            i += 1;
            continue;
        }

        let mut j = i;
        let mut collected = String::new();
        let mut end = i;
        let mut matched = false;
        while j < chars.len() {
            let c = chars[j];
            if c.is_ascii_alphanumeric() {
                collected.extend(c.to_lowercase());
                end = j + 1;
                if collected.len() == needle.len()
                    && (collected == needle || soft_alnum_match(&collected, &needle))
                    && chars[i..end]
                        .iter()
                        .all(|ch| ch.is_ascii_alphanumeric() || is_ident_sep(*ch))
                {
                    matched = true;
                    break;
                }
                if collected.len() > needle.len() {
                    break;
                }
            } else if is_ident_sep(c) {
                end = j + 1;
            } else {
                break;
            }
            j += 1;
        }

        if matched {
            out.push_str(canonical);
            i = end;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn soft_alnum_match(a: &str, b: &str) -> bool {
    if a.len() != b.len() || a.len() < 10 {
        return false;
    }
    let mut diff = 0usize;
    for (ca, cb) in a.chars().zip(b.chars()) {
        if ca != cb {
            diff += 1;
            if diff > 1 {
                return false;
            }
        }
    }
    diff == 1
}

fn is_name_like_term(term: &str) -> bool {
    if term.contains('@') || term.contains('.') || term.contains('_') || term.contains('/') {
        return false;
    }
    if term.contains(' ') {
        return false;
    }
    let letters: Vec<char> = term.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() < 6 {
        return false;
    }
    // Prefer capitalised proper nouns from UI (Slack display names, etc.).
    letters[0].is_uppercase()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut curr = vec![0usize; m + 1];
    for i in 1..=n {
        curr[0] = i;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

fn fuzzy_name_match(word: &str, canonical: &str) -> bool {
    let w = alnum_lower(word);
    let c = alnum_lower(canonical);
    if w.len() < 6 || c.len() < 6 {
        return false;
    }
    if w == c {
        return false; // already correct
    }
    let len_diff = w.len().abs_diff(c.len());
    if len_diff > 2 {
        return false;
    }
    let dist = levenshtein(&w, &c);
    let max_dist = if c.len() >= 8 { 2 } else { 1 };
    dist > 0 && dist <= max_dist && dist * 4 <= c.len().max(w.len())
}

/// Replace near-miss name tokens with the on-screen spelling.
fn replace_fuzzy_name_tokens(text: &str, terms: &[&String]) -> String {
    let name_terms: Vec<&str> = terms
        .iter()
        .map(|t| t.as_str())
        .filter(|t| is_name_like_term(t))
        .collect();
    if name_terms.is_empty() {
        return text.to_string();
    }

    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    let mut out = String::new();
    while i < chars.len() {
        if !chars[i].is_alphabetic() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && (chars[i].is_alphabetic() || matches!(chars[i], '\'' | '-' | '’'))
        {
            i += 1;
        }
        let word: String = chars[start..i].iter().collect();
        let mut best: Option<&str> = None;
        let mut best_dist = usize::MAX;
        for term in &name_terms {
            if fuzzy_name_match(&word, term) {
                let d = levenshtein(&alnum_lower(&word), &alnum_lower(term));
                if d < best_dist {
                    best_dist = d;
                    best = Some(*term);
                }
            }
        }
        if let Some(canon) = best {
            out.push_str(canon);
        } else {
            out.push_str(&word);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoken_underscore_to_snake() {
        let out = rewrite_spoken_underscores("bq underscore drift underscore sync.py");
        assert_eq!(out, "bq_drift_sync.py");
    }

    #[test]
    fn collapsed_dot_form_maps_to_canonical() {
        let terms = vec!["bq_drift_sync.py".to_string()];
        let out = rewrite_transcript("see bqdrift.sync.py please", &terms);
        assert!(out.contains("bq_drift_sync.py"), "got {out}");
        assert!(!out.contains("bqdrift.sync.py"));
    }

    #[test]
    fn collapsed_no_sep_maps() {
        let terms = vec!["bq_drift_sync.py".to_string()];
        let out = rewrite_transcript("open bqdriftsync.py now", &terms);
        assert!(out.contains("bq_drift_sync.py"), "got {out}");
    }

    #[test]
    fn fuzzy_surname_commarieu() {
        let terms = vec!["Commarieu".to_string(), "Laurent".to_string()];
        let out = rewrite_transcript("Laurent Comarieux dit bonjour", &terms);
        assert!(out.contains("Commarieu"), "got {out}");
        assert!(!out.contains("Comarieux"));
        assert!(out.contains("Laurent"));
    }

    #[test]
    fn fuzzy_does_not_replace_unrelated() {
        let terms = vec!["Commarieu".to_string()];
        let out = rewrite_transcript("Bonjour Paris demain", &terms);
        assert_eq!(out, "Bonjour Paris demain");
    }
}
