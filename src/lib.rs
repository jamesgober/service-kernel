//! # Service Kernel
//!
//! Runtime control plane for resilient Rust services.
//!
//! `service-kernel` provides a shared backbone for long-running Rust
//! services: lifecycle phase management, supervised workers, typed
//! event routing, error policy, health and metrics registries, and
//! graceful shutdown coordination. It does not replace Tokio. It does
//! not own business logic. It coordinates the runtime concerns that
//! every serious service ends up rebuilding from scratch.
//!
//! ## Status
//!
//! `0.1.x` — pre-stable. The public API may shift in patch releases
//! through `0.1.x` until the `0.2.0` boundary. Do not depend on this
//! crate from production code yet without pinning the exact patch.
//!
//! ## Quick start
//!
//! Build a kernel with [`KernelBuilder`](kernel::KernelBuilder),
//! optionally register subsystems and workers, and call
//! [`Kernel::run`](kernel::Kernel::run). A minimal sync-only example:
//!
//! ```
//! use std::thread;
//! use std::time::Duration;
//!
//! use service_kernel::prelude::*;
//!
//! # fn _docs_quick_start() -> Result<(), Box<dyn std::error::Error>> {
//! let kernel = KernelBuilder::new("my-service").build()?;
//!
//! // Signal shutdown after a short delay so this example terminates.
//! let other = kernel.clone();
//! let handle = thread::spawn(move || {
//!     thread::sleep(Duration::from_millis(20));
//!     other.shutdown();
//! });
//!
//! kernel.run()?;
//! handle.join().unwrap();
//! # Ok(())
//! # }
//! ```
//!
//! For supervised workers and the full feature set, see the
//! examples under `examples/` (run with
//! `cargo run --example minimal`, `--example workers`, or
//! `--example full`).
//!
//! ## Module map
//!
//! | Module | Owns |
//! |---|---|
//! | [`primitives`] | `Global<T>`, strongly-typed ids, time primitives. |
//! | [`lifecycle`] | `Phase`, `KernelState`, transition table, `LifecycleController`. |
//! | [`errors`] | `Severity`, `ErrorAction`, `ErrorClassifier`, `ErrorRegistry`, `KernelError`. |
//! | [`events`] | Topic-keyed dispatcher, `KernelEvent` and sub-event types. |
//! | [`health`] | Per-subsystem state, push-based aggregation, snapshots. |
//! | [`metrics`] | Backend-agnostic counter/gauge/histogram trait, no-op default. |
//! | [`worker`] | `Worker`/`AsyncWorker` traits, supervisor, watchdog, circuit breaker (gated on `tokio`). |
//! | `shutdown` | Cooperative `ShutdownToken`, generic drain helper, hooks, coordinator (gated on `tokio`). |
//! | [`kernel`] | The fluent builder + assembled `Kernel` that wires everything together. |
//!
//! ## Feature flags
//!
//! | Feature | Default | What it adds |
//! |---|---|---|
//! | `tokio` | off | `Worker` / `AsyncWorker`, supervisor, watchdog, circuit breaker, drain, signal handler. |
//! | `daemon` | off | `proc_daemon` adapter (PID file + working dir lifecycle). Implies `tokio`. |
//! | `errors` | off | First-party `ErrorClassifier` impl backed by `error-forge`. |
//! | `metrics` | off | First-party `MetricsBackend` impl backed by `metrics-lib`. |
//! | `hardware` | off | Hardware-probing re-export of `fsys`. |
//!
//! ## Layering
//!
//! ```text
//! Tokio
//!   ↓
//! service-kernel
//!   ↓
//! consumer (hive-core, artex services, third-party apps)
//! ```
//!
//! The kernel coordinates. The consumer owns business logic.

#![deny(warnings)]
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(unused_must_use)]
#![deny(unused_results)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]
#![deny(clippy::dbg_macro)]
#![deny(clippy::unreachable)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(clippy::missing_safety_doc)]

pub mod errors;
pub mod events;
pub mod health;
pub mod kernel;
pub mod lifecycle;
pub mod metrics;
pub mod primitives;
#[cfg(feature = "tokio")]
pub mod shutdown;
pub mod worker;

/// Convenience re-exports for common kernel types.
///
/// Imports everything a typical kernel consumer needs to wire up
/// and observe a service. Internal-leaning types (`Global<T>`,
/// `Interval`, `IdGenerator`, `SubscriptionId`, `KernelErrorCode`,
/// `NoopClassifier`, the placeholder event types) are intentionally
/// not re-exported here — pull those from their home modules when
/// needed.
pub mod prelude {
    pub use crate::errors::{Classification, ErrorAction, ErrorClassifier, KernelError, Severity};
    pub use crate::events::{EventDispatcher, EventHandle, KernelEvent, LifecycleEvent};
    pub use crate::health::{HealthHandle, HealthSnapshot, HealthStatus};
    pub use crate::kernel::{
        ErrorHandle, Kernel, KernelBuilder, KernelContext, KernelSnapshot, LifecycleHandle,
        ShutdownHandle, Subsystem,
    };
    pub use crate::lifecycle::{KernelState, LifecycleSnapshot, Phase};
    pub use crate::metrics::{MetricsBackend, MetricsHandle, NoopMetricsBackend};
    pub use crate::primitives::{Deadline, Instant, KernelId, SubsystemId, WorkerId};
    pub use crate::worker::{
        BackoffPolicy, CircuitPolicy, CircuitState, Criticality, RestartPolicy,
        WorkerLifecycleEvent, WorkerSpec, WorkerState,
    };

    #[cfg(feature = "tokio")]
    pub use crate::worker::{AsyncWorker, Worker, WorkerContext, WorkerError, WorkerHandle};

    #[cfg(feature = "tokio")]
    pub use crate::shutdown::{
        HookError, ShutdownContext, ShutdownCoordinator, ShutdownHook, ShutdownReport,
        ShutdownToken,
    };
}
