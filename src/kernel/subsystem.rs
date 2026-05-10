//! [`Subsystem`] trait — the kernel's primary extension point.
//!
//! Consumer-defined subsystems implement this trait and register
//! through [`KernelBuilder::with_subsystem`](super::KernelBuilder::with_subsystem).
//! The kernel boots subsystems in dependency-respecting topological
//! order, hands them a [`KernelContext`], and shuts them down in
//! reverse order at termination.

use crate::errors::KernelError;
use crate::health::HealthStatus;
use crate::primitives::{Instant, SubsystemId};

use super::KernelContext;

/// Self-contained component participating in the kernel's run cycle.
///
/// All four lifecycle methods receive a [`KernelContext`] giving
/// access to the kernel's shared services. The trait is
/// `Send + Sync + 'static` and object-safe — the kernel stores
/// subsystems behind `Box<dyn Subsystem>`.
///
/// # Lifecycle method order
///
/// 1. `boot` — runs during [`Phase::Boot`](crate::lifecycle::Phase::Boot)
///    in dependency-respecting order. Set up any state the subsystem
///    needs before its peers can talk to it.
/// 2. `load` — runs during [`Phase::Load`](crate::lifecycle::Phase::Load)
///    in dependency-respecting order. Wire up any cross-subsystem
///    relationships, register handlers, etc.
/// 3. `shutdown` — runs in reverse boot order during
///    [`Phase::Shutdown`](crate::lifecycle::Phase::Shutdown).
///
/// Default impls cover the common no-op cases.
///
/// # Examples
///
/// ```
/// use service_kernel::errors::KernelError;
/// use service_kernel::kernel::{KernelContext, Subsystem};
///
/// struct StorageSubsystem;
///
/// impl Subsystem for StorageSubsystem {
///     fn name(&self) -> &'static str {
///         "storage"
///     }
///
///     fn dependencies(&self) -> &'static [&'static str] {
///         &["events"]
///     }
///
///     fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
///         // Initialize on-disk state, open handles, etc.
///         Ok(())
///     }
/// }
/// ```
pub trait Subsystem: Send + Sync + 'static {
    /// Stable name for this subsystem.
    ///
    /// Used in events, metrics labels, error messages, and the
    /// dependency graph. Names must be unique within a kernel.
    fn name(&self) -> &'static str;

    /// Names of subsystems this one depends on.
    ///
    /// The kernel boots dependencies first. Default: no dependencies.
    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    /// Boot-phase work.
    fn boot(&self, ctx: &KernelContext) -> Result<(), KernelError>;

    /// Load-phase work. Default: no-op.
    fn load(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
        Ok(())
    }

    /// Shutdown work. Default: no-op.
    fn shutdown(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
        Ok(())
    }

    /// Current health. Default: [`HealthStatus::Healthy`].
    fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

/// Read-only snapshot of a single subsystem's state.
#[derive(Debug, Clone)]
pub struct SubsystemSnapshot {
    /// Identifier assigned by the kernel at registration time.
    pub id: SubsystemId,
    /// Stable subsystem name.
    pub name: &'static str,
    /// Names of declared dependencies.
    pub dependencies: &'static [&'static str],
    /// Last reported health.
    pub health: HealthStatus,
    /// Wall-clock instant of successful `boot`, if any.
    pub booted_at: Option<Instant>,
    /// Wall-clock instant of successful `load`, if any.
    pub loaded_at: Option<Instant>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    struct Plain;

    impl Subsystem for Plain {
        fn name(&self) -> &'static str {
            "plain"
        }

        fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
            Ok(())
        }
    }

    struct WithDeps;

    impl Subsystem for WithDeps {
        fn name(&self) -> &'static str {
            "with_deps"
        }

        fn dependencies(&self) -> &'static [&'static str] {
            &["events", "metrics"]
        }

        fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
            Ok(())
        }
    }

    fn assert_send_sync<T: Send + Sync + ?Sized>() {}

    #[test]
    fn test_trait_is_object_safe() {
        let s: Box<dyn Subsystem> = Box::new(Plain);
        assert_eq!(s.name(), "plain");
        assert!(s.dependencies().is_empty());
        assert_eq!(s.health(), HealthStatus::Healthy);
    }

    #[test]
    fn test_subsystem_dyn_is_send_sync() {
        assert_send_sync::<dyn Subsystem>();
    }

    #[test]
    fn test_dependencies_default_is_empty() {
        assert!(Plain.dependencies().is_empty());
    }

    #[test]
    fn test_dependencies_can_declare_multiple() {
        assert_eq!(WithDeps.dependencies(), &["events", "metrics"]);
    }

    #[test]
    fn test_default_health_is_healthy() {
        assert_eq!(Plain.health(), HealthStatus::Healthy);
    }
}
