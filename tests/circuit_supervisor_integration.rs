//! Integration test: full circuit-open / half-open / close cycle
//! through the supervisor.

#![cfg(feature = "tokio")]
#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use service_kernel::events::{KernelEvent, WorkerEvent};
use service_kernel::kernel::KernelBuilder;
use service_kernel::worker::{
    CircuitPolicy, Worker, WorkerContext, WorkerError, WorkerLifecycleEvent, WorkerSpec,
};

struct AlwaysFails;

impl Worker for AlwaysFails {
    fn name(&self) -> &'static str {
        "always-fails"
    }
    fn run(&self, _ctx: WorkerContext) -> Result<(), WorkerError> {
        Err(WorkerError::new("nope"))
    }
}

#[test]
fn test_circuit_opens_after_threshold_failures() {
    let opened = Arc::new(AtomicUsize::new(0));

    let kernel = KernelBuilder::new("test")
        .with_worker(
            WorkerSpec::new("always-fails")
                .restart_always()
                .backoff_fixed(Duration::from_millis(1))
                .circuit(CircuitPolicy::new(
                    2,
                    Duration::from_secs(60),
                    Duration::from_secs(30),
                )),
            AlwaysFails,
        )
        .build()
        .unwrap();

    let cb = Arc::clone(&opened);
    let _ = kernel
        .context()
        .events
        .subscribe("kernel.worker.circuit_opened", move |event| {
            if let KernelEvent::Worker(WorkerEvent {
                event: WorkerLifecycleEvent::CircuitOpened { .. },
                ..
            }) = event
            {
                let _ = cb.fetch_add(1, Ordering::Relaxed);
            }
        });

    let other = kernel.clone();
    let join = thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        other.shutdown();
    });

    kernel.run().unwrap();
    join.join().unwrap();
    assert!(
        opened.load(Ordering::Relaxed) >= 1,
        "expected at least one CircuitOpened event"
    );
}

#[test]
fn test_circuit_closes_on_successful_trial() {
    use std::sync::atomic::AtomicU32;

    struct FailNTimes {
        remaining: AtomicU32,
    }

    impl Worker for FailNTimes {
        fn name(&self) -> &'static str {
            "fail-n"
        }
        fn run(&self, _ctx: WorkerContext) -> Result<(), WorkerError> {
            let n = self.remaining.fetch_sub(1, Ordering::Relaxed);
            if n > 0 {
                Err(WorkerError::new("nope"))
            } else {
                Ok(())
            }
        }
    }

    let kernel = KernelBuilder::new("test")
        .with_worker(
            WorkerSpec::new("fail-n")
                .restart_always()
                .backoff_fixed(Duration::from_millis(1))
                .circuit(CircuitPolicy::new(
                    2,
                    Duration::from_secs(60),
                    Duration::from_millis(50),
                )),
            FailNTimes {
                remaining: AtomicU32::new(2),
            },
        )
        .build()
        .unwrap();

    let opened = Arc::new(AtomicUsize::new(0));
    let half_opened = Arc::new(AtomicUsize::new(0));
    let closed = Arc::new(AtomicUsize::new(0));
    {
        let opened = Arc::clone(&opened);
        let _ = kernel
            .context()
            .events
            .subscribe("kernel.worker.circuit_opened", move |_| {
                let _ = opened.fetch_add(1, Ordering::Relaxed);
            });
        let half_opened = Arc::clone(&half_opened);
        let _ = kernel
            .context()
            .events
            .subscribe("kernel.worker.circuit_half_opened", move |_| {
                let _ = half_opened.fetch_add(1, Ordering::Relaxed);
            });
        let closed = Arc::clone(&closed);
        let _ = kernel
            .context()
            .events
            .subscribe("kernel.worker.circuit_closed", move |_| {
                let _ = closed.fetch_add(1, Ordering::Relaxed);
            });
    }

    let other = kernel.clone();
    let join = thread::spawn(move || {
        // Need enough time for: 2 failures → open, watchdog tick (1s)
        // → half-open, trial run → close.
        thread::sleep(Duration::from_millis(2500));
        other.shutdown();
    });

    kernel.run().unwrap();
    join.join().unwrap();
    assert!(
        opened.load(Ordering::Relaxed) >= 1,
        "expected CircuitOpened"
    );
    assert!(
        half_opened.load(Ordering::Relaxed) >= 1,
        "expected CircuitHalfOpened"
    );
    assert!(
        closed.load(Ordering::Relaxed) >= 1,
        "expected CircuitClosed"
    );
}
