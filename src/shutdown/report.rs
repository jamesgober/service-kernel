//! Summary of the kernel's shutdown sequence.

use std::time::Duration;

use crate::primitives::Instant;

/// Snapshot of one full shutdown run.
///
/// Returned by
/// [`ShutdownCoordinator::shutdown`](super::ShutdownCoordinator::shutdown).
/// `is_clean` reports whether anything went wrong; the detailed
/// fields say what.
#[derive(Debug, Clone)]
pub struct ShutdownReport {
    /// When the shutdown sequence started.
    pub started_at: Instant,
    /// When the shutdown sequence completed.
    pub completed_at: Instant,
    /// Number of supervisor-managed workers that drained cleanly.
    pub workers_drained: usize,
    /// Number of supervisor-managed workers aborted on grace expiry.
    pub workers_aborted: usize,
    /// Names of hooks that ran successfully.
    pub hooks_succeeded: Vec<&'static str>,
    /// `(hook name, error message)` pairs for failed hooks.
    pub hooks_failed: Vec<(&'static str, String)>,
    /// Names of subsystems whose `shutdown` returned `Ok`.
    pub subsystems_shutdown: Vec<&'static str>,
    /// `(subsystem name, error message)` pairs for failed subsystems.
    pub subsystems_failed: Vec<(&'static str, String)>,
}

impl ShutdownReport {
    /// Returns the total time spent in the shutdown sequence.
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.completed_at.duration_since(self.started_at)
    }

    /// Returns `true` when no workers were aborted, no hooks failed,
    /// and no subsystem `shutdown` returned `Err`.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.workers_aborted == 0
            && self.hooks_failed.is_empty()
            && self.subsystems_failed.is_empty()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn empty_report() -> ShutdownReport {
        let now = Instant::now();
        ShutdownReport {
            started_at: now,
            completed_at: now,
            workers_drained: 0,
            workers_aborted: 0,
            hooks_succeeded: Vec::new(),
            hooks_failed: Vec::new(),
            subsystems_shutdown: Vec::new(),
            subsystems_failed: Vec::new(),
        }
    }

    #[test]
    fn test_empty_report_is_clean() {
        assert!(empty_report().is_clean());
    }

    #[test]
    fn test_aborted_workers_make_report_unclean() {
        let mut r = empty_report();
        r.workers_aborted = 1;
        assert!(!r.is_clean());
    }

    #[test]
    fn test_failed_hook_makes_report_unclean() {
        let mut r = empty_report();
        r.hooks_failed.push(("flush", "disk full".to_owned()));
        assert!(!r.is_clean());
    }

    #[test]
    fn test_failed_subsystem_makes_report_unclean() {
        let mut r = empty_report();
        r.subsystems_failed.push(("storage", "io".to_owned()));
        assert!(!r.is_clean());
    }

    #[test]
    fn test_duration_uses_timestamps() {
        let now = Instant::now();
        let later = now.checked_add(Duration::from_secs(2)).unwrap();
        let r = ShutdownReport {
            started_at: now,
            completed_at: later,
            ..empty_report()
        };
        assert_eq!(r.duration(), Duration::from_secs(2));
    }
}
