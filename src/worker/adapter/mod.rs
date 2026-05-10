//! Adapters that plug the kernel into existing worker primitives.
//!
//! Adapters are thin wrappers — they do not add behavior. They
//! translate between an external primitive's API and the kernel's
//! [`Worker`](super::Worker) / [`AsyncWorker`](super::AsyncWorker)
//! traits, or between the kernel's lifecycle and an external
//! runtime's lifecycle.

#[cfg(feature = "daemon")]
pub mod proc_daemon;

#[cfg(feature = "daemon")]
pub use proc_daemon::{DaemonAdapter, DaemonConfig};
