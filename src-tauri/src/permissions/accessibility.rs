use serde::{Deserialize, Serialize};

/// Bump when a one-time TCC cleanup must re-run for users stuck on a prior migration.
pub const CURRENT_CLEANUP_GENERATION: u32 = 2;

const CURRENT_TCC_BUNDLE_ID: &str = "io.gladia.gladiaflow";
const LEGACY_TCC_BUNDLE_IDS: &[&str] = &["io.gladiaflow.app"];

/// Legacy bundle ids are best-effort cleanup. Only the current bundle's reset
/// determines whether the migration completed and may be persisted.
fn migration_cleanup_succeeded(results: &[(&str, bool)]) -> bool {
    results
        .iter()
        .find(|(bundle_id, _)| *bundle_id == CURRENT_TCC_BUNDLE_ID)
        .is_some_and(|(_, succeeded)| *succeeded)
}

/// macOS Accessibility permission state derived from AX trust + prompt history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessibilityState {
    NotDetermined,
    Denied,
    GrantedWorking,
}

impl AccessibilityState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotDetermined => "not_determined",
            Self::Denied => "denied",
            Self::GrantedWorking => "granted_working",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "not_determined" => Some(Self::NotDetermined),
            "denied" => Some(Self::Denied),
            "granted_working" => Some(Self::GrantedWorking),
            _ => None,
        }
    }
}

/// Pure classifier: AX trust plus whether the native prompt has been shown.
pub fn classify(trusted: bool, prompted: bool) -> AccessibilityState {
    if trusted {
        AccessibilityState::GrantedWorking
    } else if prompted {
        AccessibilityState::Denied
    } else {
        AccessibilityState::NotDetermined
    }
}

/// Whether a one-time TCC migration still needs to be completed.
/// Version bumps alone never make a completed generation pending again.
pub fn is_tcc_cleanup_pending(tcc_reset_done: bool, cleanup_generation: u32) -> bool {
    !tcc_reset_done || cleanup_generation < CURRENT_CLEANUP_GENERATION
}

/// A pending migration only needs to clear TCC when the current signed process
/// is not already trusted. This preserves working grants for users who have
/// already authorized the Developer ID build.
fn needs_tcc_reset(tcc_reset_done: bool, cleanup_generation: u32, trusted: bool) -> bool {
    is_tcc_cleanup_pending(tcc_reset_done, cleanup_generation) && !trusted
}

#[cfg(target_os = "macos")]
mod macos {
    pub fn is_trusted() -> bool {
        unsafe {
            extern "C" {
                fn AXIsProcessTrusted() -> bool;
            }
            AXIsProcessTrusted()
        }
    }

    pub fn open_accessibility_settings() {
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn();
    }

    pub fn reset_tcc_grants() -> bool {
        let bundle_ids = std::iter::once(super::CURRENT_TCC_BUNDLE_ID)
            .chain(super::LEGACY_TCC_BUNDLE_IDS.iter().copied());
        let mut results = Vec::new();

        for bundle_id in bundle_ids {
            let succeeded = match std::process::Command::new("tccutil")
                .args(["reset", "Accessibility", bundle_id])
                .status()
            {
                Ok(status) if status.success() => {
                    log::info!("[accessibility] TCC reset for {bundle_id} succeeded");
                    true
                }
                Ok(status) => {
                    log::warn!("[accessibility] TCC reset for {bundle_id} exited with {status}");
                    false
                }
                Err(e) => {
                    log::warn!("[accessibility] TCC reset for {bundle_id} failed to run: {e}");
                    false
                }
            };
            results.push((bundle_id, succeeded));
        }

        let completed = super::migration_cleanup_succeeded(&results);
        if completed
            && results.iter().any(|(bundle_id, succeeded)| {
                *bundle_id != super::CURRENT_TCC_BUNDLE_ID && !succeeded
            })
        {
            log::info!(
                "[accessibility] current bundle reset succeeded; ignoring legacy bundle cleanup failures"
            );
        }

        completed
    }
}

#[cfg(not(target_os = "macos"))]
mod macos {
    pub fn is_trusted() -> bool {
        true
    }

    pub fn open_accessibility_settings() {}

    pub fn reset_tcc_grants() -> bool {
        true
    }
}

pub fn check_state() -> AccessibilityState {
    let trusted = macos::is_trusted();
    let prompted = crate::config::is_accessibility_prompted().unwrap_or_else(|error| {
        log::error!("[config] failed to load accessibility prompt state; using false: {error}");
        false
    });
    classify(trusted, prompted)
}

/// Spawn a throwaway subprocess so TCC trust is read without the per-process cache.
pub fn check_state_fresh() -> AccessibilityState {
    if let Ok(exe) = std::env::current_exe() {
        if let Ok(output) = std::process::Command::new(&exe)
            .arg("--check-ax-state")
            .output()
        {
            if output.status.success() {
                let state = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if let Some(parsed) = AccessibilityState::from_str(&state) {
                    return parsed;
                }
            }
        }
    }
    check_state()
}

pub fn open_accessibility_settings() {
    macos::open_accessibility_settings();
}

pub fn check_and_log_state() -> AccessibilityState {
    let state = check_state_fresh();
    log::info!(
        "[accessibility] state {:?} (v{})",
        state,
        env!("CARGO_PKG_VERSION")
    );
    state
}

/// One-time migration: clear stale TCC rows from legacy bundle ids / signing changes.
pub fn maybe_run_migration_reset() -> Result<bool, String> {
    let tcc_reset_done = crate::config::is_tcc_reset_done()?;
    let cleanup_generation = crate::config::get_cleanup_generation()?;
    if !is_tcc_cleanup_pending(tcc_reset_done, cleanup_generation) {
        return Ok(false);
    }

    let trusted = check_state_fresh() == AccessibilityState::GrantedWorking;
    if !needs_tcc_reset(tcc_reset_done, cleanup_generation, trusted) {
        log::info!(
            "[accessibility] preserving working grant and advancing TCC cleanup generation \
             from {cleanup_generation} to {CURRENT_CLEANUP_GENERATION}"
        );
        crate::config::mark_tcc_reset_done()?;
        crate::config::set_cleanup_generation(CURRENT_CLEANUP_GENERATION)?;
        return Ok(false);
    }

    log::info!(
        "[accessibility] running one-time TCC cleanup (tcc_reset_done={tcc_reset_done}, cleanup_generation={cleanup_generation})"
    );

    if !macos::reset_tcc_grants() {
        log::warn!("[accessibility] TCC cleanup failed; will retry on next launch");
        return Ok(false);
    }

    crate::config::mark_tcc_reset_done()?;
    crate::config::set_cleanup_generation(CURRENT_CLEANUP_GENERATION)?;
    Ok(true)
}

#[cfg(all(target_os = "macos", debug_assertions))]
pub fn dev_reset_tcc() -> Result<AccessibilityState, String> {
    if !macos::reset_tcc_grants() {
        return Err("tccutil reset failed".into());
    }
    Ok(check_state_fresh())
}

#[cfg(not(all(target_os = "macos", debug_assertions)))]
pub fn dev_reset_tcc() -> Result<AccessibilityState, String> {
    Err("dev_reset_tcc is only available in macOS debug builds".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_granted_working_when_trusted() {
        assert_eq!(
            classify(true, false),
            AccessibilityState::GrantedWorking
        );
        assert_eq!(classify(true, true), AccessibilityState::GrantedWorking);
    }

    #[test]
    fn classify_not_determined_when_untrusted_and_never_prompted() {
        assert_eq!(
            classify(false, false),
            AccessibilityState::NotDetermined
        );
    }

    #[test]
    fn classify_denied_when_untrusted_and_prompted() {
        assert_eq!(classify(false, true), AccessibilityState::Denied);
    }

    #[test]
    fn cleanup_is_pending_when_migration_not_done() {
        assert!(is_tcc_cleanup_pending(false, 0));
        assert!(is_tcc_cleanup_pending(false, CURRENT_CLEANUP_GENERATION));
    }

    #[test]
    fn cleanup_is_pending_when_generation_is_stale() {
        assert!(is_tcc_cleanup_pending(true, 0));
        assert!(!is_tcc_cleanup_pending(true, CURRENT_CLEANUP_GENERATION));
    }

    #[test]
    fn cleanup_is_not_pending_for_version_bump_alone() {
        assert!(!is_tcc_cleanup_pending(true, CURRENT_CLEANUP_GENERATION));
    }

    #[test]
    fn pending_cleanup_resets_when_current_process_is_untrusted() {
        assert!(needs_tcc_reset(false, 0, false));
        assert!(needs_tcc_reset(true, 1, false));
    }

    #[test]
    fn pending_cleanup_preserves_a_working_grant() {
        assert!(!needs_tcc_reset(false, 0, true));
        assert!(!needs_tcc_reset(true, 1, true));
    }

    #[test]
    fn completed_cleanup_does_not_reset_an_untrusted_process_again() {
        assert!(!needs_tcc_reset(true, CURRENT_CLEANUP_GENERATION, false));
    }

    #[test]
    fn migration_completes_when_current_bundle_reset_succeeds() {
        let results = [("io.gladia.gladiaflow", true), ("io.gladiaflow.app", false)];

        assert!(migration_cleanup_succeeded(&results));
    }

    #[test]
    fn migration_retries_when_current_bundle_reset_fails() {
        let results = [("io.gladia.gladiaflow", false), ("io.gladiaflow.app", true)];

        assert!(!migration_cleanup_succeeded(&results));
    }

    #[test]
    fn accessibility_state_round_trip() {
        for state in [
            AccessibilityState::NotDetermined,
            AccessibilityState::Denied,
            AccessibilityState::GrantedWorking,
        ] {
            assert_eq!(AccessibilityState::from_str(state.as_str()), Some(state));
        }
    }
}
