//! Integration test: restart policies behave per spec.

#![cfg(feature = "tokio")]
#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use service_kernel::kernel::KernelBuilder;
use service_kernel::worker::{Worker, WorkerContext, WorkerError, WorkerSpec};

struct FailingWorker {
    attempts: Arc<AtomicUsize>,
    until_success: usize,
}

impl Worker for FailingWorker {
    fn name(&self) -> &'static str {
        "failing"
    }
    fn run(&self, _ctx: WorkerContext) -> Result<(), WorkerError> {
        let n = self.attempts.fetch_add(1, Ordering::Relaxed);
        if n < self.until_success {
            Err(WorkerError::new(format!("attempt {} failing", n + 1)))
        } else {
            Ok(())
        }
    }
}

#[test]
fn test_restart_on_failure_eventually_succeeds() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let kernel = KernelBuilder::new("test")
        .with_worker(
            WorkerSpec::new("failing")
                .restart_on_failure()
                .backoff_fixed(Duration::from_millis(1)),
            FailingWorker {
                attempts: Arc::clone(&attempts),
                until_success: 3,
            },
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
    assert!(attempts.load(Ordering::Relaxed) >= 4);
}

#[test]
fn test_restart_never_does_not_re_spawn() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let kernel = KernelBuilder::new("test")
        .with_worker(
            WorkerSpec::new("failing").restart_never(),
            FailingWorker {
                attempts: Arc::clone(&attempts),
                until_success: 999,
            },
        )
        .build()
        .unwrap();

    let other = kernel.clone();
    let join = thread::spawn(move || {
        thread::sleep(Duration::from_millis(150));
        other.shutdown();
    });

    kernel.run().unwrap();
    join.join().unwrap();
    assert_eq!(attempts.load(Ordering::Relaxed), 1);
}

#[test]
fn test_max_retries_caps_attempts() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let kernel = KernelBuilder::new("test")
        .with_worker(
            WorkerSpec::new("failing")
                .restart_max_retries(3, Duration::from_secs(60))
                .backoff_fixed(Duration::from_millis(1)),
            FailingWorker {
                attempts: Arc::clone(&attempts),
                until_success: 999,
            },
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
    let observed = attempts.load(Ordering::Relaxed);
    // Initial run + up to 3 restarts = 4 attempts max.
    assert!(observed >= 1);
    assert!(observed <= 4);
}
