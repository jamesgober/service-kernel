//! Restart-policy decisions.
//!
//! [`RestartPolicy`] answers a simple question: given that a worker
//! returned an error or panicked, should the supervisor spawn it
//! again? Combined with [`BackoffPolicy`](super::BackoffPolicy) it
//! produces the full retry behavior.

use std::time::Duration;

/// Restart-policy variants.
///
/// Marked `#[non_exhaustive]` so future strategies can land without
/// breaking SemVer.
#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum RestartPolicy {
    /// Never restart. A failed worker stops permanently.
    Never,
    /// Restart only when the worker returned an error or panicked.
    OnFailure,
    /// Restart even on clean completion. Useful for periodic loops.
    Always,
    /// Restart up to `retries` times within a sliding `window`.
    /// Once the window is exhausted, the worker stops.
    MaxRetries {
        /// Maximum number of restarts allowed inside `window`.
        retries: u32,
        /// Sliding window over which `retries` is counted.
        window: Duration,
    },
}

impl RestartPolicy {
    /// Returns whether the supervisor should attempt another spawn.
    ///
    /// `has_failed` indicates whether the worker terminated
    /// abnormally (via error or panic). `failures_in_window` is the
    /// number of failures observed inside the policy's sliding
    /// window — only consulted by `MaxRetries`.
    #[inline]
    #[must_use]
    pub fn should_restart(&self, has_failed: bool, failures_in_window: u32) -> bool {
        match self {
            RestartPolicy::Never => false,
            RestartPolicy::OnFailure => has_failed,
            RestartPolicy::Always => true,
            RestartPolicy::MaxRetries { retries, .. } => {
                has_failed && failures_in_window < *retries
            }
        }
    }
}

impl Default for RestartPolicy {
    /// Returns [`RestartPolicy::OnFailure`] — the safe default.
    #[inline]
    fn default() -> Self {
        RestartPolicy::OnFailure
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_never_returns_false() {
        assert!(!RestartPolicy::Never.should_restart(true, 0));
        assert!(!RestartPolicy::Never.should_restart(false, 0));
    }

    #[test]
    fn test_on_failure_returns_has_failed() {
        let p = RestartPolicy::OnFailure;
        assert!(p.should_restart(true, 0));
        assert!(!p.should_restart(false, 0));
    }

    #[test]
    fn test_always_returns_true() {
        let p = RestartPolicy::Always;
        assert!(p.should_restart(true, 99));
        assert!(p.should_restart(false, 0));
    }

    #[test]
    fn test_max_retries_under_threshold() {
        let p = RestartPolicy::MaxRetries {
            retries: 3,
            window: Duration::from_secs(60),
        };
        assert!(p.should_restart(true, 0));
        assert!(p.should_restart(true, 2));
    }

    #[test]
    fn test_max_retries_at_or_above_threshold_stops() {
        let p = RestartPolicy::MaxRetries {
            retries: 3,
            window: Duration::from_secs(60),
        };
        assert!(!p.should_restart(true, 3));
        assert!(!p.should_restart(true, 4));
    }

    #[test]
    fn test_max_retries_does_not_restart_clean_completion() {
        let p = RestartPolicy::MaxRetries {
            retries: 5,
            window: Duration::from_secs(60),
        };
        assert!(!p.should_restart(false, 0));
    }

    #[test]
    fn test_default_is_on_failure() {
        assert_eq!(RestartPolicy::default(), RestartPolicy::OnFailure);
    }
}
