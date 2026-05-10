//! Five-level health-status vocabulary.
//!
//! [`HealthStatus`] is `Ord`. The order is `Healthy < Degraded <
//! Unhealthy < Critical < Unknown` — `Unknown` is the worst because
//! a subsystem that has not reported yet is treated as fail-safe.
//! The kernel's aggregate rule (`max` of per-subsystem statuses)
//! relies on this ordering; do not reorder the variants without
//! revisiting the registry's aggregation logic.

use std::fmt;

/// Operating condition of a subsystem.
///
/// Variants progress from healthy (the empty subset) to fully broken,
/// with `Unknown` past the end. `Unknown` is worse than `Critical`
/// for aggregation: a never-reported subsystem drags the aggregate
/// down so the operator notices.
///
/// Marked `#[non_exhaustive]` so future statuses can land without
/// breaking SemVer.
///
/// # Examples
///
/// ```
/// use service_kernel::health::HealthStatus;
///
/// assert!(HealthStatus::Healthy < HealthStatus::Degraded);
/// assert!(HealthStatus::Critical < HealthStatus::Unknown);
/// assert!(HealthStatus::Healthy.is_healthy());
/// assert!(HealthStatus::Unhealthy.is_actionable());
/// ```
#[non_exhaustive]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, PartialOrd, Ord)]
#[repr(u8)]
pub enum HealthStatus {
    /// Operating normally.
    Healthy = 0,
    /// Operating, with reduced quality of service.
    Degraded = 1,
    /// Failing in a way that requires attention.
    Unhealthy = 2,
    /// Critical failure; subsystem is at risk of total outage.
    Critical = 3,
    /// State has not been reported. Treated as worse than `Critical`.
    Unknown = 4,
}

impl HealthStatus {
    /// Returns the lowercase variant name as a static string.
    #[inline]
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            HealthStatus::Healthy => "healthy",
            HealthStatus::Degraded => "degraded",
            HealthStatus::Unhealthy => "unhealthy",
            HealthStatus::Critical => "critical",
            HealthStatus::Unknown => "unknown",
        }
    }

    /// Returns `true` only for [`HealthStatus::Healthy`].
    #[inline]
    #[must_use]
    pub const fn is_healthy(&self) -> bool {
        matches!(self, HealthStatus::Healthy)
    }

    /// Returns `true` for statuses an operator should look at —
    /// [`HealthStatus::Unhealthy`], [`HealthStatus::Critical`], and
    /// [`HealthStatus::Unknown`].
    #[inline]
    #[must_use]
    pub const fn is_actionable(&self) -> bool {
        matches!(
            self,
            HealthStatus::Unhealthy | HealthStatus::Critical | HealthStatus::Unknown
        )
    }

    /// Reconstructs a [`HealthStatus`] from its `u8` discriminant.
    ///
    /// Used internally by the lock-free atomic-aggregate path. Out-of-range
    /// values map to [`HealthStatus::Unknown`] — the safest fallback when
    /// a malformed read sneaks in.
    #[inline]
    #[must_use]
    pub(crate) const fn from_u8(byte: u8) -> Self {
        match byte {
            0 => HealthStatus::Healthy,
            1 => HealthStatus::Degraded,
            2 => HealthStatus::Unhealthy,
            3 => HealthStatus::Critical,
            _ => HealthStatus::Unknown,
        }
    }
}

impl Default for HealthStatus {
    /// Returns [`HealthStatus::Unknown`].
    ///
    /// The kernel treats unreported subsystems as fail-safe, so the
    /// default ranks worst on the aggregation scale.
    #[inline]
    fn default() -> Self {
        HealthStatus::Unknown
    }
}

impl fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            HealthStatus::Healthy => "HEALTHY",
            HealthStatus::Degraded => "DEGRADED",
            HealthStatus::Unhealthy => "UNHEALTHY",
            HealthStatus::Critical => "CRITICAL",
            HealthStatus::Unknown => "UNKNOWN",
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const ALL: [HealthStatus; 5] = [
        HealthStatus::Healthy,
        HealthStatus::Degraded,
        HealthStatus::Unhealthy,
        HealthStatus::Critical,
        HealthStatus::Unknown,
    ];

    #[test]
    fn test_as_str_values_are_unique() {
        let mut set = HashSet::new();
        for s in ALL {
            assert!(set.insert(s.as_str()));
        }
    }

    #[test]
    fn test_ordering_is_strictly_increasing() {
        for window in ALL.windows(2) {
            assert!(window[0] < window[1], "{:?} < {:?}", window[0], window[1]);
        }
    }

    #[test]
    fn test_unknown_ranks_worst() {
        for s in [
            HealthStatus::Healthy,
            HealthStatus::Degraded,
            HealthStatus::Unhealthy,
            HealthStatus::Critical,
        ] {
            assert!(s < HealthStatus::Unknown);
        }
    }

    #[test]
    fn test_is_healthy_only_for_healthy() {
        for s in ALL {
            assert_eq!(s.is_healthy(), matches!(s, HealthStatus::Healthy));
        }
    }

    #[test]
    fn test_is_actionable_for_unhealthy_critical_unknown() {
        for s in ALL {
            let expected = matches!(
                s,
                HealthStatus::Unhealthy | HealthStatus::Critical | HealthStatus::Unknown
            );
            assert_eq!(s.is_actionable(), expected, "{:?}", s);
        }
    }

    #[test]
    fn test_default_is_unknown() {
        assert_eq!(HealthStatus::default(), HealthStatus::Unknown);
    }

    #[test]
    fn test_display_is_uppercase_variant_name() {
        assert_eq!(HealthStatus::Healthy.to_string(), "HEALTHY");
        assert_eq!(HealthStatus::Unknown.to_string(), "UNKNOWN");
    }

    #[test]
    fn test_from_u8_round_trips_known_values() {
        for s in ALL {
            assert_eq!(HealthStatus::from_u8(s as u8), s);
        }
    }

    #[test]
    fn test_from_u8_unknown_byte_falls_back_to_unknown() {
        assert_eq!(HealthStatus::from_u8(255), HealthStatus::Unknown);
    }
}
