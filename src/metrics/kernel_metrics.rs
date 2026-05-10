//! Stable identifiers for the kernel's own metrics.
//!
//! [`KernelMetric`] enumerates the metrics the kernel emits today.
//! Metric **names** are exported as `const &str` from the [`names`]
//! module so emit sites can use the literal directly without going
//! through the enum:
//!
//! ```
//! use service_kernel::metrics::{names, NoopMetricsBackend, MetricsBackend};
//!
//! let backend = NoopMetricsBackend;
//! backend.counter(names::LIFECYCLE_TRANSITIONS, 1, &[("to", "running")]);
//! ```
//!
//! The enum exists so consumers, exporters, and admin endpoints can
//! discover the kernel's stable metric set programmatically without
//! string-matching against the constants. The kernel does NOT route
//! emits through the enum — the constants are the source of truth.

/// Stable kernel metric names.
pub mod names {
    /// Current lifecycle phase as a numeric ordinal (gauge).
    pub const LIFECYCLE_PHASE: &str = "kernel.lifecycle.phase";

    /// Total successful lifecycle transitions (counter).
    pub const LIFECYCLE_TRANSITIONS: &str = "kernel.lifecycle.transitions";

    /// Total errors emitted, labelled by severity (counter).
    pub const ERRORS_BY_SEVERITY: &str = "kernel.errors";

    /// Health aggregate as a numeric ordinal (gauge).
    pub const HEALTH_AGGREGATE: &str = "kernel.health.aggregate";

    /// Number of currently-running supervised workers (gauge).
    pub const WORKERS_RUNNING: &str = "kernel.workers.running";

    /// Total worker failures (counter).
    pub const WORKERS_FAILED: &str = "kernel.workers.failed";

    /// Total worker restarts (counter).
    pub const WORKERS_RESTARTED: &str = "kernel.workers.restarted";

    /// Time taken to complete graceful shutdown (histogram).
    pub const SHUTDOWN_DURATION: &str = "kernel.shutdown.duration";
}

/// Kind of metric a [`KernelMetric`] is.
///
/// Determines which [`MetricsBackend`](super::MetricsBackend) method
/// the kernel emits through.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum MetricKind {
    /// Monotonic counter — only increments.
    Counter,
    /// Gauge — replaces the previous value on each set.
    Gauge,
    /// Histogram — accumulates observations over time.
    Histogram,
}

/// Stable kernel-emitted metric metadata.
///
/// Each variant maps to a name in [`names`] and a [`MetricKind`].
/// Marked `#[non_exhaustive]` so future metrics land without breaking
/// SemVer.
#[non_exhaustive]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum KernelMetric {
    /// Current lifecycle phase. Gauge over phase ordinals (0–4).
    LifecyclePhase,
    /// Total successful lifecycle transitions.
    LifecycleTransitions,
    /// Total errors observed, labelled by severity.
    ErrorsBySeverity,
    /// Health aggregate ordinal.
    HealthAggregate,
    /// Currently-running worker count.
    WorkersRunning,
    /// Total worker failures.
    WorkersFailed,
    /// Total worker restarts.
    WorkersRestarted,
    /// Shutdown durations (histogram).
    ShutdownDuration,
}

impl KernelMetric {
    /// Returns the metric's stable name.
    #[inline]
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            KernelMetric::LifecyclePhase => names::LIFECYCLE_PHASE,
            KernelMetric::LifecycleTransitions => names::LIFECYCLE_TRANSITIONS,
            KernelMetric::ErrorsBySeverity => names::ERRORS_BY_SEVERITY,
            KernelMetric::HealthAggregate => names::HEALTH_AGGREGATE,
            KernelMetric::WorkersRunning => names::WORKERS_RUNNING,
            KernelMetric::WorkersFailed => names::WORKERS_FAILED,
            KernelMetric::WorkersRestarted => names::WORKERS_RESTARTED,
            KernelMetric::ShutdownDuration => names::SHUTDOWN_DURATION,
        }
    }

    /// Returns the metric's kind.
    #[inline]
    #[must_use]
    pub const fn kind(&self) -> MetricKind {
        match self {
            KernelMetric::LifecyclePhase
            | KernelMetric::HealthAggregate
            | KernelMetric::WorkersRunning => MetricKind::Gauge,
            KernelMetric::LifecycleTransitions
            | KernelMetric::ErrorsBySeverity
            | KernelMetric::WorkersFailed
            | KernelMetric::WorkersRestarted => MetricKind::Counter,
            KernelMetric::ShutdownDuration => MetricKind::Histogram,
        }
    }
}

/// Returns every kernel metric name in declaration order.
///
/// Exposed so admin endpoints, exporters, and tests can iterate the
/// kernel's metric set without hard-coding the list.
#[must_use]
pub const fn kernel_metric_names() -> &'static [&'static str] {
    &[
        names::LIFECYCLE_PHASE,
        names::LIFECYCLE_TRANSITIONS,
        names::ERRORS_BY_SEVERITY,
        names::HEALTH_AGGREGATE,
        names::WORKERS_RUNNING,
        names::WORKERS_FAILED,
        names::WORKERS_RESTARTED,
        names::SHUTDOWN_DURATION,
    ]
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const ALL: [KernelMetric; 8] = [
        KernelMetric::LifecyclePhase,
        KernelMetric::LifecycleTransitions,
        KernelMetric::ErrorsBySeverity,
        KernelMetric::HealthAggregate,
        KernelMetric::WorkersRunning,
        KernelMetric::WorkersFailed,
        KernelMetric::WorkersRestarted,
        KernelMetric::ShutdownDuration,
    ];

    #[test]
    fn test_every_variant_has_unique_name() {
        let mut set = HashSet::new();
        for m in ALL {
            assert!(set.insert(m.name()));
        }
        assert_eq!(set.len(), ALL.len());
    }

    #[test]
    fn test_kinds_match_specification() {
        assert_eq!(KernelMetric::LifecyclePhase.kind(), MetricKind::Gauge);
        assert_eq!(
            KernelMetric::LifecycleTransitions.kind(),
            MetricKind::Counter
        );
        assert_eq!(KernelMetric::ErrorsBySeverity.kind(), MetricKind::Counter);
        assert_eq!(KernelMetric::HealthAggregate.kind(), MetricKind::Gauge);
        assert_eq!(KernelMetric::WorkersRunning.kind(), MetricKind::Gauge);
        assert_eq!(KernelMetric::WorkersFailed.kind(), MetricKind::Counter);
        assert_eq!(KernelMetric::WorkersRestarted.kind(), MetricKind::Counter);
        assert_eq!(KernelMetric::ShutdownDuration.kind(), MetricKind::Histogram);
    }

    #[test]
    fn test_kernel_metric_names_lists_all() {
        let names = kernel_metric_names();
        assert_eq!(names.len(), ALL.len());
        for m in ALL {
            assert!(names.contains(&m.name()));
        }
    }

    #[test]
    fn test_constants_match_enum_names() {
        assert_eq!(KernelMetric::LifecyclePhase.name(), names::LIFECYCLE_PHASE);
        assert_eq!(
            KernelMetric::ShutdownDuration.name(),
            names::SHUTDOWN_DURATION
        );
    }
}
