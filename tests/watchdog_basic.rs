//! Integration test: a worker that stops heartbeating is reported
//! as a Timeout event by the supervisor's watchdog.

#![cfg(feature = "tokio")]
#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use service_kernel::events::{KernelEvent, WorkerEvent};
use service_kernel::kernel::KernelBuilder;
use service_kernel::worker::{
    AsyncWorker, WorkerContext, WorkerError, WorkerLifecycleEvent, WorkerSpec,
};

struct HeartbeatThenSilent {
    early_pulses: u32,
}

#[async_trait::async_trait]
impl AsyncWorker for HeartbeatThenSilent {
    fn name(&self) -> &'static str {
        "heartbeat-then-silent"
    }
    async fn run(&self, ctx: WorkerContext) -> Result<(), WorkerError> {
        for _ in 0..self.early_pulses {
            tokio::time::sleep(Duration::from_millis(20)).await;
            ctx.heartbeat();
        }
        // Stay alive but never heartbeat again.
        loop {
            tokio::select! {
                _ = ctx.cancelled() => return Ok(()),
                _ = tokio::time::sleep(Duration::from_millis(50)) => {}
            }
        }
    }
}

#[test]
fn test_silent_worker_eventually_reported_as_timeout() {
    let kernel = KernelBuilder::new("test")
        .with_async_worker(
            WorkerSpec::new("heartbeat-then-silent")
                .restart_never()
                .heartbeat(Duration::from_millis(50)),
            HeartbeatThenSilent { early_pulses: 3 },
        )
        .build()
        .unwrap();

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

    let other = kernel.clone();
    let join = thread::spawn(move || {
        // Allow enough time for: early heartbeats (~60ms) + silence
        // longer than 2 × 50ms = 100ms grace + watchdog tick (1s default).
        thread::sleep(Duration::from_millis(2200));
        other.shutdown();
    });

    kernel.run().unwrap();
    join.join().unwrap();
    assert!(
        timeouts.load(Ordering::Relaxed) >= 1,
        "expected at least one Timeout event"
    );
}
