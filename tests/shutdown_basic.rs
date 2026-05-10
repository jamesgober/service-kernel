//! Integration test: full kernel lifecycle with shutdown coordinator.

#![cfg(feature = "tokio")]
#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use service_kernel::events::{KernelEvent, LifecycleEvent};
use service_kernel::kernel::KernelBuilder;
use service_kernel::lifecycle::KernelState;

#[test]
fn test_shutdown_emits_started_and_completed_events() {
    let kernel = KernelBuilder::new("test").build().unwrap();

    let started = Arc::new(AtomicUsize::new(0));
    let completed = Arc::new(AtomicUsize::new(0));
    {
        let s_cb = Arc::clone(&started);
        let _ =
            kernel
                .context()
                .events
                .subscribe("kernel.lifecycle.shutdown_started", move |event| {
                    if matches!(
                        event,
                        KernelEvent::Lifecycle(LifecycleEvent::ShutdownStarted { .. })
                    ) {
                        let _ = s_cb.fetch_add(1, Ordering::Relaxed);
                    }
                });
        let c_cb = Arc::clone(&completed);
        let _ = kernel.context().events.subscribe(
            "kernel.lifecycle.shutdown_completed",
            move |event| {
                if matches!(
                    event,
                    KernelEvent::Lifecycle(LifecycleEvent::ShutdownCompleted { .. })
                ) {
                    let _ = c_cb.fetch_add(1, Ordering::Relaxed);
                }
            },
        );
    }

    let other = kernel.clone();
    let join = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        other.shutdown();
    });
    kernel.run().unwrap();
    join.join().unwrap();

    assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Stopped);
    assert_eq!(started.load(Ordering::Relaxed), 1);
    assert_eq!(completed.load(Ordering::Relaxed), 1);
}

#[test]
fn test_shutdown_token_is_signalled_after_run() {
    let kernel = KernelBuilder::new("test").build().unwrap();
    let token = kernel.shutdown_token();
    let other = kernel.clone();
    let join = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        other.shutdown();
    });
    kernel.run().unwrap();
    join.join().unwrap();
    assert!(token.is_signalled());
}
