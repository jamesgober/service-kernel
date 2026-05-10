//! Kernel built-in subsystems.
//!
//! Seven subsystems are registered automatically by every
//! [`KernelBuilder`](super::KernelBuilder):
//!
//! 1. [`LifecycleSubsystem`] — anchors the lifecycle controller.
//! 2. [`EventSubsystem`] — anchors the event dispatcher.
//! 3. [`ErrorSubsystem`] — anchors the error registry.
//! 4. [`HealthSubsystem`] — anchors the health registry.
//! 5. [`MetricsSubsystem`] — anchors the metrics backend.
//! 6. [`WorkerSubsystem`] — placeholder; replaced in Milestone F.
//! 7. [`ShutdownSubsystem`] — placeholder; replaced in Milestone H.
//!
//! The registries themselves are constructed at
//! [`KernelBuilder::build`](super::KernelBuilder::build) time, so the
//! built-ins' lifecycle methods are mostly no-ops. The built-ins
//! exist primarily as named anchors in the dependency graph: a
//! consumer subsystem with `dependencies(): &["events"]` will sort
//! after [`EventSubsystem`].
//!
//! Built-in names are reserved — a consumer subsystem MUST NOT take
//! one of these names. The builder validates this at `build()`.

use crate::errors::KernelError;
use crate::health::HealthStatus;

use super::{KernelContext, Subsystem};

/// Stable name of [`LifecycleSubsystem`].
pub const LIFECYCLE: &str = "lifecycle";
/// Stable name of [`EventSubsystem`].
pub const EVENTS: &str = "events";
/// Stable name of [`ErrorSubsystem`].
pub const ERRORS: &str = "errors";
/// Stable name of [`HealthSubsystem`].
pub const HEALTH: &str = "health";
/// Stable name of [`MetricsSubsystem`].
pub const METRICS: &str = "metrics";
/// Stable name of [`WorkerSubsystem`].
pub const WORKERS: &str = "workers";
/// Stable name of [`ShutdownSubsystem`].
pub const SHUTDOWN: &str = "shutdown";

/// All seven built-in names in declaration order.
pub const BUILTIN_NAMES: &[&str] = &[
    LIFECYCLE, EVENTS, ERRORS, HEALTH, METRICS, WORKERS, SHUTDOWN,
];

/// Anchors the lifecycle controller in the dependency graph.
#[derive(Debug, Default, Clone, Copy)]
pub struct LifecycleSubsystem;

impl Subsystem for LifecycleSubsystem {
    fn name(&self) -> &'static str {
        LIFECYCLE
    }

    fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
        Ok(())
    }

    fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

/// Anchors the event dispatcher in the dependency graph.
#[derive(Debug, Default, Clone, Copy)]
pub struct EventSubsystem;

impl Subsystem for EventSubsystem {
    fn name(&self) -> &'static str {
        EVENTS
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[LIFECYCLE]
    }

    fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
        Ok(())
    }

    fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

/// Anchors the error registry in the dependency graph.
#[derive(Debug, Default, Clone, Copy)]
pub struct ErrorSubsystem;

impl Subsystem for ErrorSubsystem {
    fn name(&self) -> &'static str {
        ERRORS
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[EVENTS]
    }

    fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
        Ok(())
    }

    fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

/// Anchors the health registry in the dependency graph.
#[derive(Debug, Default, Clone, Copy)]
pub struct HealthSubsystem;

impl Subsystem for HealthSubsystem {
    fn name(&self) -> &'static str {
        HEALTH
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[EVENTS]
    }

    fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
        Ok(())
    }

    fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

/// Anchors the metrics backend in the dependency graph.
#[derive(Debug, Default, Clone, Copy)]
pub struct MetricsSubsystem;

impl Subsystem for MetricsSubsystem {
    fn name(&self) -> &'static str {
        METRICS
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[LIFECYCLE]
    }

    fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
        Ok(())
    }

    fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

/// Placeholder worker subsystem.
///
/// Real worker supervision lands in Milestone F; this struct exists
/// so consumer subsystems can depend on `"workers"` today and have
/// the dependency resolved at build time.
#[derive(Debug, Default, Clone, Copy)]
pub struct WorkerSubsystem;

impl Subsystem for WorkerSubsystem {
    fn name(&self) -> &'static str {
        WORKERS
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[METRICS, EVENTS, ERRORS]
    }

    fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
        Ok(())
    }

    fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

/// Placeholder shutdown subsystem.
///
/// Real shutdown drain coordination lands in Milestone H. The
/// placeholder exists so consumer subsystems can depend on
/// `"shutdown"` today.
#[derive(Debug, Default, Clone, Copy)]
pub struct ShutdownSubsystem;

impl Subsystem for ShutdownSubsystem {
    fn name(&self) -> &'static str {
        SHUTDOWN
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[LIFECYCLE, EVENTS]
    }

    fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
        Ok(())
    }

    fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

/// Returns boxed instances of the seven built-in subsystems.
#[must_use]
pub(crate) fn boxed_builtins() -> Vec<Box<dyn Subsystem>> {
    vec![
        Box::new(LifecycleSubsystem),
        Box::new(EventSubsystem),
        Box::new(ErrorSubsystem),
        Box::new(HealthSubsystem),
        Box::new(MetricsSubsystem),
        Box::new(WorkerSubsystem),
        Box::new(ShutdownSubsystem),
    ]
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_names_match_subsystem_names() {
        let built = boxed_builtins();
        let names: Vec<&str> = built.iter().map(|s| s.name()).collect();
        assert_eq!(names, BUILTIN_NAMES);
    }

    #[test]
    fn test_builtin_names_are_unique() {
        use std::collections::HashSet;
        let set: HashSet<&str> = BUILTIN_NAMES.iter().copied().collect();
        assert_eq!(set.len(), BUILTIN_NAMES.len());
    }

    #[test]
    fn test_builtin_dependencies_reference_only_other_builtins() {
        let names: std::collections::HashSet<&str> = BUILTIN_NAMES.iter().copied().collect();
        for s in boxed_builtins() {
            for dep in s.dependencies() {
                assert!(names.contains(dep), "{} declares unknown dep {}", s.name(), dep);
            }
        }
    }
}
