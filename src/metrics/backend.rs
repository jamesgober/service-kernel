//! [`MetricsBackend`] trait — the kernel's pluggable metrics surface.
//!
//! All three methods take primitive types (`&str`, `u64`/`f64`, label
//! tuples) so the trait does not bind consumers to any particular
//! metrics library. A `metrics-lib` adapter, if/when written, is a
//! separate impl of this trait.

/// Receiver of kernel-emitted metric observations.
///
/// Implementations are stored behind
/// [`MetricsHandle`](super::MetricsHandle) — i.e.
/// `Arc<dyn MetricsBackend>` — and shared across threads. The trait
/// is object-safe and `Send + Sync + 'static`.
///
/// # No-panic contract
///
/// **Implementations MUST NOT panic.** The kernel emits metrics from
/// hot paths (the supervisor loop, the lifecycle transition path)
/// and does not wrap these calls in `catch_unwind` — the cost would
/// dominate the metric itself. A panicking backend will unwind into
/// the emitter; the only safe response is to fix the backend.
///
/// Backends that do work which can fail (network sends, file I/O)
/// MUST buffer or batch internally and surface failures through
/// their own diagnostics — not by panicking.
///
/// # Example
///
/// ```
/// use std::sync::Mutex;
/// use service_kernel::metrics::MetricsBackend;
///
/// /// A backend that just logs the last counter increment.
/// struct LastCounter {
///     state: Mutex<Option<(String, u64)>>,
/// }
///
/// impl MetricsBackend for LastCounter {
///     fn counter(&self, name: &str, value: u64, _labels: &[(&str, &str)]) {
///         if let Ok(mut guard) = self.state.lock() {
///             *guard = Some((name.to_owned(), value));
///         }
///     }
///     fn gauge(&self, _name: &str, _value: f64, _labels: &[(&str, &str)]) {}
///     fn histogram(&self, _name: &str, _value: f64, _labels: &[(&str, &str)]) {}
/// }
///
/// let backend = LastCounter { state: Mutex::new(None) };
/// backend.counter("kernel.lifecycle.transitions", 1, &[("to", "running")]);
/// ```
pub trait MetricsBackend: Send + Sync + 'static {
    /// Records a counter increment.
    fn counter(&self, name: &str, value: u64, labels: &[(&str, &str)]);

    /// Records a gauge update (replaces the previous value).
    fn gauge(&self, name: &str, value: f64, labels: &[(&str, &str)]);

    /// Records a histogram observation.
    fn histogram(&self, name: &str, value: f64, labels: &[(&str, &str)]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    fn assert_send_sync<T: Send + Sync + ?Sized>() {}

    struct CountingBackend {
        counters: AtomicU64,
        gauges: AtomicU64,
        histograms: AtomicU64,
    }

    impl Default for CountingBackend {
        fn default() -> Self {
            Self {
                counters: AtomicU64::new(0),
                gauges: AtomicU64::new(0),
                histograms: AtomicU64::new(0),
            }
        }
    }

    impl MetricsBackend for CountingBackend {
        fn counter(&self, _: &str, _: u64, _: &[(&str, &str)]) {
            let _ = self.counters.fetch_add(1, Ordering::Relaxed);
        }
        fn gauge(&self, _: &str, _: f64, _: &[(&str, &str)]) {
            let _ = self.gauges.fetch_add(1, Ordering::Relaxed);
        }
        fn histogram(&self, _: &str, _: f64, _: &[(&str, &str)]) {
            let _ = self.histograms.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn test_trait_is_send_sync() {
        assert_send_sync::<dyn MetricsBackend>();
    }

    #[test]
    fn test_object_safe_via_arc() {
        let backend: Arc<dyn MetricsBackend> = Arc::new(CountingBackend::default());
        backend.counter("a", 1, &[]);
        backend.gauge("b", 1.0, &[]);
        backend.histogram("c", 1.0, &[]);
    }

    #[test]
    fn test_arc_clone_is_cheap() {
        let backend: Arc<dyn MetricsBackend> = Arc::new(CountingBackend::default());
        let cloned = Arc::clone(&backend);
        cloned.counter("a", 1, &[]);
        backend.counter("a", 1, &[]);
        // Both clones touch the same inner state.
    }

    #[test]
    fn test_empty_labels_are_supported() {
        let backend = CountingBackend::default();
        backend.counter("x", 1, &[]);
        assert_eq!(backend.counters.load(Ordering::Relaxed), 1);
    }
}
