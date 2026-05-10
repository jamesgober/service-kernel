//! Integration tests for the typed event bus.

#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use service_kernel::errors::{ErrorAction, Severity};
use service_kernel::events::{
    CustomEvent, ErrorEvent, EventDispatcher, KernelEvent, LifecycleEvent,
};
use service_kernel::lifecycle::KernelState;
use service_kernel::primitives::Instant;

fn lifecycle_running() -> KernelEvent {
    KernelEvent::Lifecycle(LifecycleEvent::Transition {
        from: KernelState::Loading,
        to: KernelState::Running,
        at: Instant::now(),
    })
}

#[test]
fn test_subscribe_and_emit_round_trip() {
    let d = EventDispatcher::new();
    let saw_running = Arc::new(AtomicUsize::new(0));
    let cb = Arc::clone(&saw_running);
    let _ = d.subscribe("kernel.lifecycle.running", move |event| {
        if matches!(
            event,
            KernelEvent::Lifecycle(LifecycleEvent::Transition {
                to: KernelState::Running,
                ..
            })
        ) {
            let _ = cb.fetch_add(1, Ordering::Relaxed);
        }
    });
    d.emit(lifecycle_running());
    assert_eq!(saw_running.load(Ordering::Relaxed), 1);
}

#[test]
fn test_unsubscribe_removes_handler_for_future_emits() {
    let d = EventDispatcher::new();
    let count = Arc::new(AtomicUsize::new(0));
    let cb = Arc::clone(&count);
    let id = d.subscribe("kernel.lifecycle.running", move |_| {
        let _ = cb.fetch_add(1, Ordering::Relaxed);
    });
    d.emit(lifecycle_running());
    assert!(d.unsubscribe(id));
    d.emit(lifecycle_running());
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

#[test]
fn test_handle_clones_share_routing_table() {
    let d = EventDispatcher::new();
    let h1 = d.handle();
    let h2 = h1.clone();

    let count = Arc::new(AtomicUsize::new(0));
    let cb = Arc::clone(&count);
    let _ = h1.subscribe("kernel.lifecycle.running", move |_| {
        let _ = cb.fetch_add(1, Ordering::Relaxed);
    });

    h2.emit(lifecycle_running());
    d.emit(lifecycle_running());
    assert_eq!(count.load(Ordering::Relaxed), 2);
}

#[test]
fn test_panicking_handler_does_not_crash_others() {
    let d = EventDispatcher::new();
    let count = Arc::new(AtomicUsize::new(0));
    let cb = Arc::clone(&count);

    let _ = d.subscribe("kernel.lifecycle.running", |_| panic!("oops"));
    let _ = d.subscribe("kernel.lifecycle.running", move |_| {
        let _ = cb.fetch_add(1, Ordering::Relaxed);
    });
    d.emit(lifecycle_running());
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

#[test]
fn test_emit_with_no_subscribers_is_noop() {
    let d = EventDispatcher::new();
    d.emit(KernelEvent::Error(ErrorEvent::new(
        Severity::Info,
        ErrorAction::LogOnly,
        "no listeners",
    )));
}

#[test]
fn test_subscriber_count_reflects_registrations() {
    let d = EventDispatcher::new();
    assert_eq!(d.subscriber_count("kernel.lifecycle.running"), 0);
    let id = d.subscribe("kernel.lifecycle.running", |_| {});
    assert_eq!(d.subscriber_count("kernel.lifecycle.running"), 1);
    assert!(d.unsubscribe(id));
    assert_eq!(d.subscriber_count("kernel.lifecycle.running"), 0);
}

#[test]
fn test_custom_event_payload_downcasts_for_subscribers() {
    #[derive(Debug)]
    struct AppPayload {
        n: u32,
    }

    let d = EventDispatcher::new();
    let observed = Arc::new(AtomicUsize::new(0));
    let cb = Arc::clone(&observed);
    let _ = d.subscribe("kernel.custom", move |event| {
        if let KernelEvent::Custom(custom) = event {
            if let Some(p) = custom.downcast_ref::<AppPayload>() {
                let _ = cb.fetch_add(p.n as usize, Ordering::Relaxed);
            }
        }
    });

    d.emit(KernelEvent::Custom(CustomEvent::new(
        "myapp.payload",
        AppPayload { n: 7 },
    )));
    assert_eq!(observed.load(Ordering::Relaxed), 7);
}
