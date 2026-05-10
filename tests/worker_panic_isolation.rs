//! Integration test: a panicking worker doesn't crash the kernel.

#![cfg(feature = "tokio")]
#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use service_kernel::events::{KernelEvent, WorkerEvent};
use service_kernel::kernel::KernelBuilder;
use service_kernel::worker::{Worker, WorkerContext, WorkerError, WorkerLifecycleEvent, WorkerSpec};

struct PanicSync;

impl Worker for PanicSync {
    fn name(&self) -> &'static str {
        "panic-sync"
    }
    fn run(&self, _ctx: WorkerContext) -> Result<(), WorkerError> {
        panic!("intentional sync panic");
    }
}

#[test]
fn test_sync_worker_panic_caught_and_emitted() {
    let kernel = KernelBuilder::new("test")
        .with_worker(WorkerSpec::new("panic-sync").restart_never(), PanicSync)
        .build()
        .unwrap();

    let panicked = Arc::new(AtomicUsize::new(0));
    let panicked_cb = Arc::clone(&panicked);
    let _ = kernel
        .context()
        .events
        .subscribe("kernel.worker.panicked", move |event| {
            if let KernelEvent::Worker(WorkerEvent {
                event: WorkerLifecycleEvent::Panicked { .. },
                ..
            }) = event
            {
                let _ = panicked_cb.fetch_add(1, Ordering::Relaxed);
            }
        });

    let other = kernel.clone();
    let join = thread::spawn(move || {
        thread::sleep(Duration::from_millis(200));
        other.shutdown();
    });

    kernel.run().unwrap();
    join.join().unwrap();
    assert_eq!(panicked.load(Ordering::Relaxed), 1);
}
