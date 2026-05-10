//! Default metrics backend that records nothing.
//!
//! [`NoopMetricsBackend`] is the kernel's fallback when the consumer
//! does not register a real backend. The compiler optimizes the
//! method bodies to nothing in release builds, so an unused metrics
//! pipeline costs only the indirect call through `Arc<dyn
//! MetricsBackend>`.

use super::MetricsBackend;

/// No-op metrics backend.
///
/// Use as the default when no consumer-supplied backend is wired in:
///
/// ```
/// use std::sync::Arc;
/// use service_kernel::metrics::{MetricsBackend, MetricsHandle, NoopMetricsBackend};
///
/// let backend: MetricsHandle = Arc::new(NoopMetricsBackend);
/// backend.counter("example", 1, &[]);
/// ```
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopMetricsBackend;

impl MetricsBackend for NoopMetricsBackend {
    #[inline]
    fn counter(&self, _name: &str, _value: u64, _labels: &[(&str, &str)]) {}

    #[inline]
    fn gauge(&self, _name: &str, _value: f64, _labels: &[(&str, &str)]) {}

    #[inline]
    fn histogram(&self, _name: &str, _value: f64, _labels: &[(&str, &str)]) {}
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::metrics::MetricsHandle;
    use std::sync::Arc;

    #[test]
    fn test_noop_backend_does_nothing_observable() {
        let b = NoopMetricsBackend;
        b.counter("a", 1, &[]);
        b.gauge("b", 1.0, &[]);
        b.histogram("c", 1.0, &[]);
    }

    #[test]
    fn test_noop_backend_works_behind_arc_dyn_metricsbackend() {
        let handle: MetricsHandle = Arc::new(NoopMetricsBackend);
        handle.counter("x", 1, &[("k", "v")]);
        handle.gauge("y", 0.5, &[("k", "v")]);
        handle.histogram("z", 0.5, &[("k", "v")]);
    }
}
