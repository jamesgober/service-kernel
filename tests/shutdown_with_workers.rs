//! Integration test: workers + shutdown coordinator run together.

#![cfg(feature = "tokio")]
#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use service_kernel::events::{KernelEvent, LifecycleEvent};
use service_kernel::kernel::KernelBuilder;
use service_kernel::lifecycle::KernelState;
use service_kernel::shutdown::{HookError, ShutdownContext, ShutdownHook};
use service_kernel::worker::{AsyncWorker, WorkerContext, WorkerError, WorkerSpec};

struct LongRunning;

#[async_trait::async_trait]
impl AsyncWorker for LongRunning {
    fn name(&self) -> &'static str {
        "long-running"
    }
    async fn run(&self, ctx: WorkerContext) -> Result<(), WorkerError> {
        loop {
            tokio::select! {
                _ = ctx.cancelled() => return Ok(()),
                _ = tokio::time::sleep(Duration::from_millis(10)) => {
                    ctx.heartbeat();
                }
            }
        }
    }
}

struct OrderTracker {
    name: &'static str,
    counter: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl ShutdownHook for OrderTracker {
    fn name(&self) -> &'static str {
        self.name
    }
    async fn run(&self, _ctx: &ShutdownContext) -> Result<(), HookError> {
        let _ = self.counter.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[test]
fn test_workers_and_hooks_both_run_during_shutdown() {
    let kernel = KernelBuilder::new("test")
        .with_async_worker(
            WorkerSpec::new("long-running").restart_never(),
            LongRunning,
        )
        .with_shutdown_grace(Duration::from_secs(2))
        .build()
        .unwrap();

    let hook_runs = Arc::new(AtomicUsize::new(0));
    kernel.register_shutdown_hook(OrderTracker {
        name: "test-hook",
        counter: Arc::clone(&hook_runs),
    });

    let completed = Arc::new(AtomicUsize::new(0));
    let cb = Arc::clone(&completed);
    let _ = kernel
        .context()
        .events
        .subscribe("kernel.lifecycle.shutdown_completed", move |event| {
            if matches!(
                event,
                KernelEvent::Lifecycle(LifecycleEvent::ShutdownCompleted { .. })
            ) {
                let _ = cb.fetch_add(1, Ordering::Relaxed);
            }
        });

    let other = kernel.clone();
    let join = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        other.shutdown();
    });
    kernel.run().unwrap();
    join.join().unwrap();

    assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Stopped);
    assert_eq!(hook_runs.load(Ordering::Relaxed), 1);
    assert_eq!(completed.load(Ordering::Relaxed), 1);
}
