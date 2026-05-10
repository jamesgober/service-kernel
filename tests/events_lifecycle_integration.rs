//! Integration test: `LifecycleController` wired to an
//! `EventHandle` emits `LifecycleEvent::Transition` on every
//! successful transition, and emits nothing on rejected transitions.

#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use service_kernel::events::{EventDispatcher, KernelEvent, LifecycleEvent};
use service_kernel::lifecycle::{KernelState, LifecycleController, Phase};

#[test]
fn test_controller_emits_on_every_successful_transition() {
    let dispatcher = EventDispatcher::new();
    let observed = Arc::new(Mutex::new(Vec::<(KernelState, KernelState)>::new()));

    for state_str in [
        "kernel.lifecycle.booting",
        "kernel.lifecycle.loading",
        "kernel.lifecycle.running",
        "kernel.lifecycle.stopping",
        "kernel.lifecycle.stopped",
    ] {
        let observed = Arc::clone(&observed);
        let _ = dispatcher.subscribe(state_str, move |event| {
            if let KernelEvent::Lifecycle(LifecycleEvent::Transition { from, to, .. }) = event {
                observed.lock().unwrap().push((*from, *to));
            }
        });
    }

    let controller = LifecycleController::with_events(dispatcher.handle());
    controller.transition(KernelState::Booting).unwrap();
    controller.transition(KernelState::Loading).unwrap();
    controller.transition(KernelState::Running).unwrap();
    controller.transition(KernelState::Stopping).unwrap();
    controller.transition(KernelState::Stopped).unwrap();

    let log = observed.lock().unwrap();
    assert_eq!(
        *log,
        vec![
            (KernelState::Created, KernelState::Booting),
            (KernelState::Booting, KernelState::Loading),
            (KernelState::Loading, KernelState::Running),
            (KernelState::Running, KernelState::Stopping),
            (KernelState::Stopping, KernelState::Stopped),
        ]
    );
}

#[test]
fn test_controller_does_not_emit_on_failed_transition() {
    let dispatcher = EventDispatcher::new();
    let total = Arc::new(AtomicUsize::new(0));
    let cb = Arc::clone(&total);
    let _ = dispatcher.subscribe("kernel.lifecycle.running", move |_| {
        let _ = cb.fetch_add(1, Ordering::Relaxed);
    });

    let controller = LifecycleController::with_events(dispatcher.handle());
    assert!(controller.transition(KernelState::Running).is_err());
    assert_eq!(total.load(Ordering::Relaxed), 0);
    assert_eq!(controller.state(), KernelState::Created);
    assert_eq!(controller.phase(), Phase::Idle);
}

#[test]
fn test_controller_without_events_works_silently() {
    let controller = LifecycleController::new();
    controller.transition(KernelState::Booting).unwrap();
    controller.transition(KernelState::Loading).unwrap();
    assert_eq!(controller.state(), KernelState::Loading);
}

#[test]
fn test_emitted_event_carries_correct_from_to_at() {
    let dispatcher = EventDispatcher::new();
    let captured = Arc::new(Mutex::new(None));
    let cb = Arc::clone(&captured);

    let _ = dispatcher.subscribe("kernel.lifecycle.booting", move |event| {
        if let KernelEvent::Lifecycle(LifecycleEvent::Transition { from, to, at }) = event {
            *cb.lock().unwrap() = Some((*from, *to, *at));
        }
    });

    let controller = LifecycleController::with_events(dispatcher.handle());
    let snapshot_before = controller.snapshot();
    controller.transition(KernelState::Booting).unwrap();
    let snapshot_after = controller.snapshot();

    let (from, to, at) = captured.lock().unwrap().expect("expected event");
    assert_eq!(from, KernelState::Created);
    assert_eq!(to, KernelState::Booting);
    assert!(at >= snapshot_before.last_transition);
    assert_eq!(at, snapshot_after.last_transition);
}
