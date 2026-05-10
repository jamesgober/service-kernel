//! Integration tests for the metrics protocol.

#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use service_kernel::metrics::{
    kernel_metric_names, names, KernelMetric, MetricKind, MetricsBackend, MetricsHandle,
    NoopMetricsBackend,
};

#[derive(Default)]
struct InMemoryBackend {
    counters: Mutex<Vec<(String, u64)>>,
    gauges: Mutex<Vec<(String, f64)>>,
    histograms: Mutex<Vec<(String, f64)>>,
    counter_total: AtomicU64,
}

impl MetricsBackend for InMemoryBackend {
    fn counter(&self, name: &str, value: u64, _: &[(&str, &str)]) {
        self.counters
            .lock()
            .unwrap()
            .push((name.to_owned(), value));
        let _ = self.counter_total.fetch_add(value, Ordering::Relaxed);
    }
    fn gauge(&self, name: &str, value: f64, _: &[(&str, &str)]) {
        self.gauges.lock().unwrap().push((name.to_owned(), value));
    }
    fn histogram(&self, name: &str, value: f64, _: &[(&str, &str)]) {
        self.histograms
            .lock()
            .unwrap()
            .push((name.to_owned(), value));
    }
}

#[test]
fn test_noop_backend_is_safe_default() {
    let handle: MetricsHandle = Arc::new(NoopMetricsBackend);
    handle.counter("noop", 1, &[]);
    handle.gauge("noop", 1.0, &[]);
    handle.histogram("noop", 1.0, &[]);
}

#[test]
fn test_custom_backend_receives_inputs() {
    let backend = Arc::new(InMemoryBackend::default());
    let handle: MetricsHandle = Arc::clone(&backend) as MetricsHandle;
    handle.counter(names::LIFECYCLE_TRANSITIONS, 3, &[("to", "running")]);
    handle.gauge(names::LIFECYCLE_PHASE, 4.0, &[]);
    handle.histogram(names::SHUTDOWN_DURATION, 1.5, &[]);

    assert_eq!(backend.counter_total.load(Ordering::Relaxed), 3);
    assert_eq!(backend.counters.lock().unwrap().len(), 1);
    assert_eq!(backend.gauges.lock().unwrap().len(), 1);
    assert_eq!(backend.histograms.lock().unwrap().len(), 1);
}

#[test]
fn test_kernel_metric_enum_matches_constants() {
    assert_eq!(KernelMetric::LifecyclePhase.name(), names::LIFECYCLE_PHASE);
    assert_eq!(
        KernelMetric::ShutdownDuration.kind(),
        MetricKind::Histogram
    );
}

#[test]
fn test_kernel_metric_names_returns_all() {
    let all = kernel_metric_names();
    assert!(all.contains(&names::LIFECYCLE_PHASE));
    assert!(all.contains(&names::SHUTDOWN_DURATION));
    assert_eq!(all.len(), 8);
}

#[test]
fn test_arc_cloned_handles_share_backend() {
    let backend = Arc::new(InMemoryBackend::default());
    let h1: MetricsHandle = Arc::clone(&backend) as MetricsHandle;
    let h2: MetricsHandle = Arc::clone(&h1);
    h1.counter("a", 1, &[]);
    h2.counter("a", 1, &[]);
    assert_eq!(backend.counter_total.load(Ordering::Relaxed), 2);
}
