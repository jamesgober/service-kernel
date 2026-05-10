//! Metrics protocol and adapters.
//!
//! The kernel ships a backend-agnostic [`MetricsBackend`] trait.
//! Consumers plug in whatever they prefer (Prometheus, StatsD,
//! `metrics-lib`, an internal aggregator, …); the kernel emits its
//! own counters, gauges, and histograms through the trait without
//! taking a stance on the wire format.
//!
//! When the consumer does not supply a backend, the kernel uses
//! [`NoopMetricsBackend`]. The compiler optimizes the no-op calls to
//! nothing, so a kernel that does not care about metrics pays no
//! cost beyond a handful of vtable dispatches that never go anywhere.

pub mod backend;
pub mod kernel_metrics;
pub mod noop;
pub mod snapshot;

pub use backend::MetricsBackend;
pub use kernel_metrics::{kernel_metric_names, names, KernelMetric, MetricKind};
pub use noop::NoopMetricsBackend;
pub use snapshot::{MetricValue, MetricsSnapshot};

use std::sync::Arc;

/// Type alias for the shared metrics backend.
///
/// `MetricsHandle` is what the lifecycle controller, the supervisor
/// (Milestone F+), and consumer subsystems hold. Constructed from a
/// concrete backend with `Arc::new(...)`.
pub type MetricsHandle = Arc<dyn MetricsBackend>;
