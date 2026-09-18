//! Rolling on-screen vocabulary store (context memory).
//!
//! Background sampler upserts terms; dictation takes a snapshot. Entries expire
//! after [`TTL`] (longer for rare proper names) so the list tracks recent work
//! without growing forever.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Default how long a harvested term stays eligible for Gladia custom vocabulary.
pub const TTL: Duration = Duration::from_secs(15 * 60);

/// Rare on-screen surnames stick longer — you often dictate after leaving Slack.
pub const TTL_PROPER_NAME: Duration = Duration::from_secs(30 * 60);

/// Soft cap on active terms (prefer structured / recent / proper names).
pub const MAX_TERMS: usize = 100;

#[derive(Debug, Clone)]
pub struct StoredTerm {
    pub value: String,
    pub last_seen: Instant,
}

/// Title-case surname / person token worth keeping across app switches.
fn is_sticky_proper_name(value: &str) -> bool {
    if value.contains('.') || value.contains('@') || value.contains('_') {
        return false;
    }
    if value.chars().any(|c| c.is_ascii_digit()) {
        return false;
    }
    let letters: Vec<char> = value.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() < 6 {
        return false;
    }
    // Single Title-case token or multi-word person bigram (`Laurent Commarieu`).
    value.split_whitespace().all(|part| {
        let mut chars = part.chars().filter(|c| c.is_alphabetic());
        match chars.next() {
            Some(first) if first.is_uppercase() => chars.all(|c| !c.is_uppercase()),
            _ => false,
        }
    })
}

fn ttl_for(value: &str) -> Duration {
    if is_sticky_proper_name(value) {
        TTL_PROPER_NAME
    } else {
        TTL
    }
}

struct StoreInner {
    /// Lowercase key → display form + timestamp.
    by_key: HashMap<String, StoredTerm>,
}

impl StoreInner {
    fn new() -> Self {
        Self {
            by_key: HashMap::new(),
        }
    }

    fn upsert(&mut self, value: String) {
        let key = value.trim().to_lowercase();
        if key.is_empty() {
            return;
        }
        let now = Instant::now();
        match self.by_key.get_mut(&key) {
            Some(existing) => {
                existing.last_seen = now;
                // Keep richer casing if new form has more capitals.
                let new_caps = value.chars().filter(|c| c.is_uppercase()).count();
                let old_caps = existing.value.chars().filter(|c| c.is_uppercase()).count();
                if new_caps > old_caps {
                    existing.value = value;
                }
            }
            None => {
                self.by_key.insert(
                    key,
                    StoredTerm {
                        value,
                        last_seen: now,
                    },
                );
            }
        }
    }

    fn evict_expired(&mut self) {
        let now = Instant::now();
        self.by_key
            .retain(|_, t| now.duration_since(t.last_seen) <= ttl_for(&t.value));
    }

    fn trim_to_cap(&mut self) {
        if self.by_key.len() <= MAX_TERMS {
            return;
        }
        // Drop oldest non-sticky first, then oldest sticky.
        let mut entries: Vec<(String, Instant, bool)> = self
            .by_key
            .iter()
            .map(|(k, t)| (k.clone(), t.last_seen, is_sticky_proper_name(&t.value)))
            .collect();
        entries.sort_by(|a, b| {
            a.2.cmp(&b.2) // non-sticky (false) before sticky (true)
                .then_with(|| a.1.cmp(&b.1)) // oldest first
        });
        let drop_n = entries.len().saturating_sub(MAX_TERMS);
        for (key, _, _) in entries.into_iter().take(drop_n) {
            self.by_key.remove(&key);
        }
    }

    fn snapshot_values(&mut self) -> Vec<String> {
        self.evict_expired();
        self.trim_to_cap();
        let mut items: Vec<&StoredTerm> = self.by_key.values().collect();
        items.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
        items.into_iter().map(|t| t.value.clone()).collect()
    }

    fn len_active(&mut self) -> usize {
        self.evict_expired();
        self.by_key.len()
    }
}

fn store() -> &'static Mutex<StoreInner> {
    static STORE: OnceLock<Mutex<StoreInner>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(StoreInner::new()))
}

/// Insert / refresh terms from a harvest pass.
pub fn upsert_terms(terms: impl IntoIterator<Item = String>) {
    let Ok(mut guard) = store().lock() else {
        return;
    };
    for term in terms {
        if crate::screen_context::is_vocab_noise(&term) {
            continue;
        }
        guard.upsert(term);
    }
    // Drop previously stored chrome that the filter now rejects.
    guard.by_key.retain(|_, t| !crate::screen_context::is_vocab_noise(&t.value));
    guard.evict_expired();
    guard.trim_to_cap();
}

/// Current vocabulary values (newest first), expired removed.
pub fn snapshot() -> Vec<String> {
    let Ok(mut guard) = store().lock() else {
        return Vec::new();
    };
    guard
        .by_key
        .retain(|_, t| !crate::screen_context::is_vocab_noise(&t.value));
    guard.snapshot_values()
}

pub fn active_count() -> usize {
    store()
        .lock()
        .map(|mut s| s.len_active())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_dedupes_case_insensitive() {
        let mut inner = StoreInner::new();
        inner.upsert("Commarieu".into());
        inner.upsert("commarieu".into());
        assert_eq!(inner.by_key.len(), 1);
        assert_eq!(inner.by_key.values().next().unwrap().value, "Commarieu");
    }

    #[test]
    fn proper_names_use_longer_ttl() {
        assert!(is_sticky_proper_name("Commarieu"));
        assert!(is_sticky_proper_name("Laurent Commarieu"));
        assert!(!is_sticky_proper_name("API"));
        assert!(!is_sticky_proper_name("eligibility_query.py"));
        assert_eq!(ttl_for("Commarieu"), TTL_PROPER_NAME);
        assert_eq!(ttl_for("FEA-2341"), TTL);
    }
}
