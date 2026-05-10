//! Restart, backoff, and circuit-breaker policies applied to workers.
//!
//! - [`RestartPolicy`] — whether to re-spawn a failed worker.
//! - [`BackoffPolicy`] — how long to wait before re-spawning.
//! - [`CircuitBreaker`] / [`CircuitPolicy`] / [`CircuitState`] —
//!   repeated-failure containment.

pub mod backoff;
pub mod circuit;
pub mod restart;

pub use backoff::BackoffPolicy;
pub use circuit::{CircuitBreaker, CircuitPolicy, CircuitState};
pub use restart::RestartPolicy;
