//! Full kernel feature demonstration.
//!
//! Exercises every major kernel feature in one example:
//!
//! - A custom subsystem that depends on a built-in (`events`).
//! - A custom error classifier with downcast-based classification.
//! - A custom metrics backend that prints to stdout.
//! - A supervised async worker with a heartbeat interval and a
//!   circuit-breaker policy.
//! - A shutdown hook that "flushes" something.
//! - OS signal handling — Ctrl+C triggers graceful shutdown.
//!
//! Run with: `cargo run --example full --features tokio`
//! Stop with: Ctrl+C, or wait 10 seconds for the auto-shutdown.

#[cfg(not(feature = "tokio"))]
fn main() {
    eprintln!("This example requires the `tokio` feature.");
    eprintln!("Run with: cargo run --example full --features tokio");
}

#[cfg(feature = "tokio")]
fn main() -> Result<(), service_kernel::errors::KernelError> {
    tokio_main::run()
}

#[cfg(feature = "tokio")]
mod tokio_main {
    use std::error::Error;
    use std::fmt;
    use std::time::Duration;

    use service_kernel::events::KernelEvent;
    use service_kernel::prelude::*;

    // ── Custom error type ─────────────────────────────────────────
    //
    // Defined here to demonstrate the classifier pattern. The
    // example does not actually emit one of these; a real consumer
    // would route errors through `ctx.errors.classify(&err)` from
    // inside a worker or subsystem.

    #[derive(Debug)]
    #[allow(dead_code)]
    enum AppError {
        Storage(String),
        Auth(String),
    }

    impl fmt::Display for AppError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                AppError::Storage(msg) => write!(f, "storage failure: {}", msg),
                AppError::Auth(msg) => write!(f, "auth failure: {}", msg),
            }
        }
    }

    impl Error for AppError {}

    // ── Custom error classifier ───────────────────────────────────

    struct AppClassifier;

    impl ErrorClassifier for AppClassifier {
        fn classify(&self, err: &(dyn Error + 'static)) -> Classification {
            if let Some(app) = err.downcast_ref::<AppError>() {
                return match app {
                    AppError::Storage(_) => Classification {
                        severity: Severity::Critical,
                        action: ErrorAction::EnterReadOnlyMode,
                        event_topic: Some("app.storage.failure"),
                    },
                    AppError::Auth(_) => Classification {
                        severity: Severity::Warning,
                        action: ErrorAction::LogOnly,
                        event_topic: None,
                    },
                };
            }
            Classification::default()
        }
    }

    // ── Custom metrics backend ────────────────────────────────────

    struct StdoutMetrics;

    impl MetricsBackend for StdoutMetrics {
        fn counter(&self, name: &str, value: u64, _labels: &[(&str, &str)]) {
            println!("[metric] {} += {}", name, value);
        }
        fn gauge(&self, name: &str, value: f64, _labels: &[(&str, &str)]) {
            println!("[metric] {} = {:.2}", name, value);
        }
        fn histogram(&self, name: &str, value: f64, _labels: &[(&str, &str)]) {
            println!("[metric] {} hist {:.4}", name, value);
        }
    }

    // ── Custom subsystem ──────────────────────────────────────────

    struct ConfigSubsystem;

    impl Subsystem for ConfigSubsystem {
        fn name(&self) -> &'static str {
            "config"
        }
        fn dependencies(&self) -> &'static [&'static str] {
            &["events"]
        }
        fn boot(&self, ctx: &KernelContext) -> Result<(), KernelError> {
            println!("[config] booting");
            ctx.health.report("config", HealthStatus::Healthy);
            Ok(())
        }
        fn shutdown(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
            println!("[config] shutting down");
            Ok(())
        }
    }

    // ── A worker that emits heartbeats ────────────────────────────

    struct WorkUnit;

    #[async_trait::async_trait]
    impl AsyncWorker for WorkUnit {
        fn name(&self) -> &'static str {
            "work-unit"
        }
        async fn run(&self, ctx: WorkerContext) -> Result<(), WorkerError> {
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            let _ = interval.tick().await;
            loop {
                tokio::select! {
                    _ = ctx.cancelled() => return Ok(()),
                    _ = interval.tick() => {
                        ctx.heartbeat();
                    }
                }
            }
        }
    }

    // ── Shutdown hook ─────────────────────────────────────────────

    struct FlushHook;

    #[async_trait::async_trait]
    impl ShutdownHook for FlushHook {
        fn name(&self) -> &'static str {
            "flush"
        }
        async fn run(&self, _ctx: &ShutdownContext) -> Result<(), HookError> {
            println!("[hook] flushing buffers");
            tokio::time::sleep(Duration::from_millis(100)).await;
            println!("[hook] flush complete");
            Ok(())
        }
    }

    pub fn run() -> Result<(), KernelError> {
        let kernel = KernelBuilder::new("full-demo")
            .with_subsystem(ConfigSubsystem)
            .with_error_classifier(AppClassifier)
            .with_metrics_backend(StdoutMetrics)
            .with_async_worker(
                WorkerSpec::new("work-unit")
                    .essential()
                    .restart_on_failure()
                    .heartbeat(Duration::from_secs(2))
                    .circuit(CircuitPolicy::default()),
                WorkUnit,
            )
            .with_shutdown_grace(Duration::from_secs(5))
            .build()
            .map_err(|e| KernelError::Internal {
                code: service_kernel::errors::KernelErrorCode::Internal,
                message: format!("build failed: {e}"),
            })?;

        kernel.register_shutdown_hook(FlushHook);

        // Subscribe to a few topics for visibility.
        let _ = kernel
            .context()
            .events
            .subscribe("kernel.worker.started", |event| {
                if let KernelEvent::Worker(_) = event {
                    println!("[worker] started");
                }
            });
        let _ = kernel
            .context()
            .events
            .subscribe("kernel.lifecycle.shutdown_started", |_| {
                println!("[lifecycle] shutdown started");
            });
        let _ = kernel
            .context()
            .events
            .subscribe("kernel.lifecycle.shutdown_completed", |_| {
                println!("[lifecycle] shutdown completed");
            });

        kernel.install_signal_handler();

        // Auto-shutdown after 10 seconds in case Ctrl+C is unavailable
        // (e.g. running under CI). Production deployments would skip
        // this watchdog.
        let other = kernel.clone();
        let join = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(10));
            other.shutdown();
        });

        println!("kernel running. press Ctrl+C to shut down (auto-shutdown after 10s).");
        let result = kernel.run();
        let _ = join.join();
        result
    }
}
