//! On-screen context → soft custom vocabulary (opt-in).
//!
//! Background sampler fills a rolling store from window titles + accessibility
//! text (macOS AX / Windows UIA), with chat-window OCR fallback (Vision /
//! WinRT). Dictation snapshots the store into Gladia custom vocabulary.
//! Manual vocabulary always wins. Prefer OpenPII MLX when available (macOS).

mod filter;
mod mlx;
mod ner;
mod rewrite;
mod store;

#[cfg(target_os = "macos")]
mod ax;
#[cfg(target_os = "macos")]
mod ocr;
#[cfg(target_os = "windows")]
mod win_ocr;
#[cfg(target_os = "windows")]
mod win_uia;

use crate::vocabulary::CustomVocabEntry;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub use filter::{
    extract_vocabulary_terms, is_vocab_noise, pronunciations_for_term, FilterConfig,
};
pub use mlx::resolve_mlx_model_dir;
pub use ner::resolve_model_dir;
pub use rewrite::rewrite_transcript;

/// Soft intensity for terms harvested from on-screen context.
pub const SCREEN_CONTEXT_INTENSITY: f64 = 0.3;

/// Legacy timeout kept for any one-shot harvest callers.
pub const HARVEST_TIMEOUT: Duration = Duration::from_millis(500);

/// Pre-dictation chat OCR must wait for Vision (often 200–800ms).
pub const DICTATION_REFRESH_TIMEOUT: Duration = Duration::from_millis(2_500);

const SAMPLER_INTERVAL: Duration = Duration::from_secs(8);
/// How often we poll for app / window / title changes.
const FOCUS_POLL_INTERVAL: Duration = Duration::from_millis(750);

static SAMPLER_STARTED: AtomicBool = AtomicBool::new(false);

/// Warm MLX sidecar (preferred on macOS) and/or ONNX NER.
pub fn warmup() {
    #[cfg(target_os = "macos")]
    {
        mlx::warmup();
    }
    if !mlx::is_ready() {
        ner::warmup();
    }
}

/// Start background sampler if screen-context is enabled (idempotent).
pub fn start_background_sampler_if_enabled() {
    let enabled = crate::config::use_screen_context_vocabulary().unwrap_or(false);
    if !enabled {
        return;
    }
    start_background_sampler();
}

/// Spawn the rolling harvest thread once per process.
pub fn start_background_sampler() {
    if SAMPLER_STARTED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    std::thread::Builder::new()
        .name("screen-context-sampler".into())
        .spawn(|| {
            log::info!(
                "[screen_context] background sampler started (poll={}ms, regular={}s, ttl={}s, name_ttl={}s)",
                FOCUS_POLL_INTERVAL.as_millis(),
                SAMPLER_INTERVAL.as_secs(),
                store::TTL.as_secs(),
                store::TTL_PROPER_NAME.as_secs()
            );
            // First pass soon so dictation shortly after launch still has terms.
            std::thread::sleep(Duration::from_millis(800));
            let mut last_focus_sig = String::new();
            let mut last_regular = std::time::Instant::now()
                .checked_sub(SAMPLER_INTERVAL)
                .unwrap_or_else(std::time::Instant::now);
            loop {
                if !crate::config::use_screen_context_vocabulary().unwrap_or(false) {
                    std::thread::sleep(FOCUS_POLL_INTERVAL);
                    continue;
                }

                let mut force = false;
                #[cfg(target_os = "macos")]
                {
                    let sig = ax::focus_signature();
                    if sig != last_focus_sig {
                        let preview: String = sig.chars().take(100).collect();
                        log::info!(
                            "[screen_context] focus/window changed → force OCR ({preview})"
                        );
                        last_focus_sig = sig;
                        force = ax::frontmost_is_chat_ocr_app();
                    }
                }
                #[cfg(target_os = "windows")]
                {
                    let sig = win_uia::focus_signature();
                    if sig != last_focus_sig {
                        let preview: String = sig.chars().take(100).collect();
                        log::info!(
                            "[screen_context] focus/window changed → force OCR ({preview})"
                        );
                        last_focus_sig = sig;
                        force = win_uia::frontmost_is_chat_ocr_app();
                    }
                }

                let due_regular = last_regular.elapsed() >= SAMPLER_INTERVAL;
                if force || due_regular {
                    let pass = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        harvest_pass(force)
                    }));
                    match pass {
                        Ok(n) => {
                            if n > 0 {
                                log::info!(
                                    "[screen_context] sampler upserted {} term(s); store active={} (force={force})",
                                    n,
                                    store::active_count()
                                );
                            }
                        }
                        Err(_) => log::warn!("[screen_context] sampler pass panicked"),
                    }
                    if due_regular || force {
                        last_regular = std::time::Instant::now();
                    }
                }

                std::thread::sleep(FOCUS_POLL_INTERVAL);
            }
        })
        .ok();
}

fn sampler_pass() -> usize {
    harvest_pass(false)
}

/// Force-refresh chat OCR into the rolling store right before dictation.
/// Bypasses the per-window throttle so a name shown seconds ago is included.
pub fn refresh_chat_context_for_dictation() -> usize {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        harvest_pass(true)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        0
    }
}

fn harvest_pass(force_chat_ocr: bool) -> usize {
    let mut text = read_frontmost_text();
    #[cfg(target_os = "macos")]
    {
        // Electron chat (Slack, …): AX stays empty even after AXManualAccessibility.
        // Auto OCR those windows — this is the only reliable way to get message names.
        let chat_windows = ax::list_chat_windows_for_ocr();
        if !chat_windows.is_empty() {
            // Pre-dictation: only the top windows (focused/Slack first). Sampler
            // may still OCR the rest later under the per-window throttle.
            let limit = if force_chat_ocr { 2 } else { chat_windows.len() };
            let ids: Vec<u32> = chat_windows
                .iter()
                .take(limit)
                .map(|w| w.window_id)
                .collect();
            let ocr_text = if force_chat_ocr {
                ocr::recognize_windows_forced(&ids, 6_000)
            } else {
                ocr::recognize_windows(&ids, 6_000)
            };
            if !ocr_text.trim().is_empty() {
                log::info!(
                    "[screen_context] chat-window OCR appended {} char(s) from {} window(s) (force={force_chat_ocr})",
                    ocr_text.len(),
                    ids.len()
                );
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&ocr_text);
            }
        }

        // Optional full-display OCR (settings toggle).
        let ocr_on = crate::config::use_screen_context_ocr().unwrap_or(false);
        if ocr_on && text.len() < 1_200 {
            let ocr_text = ocr::recognize_screen_text(8_000);
            if !ocr_text.trim().is_empty() {
                log::info!(
                    "[screen_context] full-display OCR appended {} char(s)",
                    ocr_text.len()
                );
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&ocr_text);
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        let chat_windows = win_uia::list_chat_windows_for_ocr();
        if !chat_windows.is_empty() {
            let limit = if force_chat_ocr { 2 } else { chat_windows.len().min(3) };
            let hwnds: Vec<isize> = chat_windows
                .iter()
                .take(limit)
                .map(|w| w.hwnd)
                .collect();
            let ocr_text = if force_chat_ocr {
                win_ocr::recognize_windows_forced(&hwnds, 6_000)
            } else {
                win_ocr::recognize_windows(&hwnds, 6_000)
            };
            if !ocr_text.trim().is_empty() {
                log::info!(
                    "[screen_context] chat-window OCR appended {} char(s) from {} window(s) (force={force_chat_ocr})",
                    ocr_text.len(),
                    hwnds.len()
                );
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&ocr_text);
            }
        }
    }
    if text.trim().is_empty() {
        return 0;
    }
    let terms = extract_terms_from_text(&text);
    let n = terms.len();
    store::upsert_terms(terms);
    n
}

fn extract_terms_from_text(text: &str) -> Vec<String> {
    let mut terms = extract_vocabulary_terms(text, &FilterConfig::default());
    let ner_terms = if mlx::is_ready() {
        mlx::extract_entity_terms(text)
    } else {
        ner::extract_entity_terms(text)
    };
    let ner_terms: Vec<String> = ner_terms
        .into_iter()
        .filter(|t| !is_vocab_noise(t))
        .collect();
    merge_term_strings(&mut terms, ner_terms);
    terms.retain(|t| !is_vocab_noise(t));
    terms
}

fn entries_from_term_strings(terms: Vec<String>) -> Vec<CustomVocabEntry> {
    terms
        .into_iter()
        .map(|value| {
            let pronunciations = pronunciations_for_term(&value);
            let name_like = {
                let letters: Vec<char> = value.chars().filter(|c| c.is_alphabetic()).collect();
                !value.contains('.')
                    && !value.contains('@')
                    && !value.contains('_')
                    && letters.len() >= 6
                    && letters.first().is_some_and(|c| c.is_uppercase())
            };
            let intensity = if pronunciations.is_some() || name_like {
                (SCREEN_CONTEXT_INTENSITY + 0.2).min(0.6)
            } else {
                SCREEN_CONTEXT_INTENSITY
            };
            CustomVocabEntry {
                value,
                pronunciations,
                language: None,
                intensity,
            }
        })
        .collect()
}

/// One-shot harvest (also upserts the store). Prefer [`snapshot_screen_vocabulary`].
pub fn harvest_screen_vocabulary() -> Vec<CustomVocabEntry> {
    let _ = harvest_pass(true);
    snapshot_screen_vocabulary()
}

/// Fast path for dictation: rolling store snapshot (no AX walk on the hot path).
pub fn snapshot_screen_vocabulary() -> Vec<CustomVocabEntry> {
    let terms = store::snapshot();
    entries_from_term_strings(terms)
}

fn merge_term_strings(into: &mut Vec<String>, extra: Vec<String>) {
    let mut seen: std::collections::HashSet<String> =
        into.iter().map(|t| t.to_lowercase()).collect();
    for term in extra {
        let key = term.to_lowercase();
        if key.is_empty() || seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        into.push(term);
    }
}

/// Merge screen terms under manual vocabulary. Manual wins on case-insensitive match.
pub fn merge_vocabulary_manual_priority(
    manual: Vec<CustomVocabEntry>,
    screen: Vec<CustomVocabEntry>,
) -> Vec<CustomVocabEntry> {
    let mut merged = manual;
    let mut seen: std::collections::HashSet<String> = merged
        .iter()
        .map(|e| e.value.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    for entry in screen {
        let key = entry.value.trim().to_lowercase();
        if key.is_empty() || seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        merged.push(entry);
    }
    merged
}

fn read_frontmost_text() -> String {
    #[cfg(target_os = "macos")]
    {
        ax::read_frontmost_context_text(12_000)
    }
    #[cfg(target_os = "windows")]
    {
        win_uia::read_frontmost_context_text(12_000)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        String::new()
    }
}

#[cfg(test)]
mod merge_tests {
    use super::*;

    #[test]
    fn manual_wins_over_screen_duplicate() {
        let manual = vec![CustomVocabEntry {
            value: "GladiaFlow".to_string(),
            pronunciations: Some(vec!["gladioflow".to_string()]),
            language: None,
            intensity: 0.8,
        }];
        let screen = vec![CustomVocabEntry {
            value: "gladiaflow".to_string(),
            pronunciations: None,
            language: None,
            intensity: SCREEN_CONTEXT_INTENSITY,
        }];
        let merged = merge_vocabulary_manual_priority(manual, screen);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].intensity, 0.8);
        assert!(merged[0].pronunciations.is_some());
    }

    #[test]
    fn screen_terms_append_when_new() {
        let manual = vec![CustomVocabEntry {
            value: "GladiaFlow".to_string(),
            pronunciations: None,
            language: None,
            intensity: 0.8,
        }];
        let screen = vec![CustomVocabEntry {
            value: "PostgreSQL".to_string(),
            pronunciations: None,
            language: None,
            intensity: SCREEN_CONTEXT_INTENSITY,
        }];
        let merged = merge_vocabulary_manual_priority(manual, screen);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[1].value, "PostgreSQL");
    }

    #[test]
    fn merge_term_strings_dedupes() {
        let mut terms = vec!["GladiaFlow".to_string()];
        merge_term_strings(
            &mut terms,
            vec!["gladiaflow".to_string(), "Paris".to_string()],
        );
        assert_eq!(terms, vec!["GladiaFlow".to_string(), "Paris".to_string()]);
    }
}
