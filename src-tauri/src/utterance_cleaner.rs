fn strip_leading_punctuation(s: &str) -> String {
    s.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '.' | ',' | ';' | ':' | '…'))
        .to_string()
}

fn split_trailing_period(s: &str) -> (&str, Option<char>) {
    let trimmed = s.trim_end();
    let body = trimmed.trim_end_matches(['.', '…']);
    if body.len() < trimmed.len() {
        (body, Some('.'))
    } else {
        (trimmed, None)
    }
}

fn starts_lowercase(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_lowercase())
}

fn decapitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_lowercase().to_string() + chars.as_str(),
    }
}

/// Utterances closer than this threshold (in seconds) are treated as a
/// continuation of the same sentence, regardless of capitalisation.
/// Gladia auto-capitalises the start of every new segment, so a short gap
/// with an uppercase word does not imply a sentence boundary.
const CONTINUATION_GAP_SECS: f64 = 1.5;

fn is_continuation_gap(prev_end: f64, curr_start: f64) -> bool {
    if prev_end < 0.0 || curr_start < 0.0 {
        return false;
    }
    (curr_start - prev_end) < CONTINUATION_GAP_SECS
}

/// Stateful cleaner that processes Gladia final utterances one at a time and
/// builds up a clean, properly punctuated accumulated transcript.
pub struct UtteranceCleaner {
    accumulated_text: String,
    pending_period: Option<char>,
    last_final_text: Option<String>,
    last_final_end: Option<f64>,
}

impl UtteranceCleaner {
    pub fn new() -> Self {
        Self {
            accumulated_text: String::new(),
            pending_period: None,
            last_final_text: None,
            last_final_end: None,
        }
    }

    /// Feed one final utterance. Updates the internal accumulated text and
    /// returns the exact fragment that was appended this call — this is what
    /// should be pasted incrementally into the focused app. Returns `None` for
    /// duplicates/empty input (nothing appended).
    ///
    /// Invariant: concatenating every `Some(_)` fragment returned here, plus the
    /// `flush_pending_period` result, reproduces `accumulated_text()` exactly.
    pub fn process_final(&mut self, raw_text: &str, start: f64, end: f64) -> Option<String> {
        let trimmed = raw_text.trim();
        if self.last_final_text.as_deref() == Some(trimmed) {
            return None;
        }
        self.last_final_text = Some(trimmed.to_string());

        let text = if self.accumulated_text.is_empty() {
            strip_leading_punctuation(trimmed)
        } else {
            trimmed.to_string()
        };
        if text.is_empty() {
            return None;
        }

        let (body_str, new_period) = split_trailing_period(&text);
        let mut body = body_str.to_string();

        let mut fragment = String::new();

        if !self.accumulated_text.is_empty() {
            if let Some(p) = self.pending_period.take() {
                let close_in_time = self
                    .last_final_end
                    .is_some_and(|prev_end| is_continuation_gap(prev_end, start));

                let keep_period = !starts_lowercase(&body) && !close_in_time;

                if keep_period {
                    self.accumulated_text.push(p);
                    fragment.push(p);
                } else if close_in_time && !starts_lowercase(&body) {
                    body = decapitalize_first(&body);
                }
            }
            self.accumulated_text.push(' ');
            fragment.push(' ');
        }

        self.accumulated_text.push_str(&body);
        fragment.push_str(&body);
        self.pending_period = new_period;
        self.last_final_end = Some(end);
        Some(fragment)
    }

    /// Call when recording stops. Appends any deferred trailing period to the
    /// accumulated text, clears the pending state, and returns the period so the
    /// caller can paste it as the final fragment.
    pub fn flush_pending_period(&mut self) -> Option<char> {
        let p = self.pending_period.take()?;
        self.accumulated_text.push(p);
        Some(p)
    }

    pub fn accumulated_text(&self) -> &str {
        &self.accumulated_text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- split_trailing_period ---

    #[test]
    fn test_split_trailing_period_with_period() {
        let (body, punct) = split_trailing_period("Hello world.");
        assert_eq!(body, "Hello world");
        assert_eq!(punct, Some('.'));
    }

    #[test]
    fn test_split_trailing_period_with_ellipsis() {
        let (body, punct) = split_trailing_period("en cours…");
        assert_eq!(body, "en cours");
        assert_eq!(punct, Some('.'));
    }

    #[test]
    fn test_split_trailing_period_three_dots() {
        let (body, punct) = split_trailing_period("qu'elle a pas...");
        assert_eq!(body, "qu'elle a pas");
        assert_eq!(punct, Some('.'));
    }

    #[test]
    fn test_split_trailing_period_two_dots() {
        let (body, punct) = split_trailing_period("me..");
        assert_eq!(body, "me");
        assert_eq!(punct, Some('.'));
    }

    #[test]
    fn test_split_trailing_period_without_period() {
        let (body, punct) = split_trailing_period("Hello world");
        assert_eq!(body, "Hello world");
        assert_eq!(punct, None);
    }

    #[test]
    fn test_split_trailing_period_question_mark_kept() {
        let (body, punct) = split_trailing_period("How are you?");
        assert_eq!(body, "How are you?");
        assert_eq!(punct, None);
    }

    #[test]
    fn test_split_trailing_period_exclamation_kept() {
        let (body, punct) = split_trailing_period("Wow!");
        assert_eq!(body, "Wow!");
        assert_eq!(punct, None);
    }

    #[test]
    fn test_split_trailing_period_comma_kept() {
        let (body, punct) = split_trailing_period("well,");
        assert_eq!(body, "well,");
        assert_eq!(punct, None);
    }

    #[test]
    fn test_split_trailing_period_trailing_whitespace() {
        let (body, punct) = split_trailing_period("Hello world.  ");
        assert_eq!(body, "Hello world");
        assert_eq!(punct, Some('.'));
    }

    #[test]
    fn test_split_trailing_period_dots_with_whitespace() {
        let (body, punct) = split_trailing_period("Hello world...  ");
        assert_eq!(body, "Hello world");
        assert_eq!(punct, Some('.'));
    }

    // --- starts_lowercase ---

    #[test]
    fn test_starts_lowercase_true() {
        assert!(starts_lowercase("hello"));
        assert!(starts_lowercase("l'enregistrement"));
    }

    #[test]
    fn test_starts_lowercase_false() {
        assert!(!starts_lowercase("Hello"));
        assert!(!starts_lowercase("L'enregistrement"));
        assert!(!starts_lowercase("123abc"));
        assert!(!starts_lowercase(""));
    }

    // --- strip_leading_punctuation ---

    #[test]
    fn test_strip_leading_punctuation() {
        assert_eq!(strip_leading_punctuation(". Hello"), "Hello");
        assert_eq!(strip_leading_punctuation("  , World"), "World");
        assert_eq!(strip_leading_punctuation("Hello"), "Hello");
        assert_eq!(strip_leading_punctuation("...test"), "test");
    }

    // --- is_continuation_gap ---

    #[test]
    fn test_is_continuation_gap_close() {
        assert!(is_continuation_gap(1.2, 1.5));
    }

    #[test]
    fn test_is_continuation_gap_zero() {
        assert!(is_continuation_gap(1.2, 1.2));
    }

    #[test]
    fn test_is_continuation_gap_far() {
        assert!(!is_continuation_gap(1.0, 3.0));
    }

    #[test]
    fn test_is_continuation_gap_at_threshold() {
        assert!(!is_continuation_gap(0.0, CONTINUATION_GAP_SECS));
    }

    #[test]
    fn test_is_continuation_gap_just_below_threshold() {
        assert!(is_continuation_gap(0.0, CONTINUATION_GAP_SECS - 0.01));
    }

    #[test]
    fn test_is_continuation_gap_missing_timestamps() {
        assert!(!is_continuation_gap(-1.0, 1.5));
        assert!(!is_continuation_gap(1.0, -1.0));
        assert!(!is_continuation_gap(-1.0, -1.0));
    }

    // --- decapitalize_first ---

    #[test]
    fn test_decapitalize_first_basic() {
        assert_eq!(decapitalize_first("Donc"), "donc");
        assert_eq!(decapitalize_first("Il a dit"), "il a dit");
        assert_eq!(decapitalize_first("Le résultat"), "le résultat");
    }

    #[test]
    fn test_decapitalize_first_already_lowercase() {
        assert_eq!(decapitalize_first("donc"), "donc");
    }

    #[test]
    fn test_decapitalize_first_empty() {
        assert_eq!(decapitalize_first(""), "");
    }

    #[test]
    fn test_decapitalize_first_unicode() {
        assert_eq!(decapitalize_first("Écoute"), "écoute");
        assert_eq!(decapitalize_first("À bientôt"), "à bientôt");
    }

    // --- UtteranceCleaner integration ---

    #[test]
    fn test_single_utterance_no_period() {
        let mut c = UtteranceCleaner::new();
        c.process_final("Hello world", -1.0, -1.0);
        assert_eq!(c.accumulated_text(), "Hello world");
    }

    #[test]
    fn test_two_utterances_joined_with_space() {
        let mut c = UtteranceCleaner::new();
        c.process_final("Hello world", -1.0, -1.0);
        c.process_final("how are you", -1.0, -1.0);
        assert_eq!(c.accumulated_text(), "Hello world how are you");
    }

    #[test]
    fn test_strip_leading_punctuation_on_first_utterance_only() {
        let mut c = UtteranceCleaner::new();
        c.process_final(". Hello", -1.0, -1.0);
        assert_eq!(c.accumulated_text(), "Hello");
        c.process_final(". world", -1.0, -1.0);
        assert_eq!(c.accumulated_text(), "Hello . world");
    }

    #[test]
    fn test_duplicate_final_skipped() {
        let mut c = UtteranceCleaner::new();
        c.process_final("C'est bon", -1.0, -1.0);
        let added = c.process_final("C'est bon", -1.0, -1.0);
        assert!(added.is_none());
        assert_eq!(c.accumulated_text(), "C'est bon");
    }

    #[test]
    fn test_period_kept_on_long_gap_uppercase_next() {
        // "Bonjour." [end=0.8] → "Comment vas-tu" [start=2.5]: 1.7s gap → keep period
        let mut c = UtteranceCleaner::new();
        c.process_final("Bonjour.", 0.0, 0.8);
        c.process_final("Comment vas-tu", 2.5, 3.2);
        assert_eq!(c.accumulated_text(), "Bonjour. Comment vas-tu");
    }

    #[test]
    fn test_period_discarded_on_lowercase_next() {
        let mut c = UtteranceCleaner::new();
        c.process_final("tu perds.", 0.0, 1.0);
        c.process_final("l'enregistrement", 1.1, 2.0);
        assert_eq!(c.accumulated_text(), "tu perds l'enregistrement");
    }

    #[test]
    fn test_period_discarded_and_decapitalized_on_short_gap() {
        // "la ponctuation." [end=3.0] → "Donc on continue" [start=3.2]: 0.2s gap
        let mut c = UtteranceCleaner::new();
        c.process_final("la ponctuation.", 0.0, 3.0);
        c.process_final("Donc on continue", 3.2, 4.5);
        assert_eq!(c.accumulated_text(), "la ponctuation donc on continue");
    }

    #[test]
    fn test_flush_pending_period_appends_to_accumulated() {
        let mut c = UtteranceCleaner::new();
        c.process_final("Hello world.", -1.0, -1.0);
        c.flush_pending_period();
        assert_eq!(c.accumulated_text(), "Hello world.");
    }

    #[test]
    fn test_flush_pending_period_noop_when_empty() {
        let mut c = UtteranceCleaner::new();
        c.process_final("Hello world", -1.0, -1.0);
        c.flush_pending_period();
        assert_eq!(c.accumulated_text(), "Hello world");
    }

    #[test]
    fn test_process_final_returns_incremental_fragment() {
        let mut c = UtteranceCleaner::new();
        // First utterance: fragment is the body, no leading space.
        assert_eq!(c.process_final("Hello world.", -1.0, -1.0).as_deref(), Some("Hello world"));
        // Continuation (lowercase next): period discarded, leading space + body.
        assert_eq!(
            c.process_final("and more", -1.0, -1.0).as_deref(),
            Some(" and more")
        );
    }

    #[test]
    fn test_fragments_reconstruct_accumulated_text() {
        // The core invariant the incremental-paste path relies on: the pasted
        // fragments concatenated equal the final accumulated transcript.
        let inputs = [
            ("la ponctuation.", 0.0, 3.0),
            ("Donc on continue.", 3.2, 4.5),
            ("Bonjour.", 6.0, 6.8),
            ("Comment vas-tu", 8.5, 9.2),
        ];
        let mut c = UtteranceCleaner::new();
        let mut pasted = String::new();
        for (text, start, end) in inputs {
            if let Some(frag) = c.process_final(text, start, end) {
                pasted.push_str(&frag);
            }
        }
        if let Some(p) = c.flush_pending_period() {
            pasted.push(p);
        }
        assert_eq!(pasted, c.accumulated_text());
    }

    #[test]
    fn test_full_donc_scenario() {
        let mut c = UtteranceCleaner::new();
        c.process_final("la ponctuation.", 0.0, 3.0);
        c.process_final("Donc on ne devrait plus avoir de problèmes", 3.2, 5.0);
        assert_eq!(
            c.accumulated_text(),
            "la ponctuation donc on ne devrait plus avoir de problèmes"
        );
    }
}
