//! Kernel subsystem loader and the public consumer-facing API.
//!
//! This module composes the kernel's built-in subsystems (lifecycle,
//! events, errors, health, metrics, workers, shutdown) plus any
//! consumer-supplied subsystems into a single boot-orchestrated
//! runtime.
//!
//! Typical consumer flow:
//!
//! 1. Build a kernel with [`KernelBuilder`], registering subsystems
//!    via [`KernelBuilder::with_subsystem`].
//! 2. Call [`Kernel::boot`] (Boot + Load) or [`Kernel::run`] (Boot +
//!    Load + block in Exec until [`Kernel::shutdown`] is called).
//! 3. Inspect the running kernel via [`Kernel::snapshot`] or
//!    [`Kernel::context`].
//!
//! Subsystems implement [`Subsystem`] and receive a [`KernelContext`]
//! in every lifecycle method. The context is a bundle of focused
//! handles ([`EventHandle`], [`ErrorHandle`], [`HealthHandle`],
//! [`MetricsHandle`], [`ShutdownHandle`], [`LifecycleHandle`]) plus
//! the kernel's stable `name`.

pub mod builder;
pub mod builtins;
pub mod context;
pub mod core;
pub mod handles;
pub mod subsystem;

pub use builder::{BuildError, KernelBuilder};
pub use context::KernelContext;
pub use core::{Kernel, KernelSnapshot};
pub use handles::{ErrorHandle, LifecycleHandle, ShutdownHandle};
pub use subsystem::{Subsystem, SubsystemSnapshot};

// Re-export the subsystem identifier from `primitives` so consumers can
// use a single canonical id type.
pub use crate::primitives::SubsystemId;

// Re-export the handles whose owning modules are elsewhere.
pub use crate::events::EventHandle;
pub use crate::health::HealthHandle;
pub use crate::metrics::MetricsHandle;
