//! Health status, checks, and aggregation.
//!
//! The kernel tracks one [`HealthStatus`] per subsystem and folds the
//! per-subsystem statuses into a global aggregate. Aggregation is
//! **push-based**: subsystems call
//! [`HealthHandle::report`](registry::HealthHandle::report) when their
//! state changes and the registry recomputes the aggregate eagerly.
//! There is no polling.
//!
//! The aggregate rule is "worst wins" — `aggregate = max(per-subsystem
//! status)`. The [`HealthStatus`] ordering ranks `Unknown` as worse
//! than `Critical`, so a subsystem that has not yet reported is
//! treated as fail-safe (the kernel does not assume health that has
//! not been observed).
//!
//! ## Relationship to [`Severity`](crate::errors::Severity)
//!
//! Health and severity are different axes. `Severity` describes a
//! single error event; `HealthStatus` describes a subsystem's
//! ongoing operating condition. A `Severity::Critical` error may or
//! may not produce a `HealthStatus::Critical` subsystem state — the
//! consumer's classifier and the subsystem's reporting decide.

pub mod check;
pub mod registry;
pub mod snapshot;
pub mod status;

pub use check::HealthCheck;
pub use registry::{HealthHandle, HealthRegistry};
pub use snapshot::HealthSnapshot;
pub use status::HealthStatus;
