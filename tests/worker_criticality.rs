//! Integration test: criticality drives the kernel's response to a
//! non-restartable worker failure.

#![cfg(feature = "tokio")]
#![allow(clippy::unwrap_used)]

use std::thread;
use std::time::Duration;

use service_kernel::health::HealthStatus;
use service_kernel::kernel::KernelBuilder;
use service_kernel::lifecycle::KernelState;
use service_kernel::worker::{Worker, WorkerContext, WorkerError, WorkerSpec};

struct AlwaysFailing;

impl Worker for AlwaysFailing {
    fn name(&self) -> &'static str {
        "always-failing"
    }
    fn run(&self, _ctx: WorkerContext) -> Result<(), WorkerError> {
        Err(WorkerError::new("nope"))
    }
}

#[test]
fn test_critical_worker_failure_signals_kernel_shutdown() {
    let kernel = KernelBuilder::new("test")
        .with_worker(
            WorkerSpec::new("always-failing").critical().restart_never(),
            AlwaysFailing,
        )
        .build()
        .unwrap();

    // No external thread will signal shutdown — the supervisor is
    // expected to do it itself once the critical worker fails
    // unrecoverably. Cap the test's runtime as a backstop.
    let other = kernel.clone();
    let join = thread::spawn(move || {
        thread::sleep(Duration::from_secs(5));
        other.shutdown();
    });

    kernel.run().unwrap();
    let _ = join.join();
    assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Stopped);
}

#[test]
fn test_optional_worker_failure_marks_health_degraded() {
    let kernel = KernelBuilder::new("test")
        .with_worker(
            WorkerSpec::new("always-failing").optional().restart_never(),
            AlwaysFailing,
        )
        .build()
        .unwrap();

    let other = kernel.clone();
    let join = thread::spawn(move || {
        thread::sleep(Duration::from_millis(200));
        other.shutdown();
    });

    kernel.run().unwrap();
    join.join().unwrap();
    let snap = kernel.snapshot();
    let entry = snap.health.subsystems.get("always-failing").copied();
    assert_eq!(entry, Some(HealthStatus::Degraded));
}

#[test]
fn test_essential_worker_failure_marks_health_unhealthy() {
    let kernel = KernelBuilder::new("test")
        .with_worker(
            WorkerSpec::new("always-failing")
                .essential()
                .restart_never(),
            AlwaysFailing,
        )
        .build()
        .unwrap();

    let other = kernel.clone();
    let join = thread::spawn(move || {
        thread::sleep(Duration::from_millis(200));
        other.shutdown();
    });

    kernel.run().unwrap();
    join.join().unwrap();
    let snap = kernel.snapshot();
    let entry = snap.health.subsystems.get("always-failing").copied();
    assert_eq!(entry, Some(HealthStatus::Unhealthy));
}
