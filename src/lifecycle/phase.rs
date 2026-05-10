//! Coarse-grained phase of the kernel's run cycle.
//!
//! Phase is the five-stage view of the lifecycle: where the kernel
//! is in its run cycle without saying anything about whether the
//! current step is healthy. The fine-grained view lives in
//! [`KernelState`](super::KernelState).
//!
//! `Phase` is internal output. `Phase::Display` writes the variant
//! name in uppercase and is intended for logs, metrics labels, and
//! event topic suffixes — not for end-user surfaces. User-visible
//! lifecycle strings live in [`TransitionError`](super::TransitionError)
//! and route through `lang_lib::t!`.

use std::fmt;

/// Coarse-grained lifecycle stage.
///
/// Variants progress in declaration order: `Idle` (before boot)
/// through `Shutdown` (teardown in progress or complete). The
/// numeric ordinal — exposed via [`Phase::ordinal`] — matches that
/// declaration order and is part of this enum's contract.
///
/// Marked `#[non_exhaustive]` so future phases can be introduced
/// without breaking SemVer.
///
/// # Examples
///
/// ```
/// use service_kernel::lifecycle::Phase;
///
/// assert_eq!(Phase::default(), Phase::Idle);
/// assert_eq!(Phase::Boot.as_str(), "boot");
/// assert!(Phase::Boot.ordinal() < Phase::Exec.ordinal());
/// ```
#[non_exhaustive]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
#[repr(u8)]
pub enum Phase {
    /// Before boot — no kernel work has started.
    Idle,
    /// Initial setup: logging, basics, kernel-owned subsystems wiring.
    Boot,
    /// Dependencies, registries, and consumer subsystems loading.
    Load,
    /// Steady-state execution.
    Exec,
    /// Teardown.
    Shutdown,
}

impl Phase {
    /// Returns the lowercase variant name as a static string.
    ///
    /// Used as both an event-topic suffix and a metrics label. The
    /// returned string is stable and part of this enum's contract.
    ///
    /// # Examples
    ///
    /// ```
    /// use service_kernel::lifecycle::Phase;
    ///
    /// assert_eq!(Phase::Idle.as_str(), "idle");
    /// assert_eq!(Phase::Shutdown.as_str(), "shutdown");
    /// ```
    #[inline]
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Boot => "boot",
            Phase::Load => "load",
            Phase::Exec => "exec",
            Phase::Shutdown => "shutdown",
        }
    }

    /// Returns the numeric ordinal in declaration order.
    ///
    /// `Phase::Idle` is `0`, `Phase::Shutdown` is `4`. The mapping
    /// is part of this enum's contract.
    #[inline]
    #[must_use]
    pub const fn ordinal(&self) -> u8 {
        match self {
            Phase::Idle => 0,
            Phase::Boot => 1,
            Phase::Load => 2,
            Phase::Exec => 3,
            Phase::Shutdown => 4,
        }
    }
}

impl Default for Phase {
    /// Returns [`Phase::Idle`].
    #[inline]
    fn default() -> Self {
        Phase::Idle
    }
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Phase::Idle => f.write_str("IDLE"),
            Phase::Boot => f.write_str("BOOT"),
            Phase::Load => f.write_str("LOAD"),
            Phase::Exec => f.write_str("EXEC"),
            Phase::Shutdown => f.write_str("SHUTDOWN"),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const ALL: [Phase; 5] = [
        Phase::Idle,
        Phase::Boot,
        Phase::Load,
        Phase::Exec,
        Phase::Shutdown,
    ];

    #[test]
    fn test_as_str_values_are_unique() {
        let mut set = HashSet::new();
        for phase in ALL {
            assert!(set.insert(phase.as_str()));
        }
        assert_eq!(set.len(), ALL.len());
    }

    #[test]
    fn test_ordinals_are_unique_and_match_declaration_order() {
        for (index, phase) in ALL.iter().enumerate() {
            assert_eq!(usize::from(phase.ordinal()), index);
        }
    }

    #[test]
    fn test_display_is_uppercase_variant_name() {
        assert_eq!(Phase::Idle.to_string(), "IDLE");
        assert_eq!(Phase::Boot.to_string(), "BOOT");
        assert_eq!(Phase::Load.to_string(), "LOAD");
        assert_eq!(Phase::Exec.to_string(), "EXEC");
        assert_eq!(Phase::Shutdown.to_string(), "SHUTDOWN");
    }

    #[test]
    fn test_default_is_idle() {
        assert_eq!(Phase::default(), Phase::Idle);
    }

    #[test]
    fn test_as_str_matches_lowercase_display() {
        for phase in ALL {
            assert_eq!(phase.as_str(), phase.to_string().to_lowercase());
        }
    }
}
