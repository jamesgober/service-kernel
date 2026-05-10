//! Integration test: register a worker, run a kernel briefly, observe events.

#![cfg(feature = "tokio")]
#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use service_kernel::events::{KernelEvent, WorkerEvent};
use service_kernel::kernel::KernelBuilder;
use service_kernel::worker::{
    AsyncWorker, Worker, WorkerContext, WorkerError, WorkerLifecycleEvent, WorkerSpec,
};

struct CounterWorker {
    iterations: Arc<AtomicUsize>,
}

impl Worker for CounterWorker {
    fn name(&self) -> &'static str {
        "counter"
    }
    fn run(&self, ctx: WorkerContext) -> Result<(), WorkerError> {
        let mut n = 0;
        while !ctx.is_cancelled() {
            n += 1;
            ctx.heartbeat();
            std::thread::sleep(Duration::from_millis(2));
            if n >= 5 {
                let _ = self.iterations.fetch_add(n, Ordering::Relaxed);
                return Ok(());
            }
        }
        let _ = self.iterations.fetch_add(n, Ordering::Relaxed);
        Ok(())
    }
}

struct AsyncCounter {
    iterations: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl AsyncWorker for AsyncCounter {
    fn name(&self) -> &'static str {
        "async-counter"
    }
    async fn run(&self, ctx: WorkerContext) -> Result<(), WorkerError> {
        for _ in 0..5 {
            tokio::select! {
                _ = ctx.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_millis(2)) => {
                    let _ = self.iterations.fetch_add(1, Ordering::Relaxed);
                    ctx.heartbeat();
                }
            }
        }
        Ok(())
    }
}

#[test]
fn test_sync_worker_runs_and_emits_started_event() {
    let iterations = Arc::new(AtomicUsize::new(0));
    let kernel = KernelBuilder::new("test")
        .with_worker(
            WorkerSpec::new("counter").restart_never(),
            CounterWorker {
                iterations: Arc::clone(&iterations),
            },
        )
        .build()
        .unwrap();

    let started = Arc::new(AtomicUsize::new(0));
    let started_cb = Arc::clone(&started);
    let _ = kernel.context().events.subscribe("kernel.worker.started", move |event| {
        if let KernelEvent::Worker(WorkerEvent {
            event: WorkerLifecycleEvent::Started { .. },
            ..
        }) = event
        {
            let _ = started_cb.fetch_add(1, Ordering::Relaxed);
        }
    });

    let other = kernel.clone();
    let join = thread::spawn(move || {
        thread::sleep(Duration::from_millis(200));
        other.shutdown();
    });

    kernel.run().unwrap();
    join.join().unwrap();
    assert!(started.load(Ordering::Relaxed) >= 1);
    assert!(iterations.load(Ordering::Relaxed) >= 5);
}

#[test]
fn test_async_worker_runs_and_completes() {
    let iterations = Arc::new(AtomicUsize::new(0));
    let kernel = KernelBuilder::new("test")
        .with_async_worker(
            WorkerSpec::new("async-counter").restart_never(),
            AsyncCounter {
                iterations: Arc::clone(&iterations),
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
    assert!(iterations.load(Ordering::Relaxed) >= 5);
}

#[test]
fn test_kernel_without_workers_uses_sync_path() {
    // No tokio runtime needed when no workers are registered.
    let kernel = KernelBuilder::new("test").build().unwrap();
    let other = kernel.clone();
    let join = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        other.shutdown();
    });
    kernel.run().unwrap();
    join.join().unwrap();
}
