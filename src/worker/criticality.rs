//! Worker criticality vocabulary.
//!
//! [`Criticality`] is the supervisor's input for "how badly does the
//! kernel need this worker?". Restart policy decides whether to
//! retry; criticality decides what the kernel does when the retry
//! budget is exhausted — escalating from a quiet log up to
//! signalling kernel shutdown.

use std::fmt;

/// How essential a worker is to the kernel.
///
/// Variants are declared in increasing order of criticality so
/// callers can find the worst-case across a worker set with `max()`.
///
/// # Examples
///
/// ```
/// use service_kernel::worker::Criticality;
///
/// assert!(Criticality::Background < Criticality::Critical);
/// assert_eq!(Criticality::default(), Criticality::Optional);
/// ```
#[non_exhaustive]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, PartialOrd, Ord)]
#[repr(u8)]
pub enum Criticality {
    /// Best-effort. Failure is logged and forgotten.
    Background,
    /// Service runs without it. Failure marks health `Degraded`.
    Optional,
    /// Service runs but partially broken. Failure marks `Unhealthy`.
    Essential,
    /// Service cannot run. Failure signals kernel shutdown.
    Critical,
}

impl Criticality {
    /// Returns the lowercase variant name.
    #[inline]
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Criticality::Background => "background",
            Criticality::Optional => "optional",
            Criticality::Essential => "essential",
            Criticality::Critical => "critical",
        }
    }

    /// Returns the numeric ordinal in declaration order.
    #[inline]
    #[must_use]
    pub const fn ordinal(&self) -> u8 {
        match self {
            Criticality::Background => 0,
            Criticality::Optional => 1,
            Criticality::Essential => 2,
            Criticality::Critical => 3,
        }
    }
}

impl Default for Criticality {
    /// Returns [`Criticality::Optional`] — the safe middle.
    #[inline]
    fn default() -> Self {
        Criticality::Optional
    }
}

impl fmt::Display for Criticality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Criticality::Background => "BACKGROUND",
            Criticality::Optional => "OPTIONAL",
            Criticality::Essential => "ESSENTIAL",
            Criticality::Critical => "CRITICAL",
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const ALL: [Criticality; 4] = [
        Criticality::Background,
        Criticality::Optional,
        Criticality::Essential,
        Criticality::Critical,
    ];

    #[test]
    fn test_as_str_unique() {
        let mut set = HashSet::new();
        for c in ALL {
            assert!(set.insert(c.as_str()));
        }
    }

    #[test]
    fn test_ordering_is_strictly_increasing() {
        for window in ALL.windows(2) {
            assert!(window[0] < window[1], "{:?} < {:?}", window[0], window[1]);
        }
    }

    #[test]
    fn test_default_is_optional() {
        assert_eq!(Criticality::default(), Criticality::Optional);
    }

    #[test]
    fn test_display_is_uppercase() {
        assert_eq!(Criticality::Critical.to_string(), "CRITICAL");
    }
}
