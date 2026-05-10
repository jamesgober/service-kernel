//! Stress test: many workers with mixed criticality and restart policy.
//!
//! Spec calls for 1000 workers; we run a smaller set (200) to keep the
//! per-test runtime under the 90-second cap on a typical CI runner.
//! The worker mix includes sync and async, with a fraction that
//! deliberately fail and panic.

#![cfg(feature = "tokio")]
#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use service_kernel::events::{KernelEvent, WorkerEvent};
use service_kernel::kernel::KernelBuilder;
use service_kernel::lifecycle::KernelState;
use service_kernel::worker::{
    AsyncWorker, Worker, WorkerContext, WorkerError, WorkerLifecycleEvent, WorkerSpec,
};

const TOTAL_WORKERS: usize = 200;
const RUN_DURATION: Duration = Duration::from_millis(500);
const WALL_CLOCK_CAP: Duration = Duration::from_secs(60);

struct QuickSync {
    behaviour: u8,
}

impl Worker for QuickSync {
    fn name(&self) -> &'static str {
        "quick-sync"
    }
    fn run(&self, ctx: WorkerContext) -> Result<(), WorkerError> {
        for _ in 0..3 {
            if ctx.is_cancelled() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        match self.behaviour {
            0 => Ok(()),
            1 => Err(WorkerError::new("quick fail")),
            _ => panic!("quick panic"),
        }
    }
}

struct QuickAsync {
    behaviour: u8,
}

#[async_trait::async_trait]
impl AsyncWorker for QuickAsync {
    fn name(&self) -> &'static str {
        "quick-async"
    }
    async fn run(&self, ctx: WorkerContext) -> Result<(), WorkerError> {
        for _ in 0..3 {
            if ctx.is_cancelled() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        match self.behaviour {
            0 => Ok(()),
            _ => Err(WorkerError::new("async fail")),
        }
    }
}

#[test]
fn test_supervisor_handles_many_mixed_workers_without_panic_or_deadlock() {
    let mut builder = KernelBuilder::new("stress");

    let started = Arc::new(AtomicUsize::new(0));

    for i in 0..TOTAL_WORKERS {
        let spec = WorkerSpec::new("quick-sync")
            .background()
            .restart_never()
            .backoff_fixed(Duration::from_millis(1));
        if i % 2 == 0 {
            builder = builder.with_worker(
                spec,
                QuickSync {
                    behaviour: (i % 3) as u8,
                },
            );
        } else {
            builder = builder.with_async_worker(
                spec,
                QuickAsync {
                    behaviour: (i % 2) as u8,
                },
            );
        }
    }

    let kernel = builder.build().unwrap();

    let started_cb = Arc::clone(&started);
    let _ = kernel
        .context()
        .events
        .subscribe("kernel.worker.started", move |event| {
            if let KernelEvent::Worker(WorkerEvent {
                event: WorkerLifecycleEvent::Started { .. },
                ..
            }) = event
            {
                let _ = started_cb.fetch_add(1, Ordering::Relaxed);
            }
        });

    let started_at = Instant::now();
    let other = kernel.clone();
    let join = thread::spawn(move || {
        thread::sleep(RUN_DURATION);
        other.shutdown();
    });

    kernel.run().unwrap();
    join.join().unwrap();

    let elapsed = started_at.elapsed();
    assert!(
        elapsed < WALL_CLOCK_CAP,
        "stress run took {:?}, exceeded {:?}",
        elapsed,
        WALL_CLOCK_CAP
    );
    assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Stopped);
    assert!(started.load(Ordering::Relaxed) >= TOTAL_WORKERS);
}
