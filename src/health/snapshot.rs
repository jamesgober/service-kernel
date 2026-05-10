//! Read-only view of the health registry.

use std::collections::HashMap;

use crate::primitives::Instant;

use super::HealthStatus;

/// Snapshot of the kernel's health state at a point in time.
///
/// Produced by [`HealthRegistry::snapshot`](super::HealthRegistry::snapshot)
/// and [`HealthHandle::snapshot`](super::HealthHandle::snapshot). The
/// snapshot is fully owned — clone it, hand it to other threads, or
/// serialize it into a status endpoint without re-locking the
/// registry.
#[derive(Debug, Clone)]
pub struct HealthSnapshot {
    /// Aggregate status across all subsystems at snapshot time.
    pub aggregate: HealthStatus,
    /// Per-subsystem statuses keyed by stable name.
    pub subsystems: HashMap<&'static str, HealthStatus>,
    /// Wall-clock instant at which this snapshot was taken.
    pub timestamp: Instant,
}

impl HealthSnapshot {
    /// Returns the number of subsystems whose status equals `status`.
    #[inline]
    #[must_use]
    pub fn count_by_status(&self, status: HealthStatus) -> usize {
        self.subsystems.values().filter(|s| **s == status).count()
    }

    /// Returns the names of subsystems whose status is
    /// [`HealthStatus::is_actionable`] (i.e. `Unhealthy`, `Critical`,
    /// or `Unknown`).
    ///
    /// Names are sorted alphabetically so snapshot tests are stable.
    #[must_use]
    pub fn unhealthy_subsystems(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = self
            .subsystems
            .iter()
            .filter_map(|(name, status)| {
                if status.is_actionable() {
                    Some(*name)
                } else {
                    None
                }
            })
            .collect();
        out.sort_unstable();
        out
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn empty_snapshot() -> HealthSnapshot {
        HealthSnapshot {
            aggregate: HealthStatus::Healthy,
            subsystems: HashMap::new(),
            timestamp: Instant::now(),
        }
    }

    fn snapshot_with(entries: &[(&'static str, HealthStatus)]) -> HealthSnapshot {
        let mut subsystems = HashMap::new();
        for (name, status) in entries {
            let _ = subsystems.insert(*name, *status);
        }
        let aggregate = subsystems
            .values()
            .copied()
            .max()
            .unwrap_or(HealthStatus::Healthy);
        HealthSnapshot {
            aggregate,
            subsystems,
            timestamp: Instant::now(),
        }
    }

    #[test]
    fn test_empty_snapshot_has_no_unhealthy_subsystems() {
        let snap = empty_snapshot();
        assert_eq!(snap.aggregate, HealthStatus::Healthy);
        assert_eq!(snap.unhealthy_subsystems(), Vec::<&str>::new());
        assert_eq!(snap.count_by_status(HealthStatus::Healthy), 0);
    }

    #[test]
    fn test_count_by_status_returns_correct_counts() {
        let snap = snapshot_with(&[
            ("a", HealthStatus::Healthy),
            ("b", HealthStatus::Healthy),
            ("c", HealthStatus::Degraded),
            ("d", HealthStatus::Critical),
        ]);
        assert_eq!(snap.count_by_status(HealthStatus::Healthy), 2);
        assert_eq!(snap.count_by_status(HealthStatus::Degraded), 1);
        assert_eq!(snap.count_by_status(HealthStatus::Critical), 1);
        assert_eq!(snap.count_by_status(HealthStatus::Unknown), 0);
    }

    #[test]
    fn test_unhealthy_subsystems_returns_sorted_actionable_names() {
        let snap = snapshot_with(&[
            ("zebra", HealthStatus::Unhealthy),
            ("apple", HealthStatus::Healthy),
            ("mango", HealthStatus::Critical),
            ("banana", HealthStatus::Unknown),
        ]);
        assert_eq!(
            snap.unhealthy_subsystems(),
            vec!["banana", "mango", "zebra"]
        );
    }
}
