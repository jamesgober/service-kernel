//! Workers and supervision.
//!
//! Workers are the unit of supervised, long-running work. The
//! `Supervisor` catches panics, applies restart policy, tracks
//! per-worker state, and reports back to the kernel's events,
//! health, and metrics handles. The supervisor runs as a single
//! Tokio task driving a `tokio::select!` loop — not a per-worker
//! task storm.
//!
//! This module REQUIRES the `tokio` feature.
//!
//! # Runtime-agnostic vs Tokio-bound
//!
//! The kernel's core types ([`crate::lifecycle`], [`crate::events`],
//! [`crate::errors`], [`crate::health`], [`crate::metrics`]) stay
//! runtime-agnostic. The `worker` module is the first place Tokio
//! types appear in public signatures: `WorkerContext` holds a
//! `tokio_util::sync::CancellationToken`, and `Supervisor` uses
//! `tokio::task::JoinSet`. The runtime-agnostic-core rule applies
//! to everything outside `worker` and `shutdown`; this module is the
//! Tokio-bound boundary.

// Value-type submodules — always compiled. Pure data + std-only
// helpers. The Tokio-bound surface is gated below.
pub mod criticality;
pub mod event;
pub mod panic;
pub mod policy;
pub mod spec;
pub mod state;

// Tokio-bound submodules — feature-gated on `tokio`.
#[cfg(feature = "tokio")]
pub mod adapter;
#[cfg(feature = "tokio")]
pub mod context;
#[cfg(feature = "tokio")]
pub mod handle;
#[cfg(feature = "tokio")]
pub mod supervisor;
#[cfg(feature = "tokio")]
pub mod traits;
#[cfg(feature = "tokio")]
pub mod watchdog;

pub use criticality::Criticality;
pub use event::WorkerLifecycleEvent;
pub use panic::{catch_panic, PanicReason};
pub use policy::{BackoffPolicy, CircuitBreaker, CircuitPolicy, CircuitState, RestartPolicy};
pub use spec::WorkerSpec;
pub use state::WorkerState;

#[cfg(feature = "tokio")]
pub use context::WorkerContext;
#[cfg(feature = "tokio")]
pub use handle::WorkerHandle;
#[cfg(feature = "tokio")]
pub use supervisor::Supervisor;
#[cfg(feature = "tokio")]
pub use traits::{AsyncWorker, Worker, WorkerError};
#[cfg(feature = "tokio")]
pub use watchdog::{Watchdog, WatchdogTarget, WatchdogTimeout};
