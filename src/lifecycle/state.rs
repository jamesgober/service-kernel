//! Fine-grained kernel runtime status.
//!
//! [`KernelState`] is the eight-variant view of "where the kernel is
//! and how is it going". It refines [`Phase`] with health information
//! (Running vs. Degraded), shutdown subdivision (Stopping vs. Stopped),
//! and the terminal Failed bucket.
//!
//! Like [`Phase`], `KernelState::Display` is internal output (logs,
//! metrics, event topics, debug). User-visible status text lives in
//! the error types that route through `lang_lib::t!`.

use std::fmt;

use super::Phase;

/// Fine-grained runtime status of the kernel.
///
/// Variants are declared in the natural progression order:
/// `Created → Booting → Loading → Running → Degraded → Stopping →
/// Stopped`. `Failed` is a terminal "anything went wrong"
/// destination reachable from any non-terminal state. The set of
/// legal transitions between states is enforced by
/// [`super::transition`].
///
/// Marked `#[non_exhaustive]` so future states can be added without
/// breaking SemVer.
///
/// # Examples
///
/// ```
/// use service_kernel::lifecycle::{KernelState, Phase};
///
/// assert_eq!(KernelState::default(), KernelState::Created);
/// assert_eq!(KernelState::Running.phase(), Phase::Exec);
/// assert!(KernelState::Stopped.is_terminal());
/// assert!(KernelState::Running.is_running());
/// ```
#[non_exhaustive]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
#[repr(u8)]
pub enum KernelState {
    /// Constructed but no boot work has started.
    Created,
    /// Boot phase is in progress.
    Booting,
    /// Load phase is in progress.
    Loading,
    /// Steady-state, healthy.
    Running,
    /// Steady-state, partial failure tolerated.
    Degraded,
    /// Shutdown is in progress.
    Stopping,
    /// Shutdown completed cleanly. Terminal.
    Stopped,
    /// An unrecoverable error occurred. Terminal.
    Failed,
}

impl KernelState {
    /// Returns the lowercase variant name as a static string.
    ///
    /// Used as event-topic suffix and metrics label.
    #[inline]
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            KernelState::Created => "created",
            KernelState::Booting => "booting",
            KernelState::Loading => "loading",
            KernelState::Running => "running",
            KernelState::Degraded => "degraded",
            KernelState::Stopping => "stopping",
            KernelState::Stopped => "stopped",
            KernelState::Failed => "failed",
        }
    }

    /// Returns the [`Phase`] that contains this state.
    #[inline]
    #[must_use]
    pub const fn phase(&self) -> Phase {
        match self {
            KernelState::Created => Phase::Idle,
            KernelState::Booting => Phase::Boot,
            KernelState::Loading => Phase::Load,
            KernelState::Running | KernelState::Degraded => Phase::Exec,
            KernelState::Stopping | KernelState::Stopped | KernelState::Failed => {
                Phase::Shutdown
            }
        }
    }

    /// Returns `true` if the state has no outgoing transitions.
    ///
    /// Only [`KernelState::Stopped`] and [`KernelState::Failed`] are
    /// terminal.
    #[inline]
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, KernelState::Stopped | KernelState::Failed)
    }

    /// Returns `true` if the state is one of the steady-state
    /// running states.
    ///
    /// Both [`KernelState::Running`] and [`KernelState::Degraded`]
    /// qualify; degraded is still "running, but not at full health".
    #[inline]
    #[must_use]
    pub const fn is_running(&self) -> bool {
        matches!(self, KernelState::Running | KernelState::Degraded)
    }
}

impl Default for KernelState {
    /// Returns [`KernelState::Created`].
    #[inline]
    fn default() -> Self {
        KernelState::Created
    }
}

impl fmt::Display for KernelState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KernelState::Created => f.write_str("CREATED"),
            KernelState::Booting => f.write_str("BOOTING"),
            KernelState::Loading => f.write_str("LOADING"),
            KernelState::Running => f.write_str("RUNNING"),
            KernelState::Degraded => f.write_str("DEGRADED"),
            KernelState::Stopping => f.write_str("STOPPING"),
            KernelState::Stopped => f.write_str("STOPPED"),
            KernelState::Failed => f.write_str("FAILED"),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    pub(crate) const ALL: [KernelState; 8] = [
        KernelState::Created,
        KernelState::Booting,
        KernelState::Loading,
        KernelState::Running,
        KernelState::Degraded,
        KernelState::Stopping,
        KernelState::Stopped,
        KernelState::Failed,
    ];

    #[test]
    fn test_as_str_values_are_unique() {
        let mut set = HashSet::new();
        for state in ALL {
            assert!(set.insert(state.as_str()));
        }
        assert_eq!(set.len(), ALL.len());
    }

    #[test]
    fn test_phase_mapping_per_variant() {
        assert_eq!(KernelState::Created.phase(), Phase::Idle);
        assert_eq!(KernelState::Booting.phase(), Phase::Boot);
        assert_eq!(KernelState::Loading.phase(), Phase::Load);
        assert_eq!(KernelState::Running.phase(), Phase::Exec);
        assert_eq!(KernelState::Degraded.phase(), Phase::Exec);
        assert_eq!(KernelState::Stopping.phase(), Phase::Shutdown);
        assert_eq!(KernelState::Stopped.phase(), Phase::Shutdown);
        assert_eq!(KernelState::Failed.phase(), Phase::Shutdown);
    }

    #[test]
    fn test_is_terminal_only_for_stopped_and_failed() {
        for state in ALL {
            let expected =
                matches!(state, KernelState::Stopped | KernelState::Failed);
            assert_eq!(state.is_terminal(), expected, "{:?}", state);
        }
    }

    #[test]
    fn test_is_running_only_for_running_and_degraded() {
        for state in ALL {
            let expected =
                matches!(state, KernelState::Running | KernelState::Degraded);
            assert_eq!(state.is_running(), expected, "{:?}", state);
        }
    }

    #[test]
    fn test_default_is_created() {
        assert_eq!(KernelState::default(), KernelState::Created);
    }

    #[test]
    fn test_display_is_uppercase_variant_name() {
        assert_eq!(KernelState::Created.to_string(), "CREATED");
        assert_eq!(KernelState::Booting.to_string(), "BOOTING");
        assert_eq!(KernelState::Loading.to_string(), "LOADING");
        assert_eq!(KernelState::Running.to_string(), "RUNNING");
        assert_eq!(KernelState::Degraded.to_string(), "DEGRADED");
        assert_eq!(KernelState::Stopping.to_string(), "STOPPING");
        assert_eq!(KernelState::Stopped.to_string(), "STOPPED");
        assert_eq!(KernelState::Failed.to_string(), "FAILED");
    }
}
