//! Read-only snapshot of recent metric values.
//!
//! [`MetricsSnapshot`] is consumed by the future kernel-wide
//! `Kernel::snapshot()` API (Milestone E+). At this milestone, only
//! the type definition lands — no producer in the kernel populates
//! it yet. Defining it now lets later milestones add fields and
//! producers without re-shaping the type.

use std::collections::HashMap;

use crate::primitives::Instant;

/// One observed metric value.
///
/// `Histogram` carries a bounded sample buffer — callers (typically
/// admin endpoints) decide how many samples to ship in a snapshot.
/// The bound itself is a backend concern.
#[derive(Debug, Clone)]
pub enum MetricValue {
    /// Counter total.
    Counter(u64),
    /// Gauge value.
    Gauge(f64),
    /// Recent histogram observations.
    Histogram(Vec<f64>),
}

/// Snapshot of the kernel's metrics at a point in time.
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    /// Wall-clock instant at which this snapshot was taken.
    pub timestamp: Instant,
    /// Kernel-emitted metrics keyed by stable name.
    pub kernel_metrics: HashMap<&'static str, MetricValue>,
}

impl MetricsSnapshot {
    /// Constructs an empty snapshot stamped with the current time.
    #[inline]
    #[must_use]
    pub fn empty() -> Self {
        Self {
            timestamp: Instant::now(),
            kernel_metrics: HashMap::new(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn assert_send_sync_clone<T: Send + Sync + Clone>() {}

    #[test]
    fn test_metrics_snapshot_is_send_sync_clone() {
        assert_send_sync_clone::<MetricsSnapshot>();
        assert_send_sync_clone::<MetricValue>();
    }

    #[test]
    fn test_empty_snapshot_is_empty() {
        let snap = MetricsSnapshot::empty();
        assert!(snap.kernel_metrics.is_empty());
    }

    #[test]
    fn test_metric_value_debug_round_trips() {
        let counter = MetricValue::Counter(42);
        let gauge = MetricValue::Gauge(0.75);
        let hist = MetricValue::Histogram(vec![1.0, 2.0]);
        assert!(format!("{:?}", counter).contains("Counter"));
        assert!(format!("{:?}", gauge).contains("Gauge"));
        assert!(format!("{:?}", hist).contains("Histogram"));
    }

    #[test]
    fn test_kernel_metrics_round_trip() {
        let mut snap = MetricsSnapshot::empty();
        let _ = snap.kernel_metrics.insert(
            crate::metrics::names::LIFECYCLE_PHASE,
            MetricValue::Gauge(2.0),
        );
        assert!(snap
            .kernel_metrics
            .contains_key(crate::metrics::names::LIFECYCLE_PHASE));
    }
}
