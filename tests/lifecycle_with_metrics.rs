//! Integration test: `LifecycleController` wired to a
//! `MetricsHandle` emits the documented gauge and counter on every
//! successful transition, and emits nothing on failed transitions.

#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use service_kernel::lifecycle::{KernelState, LifecycleController, Phase};
use service_kernel::metrics::{names, MetricsBackend, MetricsHandle};

struct CaptureBackend {
    counter_calls: AtomicU64,
    last_gauge: Mutex<Option<f64>>,
    last_counter_label: Mutex<Option<String>>,
}

impl Default for CaptureBackend {
    fn default() -> Self {
        Self {
            counter_calls: AtomicU64::new(0),
            last_gauge: Mutex::new(None),
            last_counter_label: Mutex::new(None),
        }
    }
}

impl MetricsBackend for CaptureBackend {
    fn counter(&self, name: &str, value: u64, labels: &[(&str, &str)]) {
        if name == names::LIFECYCLE_TRANSITIONS {
            let _ = self.counter_calls.fetch_add(value, Ordering::Relaxed);
            if let Some((_, v)) = labels.iter().find(|(k, _)| *k == "to") {
                *self.last_counter_label.lock().unwrap() = Some((*v).to_owned());
            }
        }
    }
    fn gauge(&self, name: &str, value: f64, _: &[(&str, &str)]) {
        if name == names::LIFECYCLE_PHASE {
            *self.last_gauge.lock().unwrap() = Some(value);
        }
    }
    fn histogram(&self, _: &str, _: f64, _: &[(&str, &str)]) {}
}

#[test]
fn test_each_successful_transition_emits_one_gauge_and_one_counter() {
    let backend = Arc::new(CaptureBackend::default());
    let handle: MetricsHandle = Arc::clone(&backend) as MetricsHandle;
    let c = LifecycleController::with_metrics(handle);

    c.transition(KernelState::Booting).unwrap();
    c.transition(KernelState::Loading).unwrap();
    c.transition(KernelState::Running).unwrap();

    assert_eq!(backend.counter_calls.load(Ordering::Relaxed), 3);
    assert_eq!(
        backend.last_gauge.lock().unwrap().map(|v| v.round() as u8),
        Some(Phase::Exec.ordinal())
    );
    assert_eq!(
        backend.last_counter_label.lock().unwrap().as_deref(),
        Some("running")
    );
}

#[test]
fn test_failed_transition_emits_no_metrics() {
    let backend = Arc::new(CaptureBackend::default());
    let handle: MetricsHandle = Arc::clone(&backend) as MetricsHandle;
    let c = LifecycleController::with_metrics(handle);

    assert!(c.transition(KernelState::Running).is_err());
    assert_eq!(backend.counter_calls.load(Ordering::Relaxed), 0);
    assert!(backend.last_gauge.lock().unwrap().is_none());
}

#[test]
fn test_controller_without_metrics_still_works() {
    let c = LifecycleController::new();
    c.transition(KernelState::Booting).unwrap();
    c.transition(KernelState::Loading).unwrap();
    assert_eq!(c.state(), KernelState::Loading);
}

#[test]
fn test_with_events_and_metrics_combines_both_emitters() {
    use service_kernel::events::{EventDispatcher, KernelEvent, LifecycleEvent};

    let dispatcher = EventDispatcher::new();
    let event_count = Arc::new(AtomicU64::new(0));
    let cb = Arc::clone(&event_count);
    let _ = dispatcher.subscribe("kernel.lifecycle.booting", move |event| {
        if matches!(
            event,
            KernelEvent::Lifecycle(LifecycleEvent::Transition { .. })
        ) {
            let _ = cb.fetch_add(1, Ordering::Relaxed);
        }
    });

    let backend = Arc::new(CaptureBackend::default());
    let handle: MetricsHandle = Arc::clone(&backend) as MetricsHandle;
    let c = LifecycleController::with_events_and_metrics(dispatcher.handle(), handle);

    c.transition(KernelState::Booting).unwrap();
    assert_eq!(event_count.load(Ordering::Relaxed), 1);
    assert_eq!(backend.counter_calls.load(Ordering::Relaxed), 1);
}
