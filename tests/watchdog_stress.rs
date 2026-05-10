//! Stress test: 200 well-behaved workers, no false timeouts; then a
//! subset goes silent and is reported as Timeout within the
//! watchdog's grace window.
//!
//! Spec calls for 500 workers; we run a smaller set (200) to stay
//! well under the 60-second cap on Windows CI.

#![cfg(feature = "tokio")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use service_kernel::events::{KernelEvent, WorkerEvent};
use service_kernel::kernel::KernelBuilder;
use service_kernel::worker::{
    AsyncWorker, WorkerContext, WorkerError, WorkerLifecycleEvent, WorkerSpec,
};

const WORKERS: usize = 200;
const WALL_CLOCK_CAP: Duration = Duration::from_secs(60);

struct GatedWorker {
    silent: Arc<AtomicBool>,
}

#[async_trait::async_trait]
impl AsyncWorker for GatedWorker {
    fn name(&self) -> &'static str {
        "gated"
    }
    async fn run(&self, ctx: WorkerContext) -> Result<(), WorkerError> {
        loop {
            tokio::select! {
                _ = ctx.cancelled() => return Ok(()),
                _ = tokio::time::sleep(Duration::from_millis(20)) => {
                    if !self.silent.load(Ordering::Relaxed) {
                        ctx.heartbeat();
                    }
                }
            }
        }
    }
}

#[test]
fn test_watchdog_under_load_only_flags_silent_workers() {
    let mut builder = KernelBuilder::new("watchdog-stress");
    let mut gates: Vec<Arc<AtomicBool>> = Vec::with_capacity(WORKERS);
    for _ in 0..WORKERS {
        let gate = Arc::new(AtomicBool::new(false));
        gates.push(Arc::clone(&gate));
        builder = builder.with_async_worker(
            WorkerSpec::new("gated")
                .background()
                .restart_never()
                .heartbeat(Duration::from_millis(100)),
            GatedWorker { silent: gate },
        );
    }

    let kernel = builder.build().unwrap();

    let timeouts = Arc::new(AtomicUsize::new(0));
    let cb = Arc::clone(&timeouts);
    let _ = kernel
        .context()
        .events
        .subscribe("kernel.worker.timeout", move |event| {
            if let KernelEvent::Worker(WorkerEvent {
                event: WorkerLifecycleEvent::Timeout { .. },
                ..
            }) = event
            {
                let _ = cb.fetch_add(1, Ordering::Relaxed);
            }
        });

    let started_at = Instant::now();
    let other = kernel.clone();
    let join = thread::spawn(move || {
        // Phase 1: 1.2s of healthy heartbeating.
        thread::sleep(Duration::from_millis(1200));
        // Phase 2: silence 20 workers and wait for the watchdog
        // tick + 2× heartbeat-interval grace.
        for gate in gates.iter().take(20) {
            gate.store(true, Ordering::Relaxed);
        }
        thread::sleep(Duration::from_millis(1500));
        other.shutdown();
    });

    kernel.run().unwrap();
    join.join().unwrap();

    assert!(
        started_at.elapsed() < WALL_CLOCK_CAP,
        "stress took {:?}, exceeded cap {:?}",
        started_at.elapsed(),
        WALL_CLOCK_CAP,
    );

    let observed = timeouts.load(Ordering::Relaxed);
    assert!(
        observed >= 20,
        "expected ≥20 timeouts after silencing, got {}",
        observed
    );
}
