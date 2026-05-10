//! Supervised workers example.
//!
//! Runs a kernel with two workers:
//!
//! - A critical async worker that emits a heartbeat every second.
//! - An optional sync worker that fails its first three runs to
//!   demonstrate the restart policy with exponential backoff.
//!
//! Run with: `cargo run --example workers --features tokio`

#[cfg(not(feature = "tokio"))]
fn main() {
    eprintln!("This example requires the `tokio` feature.");
    eprintln!("Run with: cargo run --example workers --features tokio");
}

#[cfg(feature = "tokio")]
fn main() -> Result<(), service_kernel::errors::KernelError> {
    tokio_main::run()
}

#[cfg(feature = "tokio")]
mod tokio_main {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use service_kernel::events::{KernelEvent, WorkerEvent};
    use service_kernel::prelude::*;

    struct HeartbeatWorker;

    #[async_trait::async_trait]
    impl AsyncWorker for HeartbeatWorker {
        fn name(&self) -> &'static str {
            "heartbeat"
        }

        async fn run(&self, ctx: WorkerContext) -> Result<(), WorkerError> {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            // Skip the first instant tick so output lines up with wall-clock seconds.
            let _ = interval.tick().await;
            loop {
                tokio::select! {
                    _ = ctx.cancelled() => return Ok(()),
                    _ = interval.tick() => {
                        ctx.heartbeat();
                        println!("[heartbeat] tick");
                    }
                }
            }
        }
    }

    struct FlakeyWorker {
        runs: Arc<AtomicU64>,
    }

    impl Worker for FlakeyWorker {
        fn name(&self) -> &'static str {
            "flakey"
        }

        fn run(&self, ctx: WorkerContext) -> Result<(), WorkerError> {
            let n = self.runs.fetch_add(1, Ordering::SeqCst);
            if n < 3 {
                std::thread::sleep(Duration::from_millis(100));
                return Err(WorkerError::new(format!("flakey failure #{}", n + 1)));
            }
            while !ctx.is_cancelled() {
                ctx.heartbeat();
                std::thread::sleep(Duration::from_millis(500));
            }
            Ok(())
        }
    }

    pub fn run() -> Result<(), KernelError> {
        let runs = Arc::new(AtomicU64::new(0));

        let kernel = KernelBuilder::new("workers-demo")
            .with_async_worker(
                WorkerSpec::new("heartbeat")
                    .critical()
                    .restart_on_failure()
                    .heartbeat(Duration::from_secs(2)),
                HeartbeatWorker,
            )
            .with_worker(
                WorkerSpec::new("flakey")
                    .optional()
                    .restart_max_retries(5, Duration::from_secs(10))
                    .backoff_exponential(Duration::from_millis(100), Duration::from_secs(2)),
                FlakeyWorker {
                    runs: Arc::clone(&runs),
                },
            )
            .build()
            .map_err(|e| KernelError::Internal {
                code: service_kernel::errors::KernelErrorCode::Internal,
                message: format!("build failed: {e}"),
            })?;

        // Subscribe to a couple of worker topics so the example prints
        // the supervisor's actions in real time.
        for topic in [
            "kernel.worker.started",
            "kernel.worker.failed",
            "kernel.worker.restarted",
            "kernel.worker.stopped",
        ] {
            let _ = kernel.context().events.subscribe(topic, |event| {
                if let KernelEvent::Worker(WorkerEvent { event: lc, .. }) = event {
                    println!("[supervisor] {}", lc.kind());
                }
            });
        }

        // Run for 10 seconds, then signal shutdown.
        let other = kernel.clone();
        let join = thread::spawn(move || {
            thread::sleep(Duration::from_secs(10));
            other.shutdown();
        });

        let result = kernel.run();
        let _ = join.join();
        println!(
            "[main] flakey ran {} times before stable",
            runs.load(Ordering::Relaxed)
        );
        result
    }
}
