//! On-demand health-check trait.
//!
//! [`HealthCheck`] is for components that compute their status on the
//! fly rather than push it. The push-based registry is the kernel's
//! default flow — implementing `HealthCheck` is optional and exists
//! for cases where polling is the natural model (e.g. a periodic
//! job that wants to be queried by an admin endpoint).

use super::HealthStatus;

/// Synchronous, on-demand health check.
///
/// Implementations return the current [`HealthStatus`] when called.
/// The trait is `Send + Sync + 'static` and object-safe; consumers
/// store implementations behind `Box<dyn HealthCheck>` or
/// `Arc<dyn HealthCheck>`.
///
/// # Examples
///
/// ```
/// use service_kernel::health::{HealthCheck, HealthStatus};
///
/// struct DiskCheck;
///
/// impl HealthCheck for DiskCheck {
///     fn name(&self) -> &'static str {
///         "disk"
///     }
///
///     fn check(&self) -> HealthStatus {
///         // Real implementation would inspect free space, IO latency, etc.
///         HealthStatus::Healthy
///     }
/// }
///
/// let probe: Box<dyn HealthCheck> = Box::new(DiskCheck);
/// assert_eq!(probe.name(), "disk");
/// assert_eq!(probe.check(), HealthStatus::Healthy);
/// ```
pub trait HealthCheck: Send + Sync + 'static {
    /// Stable identifier for this check (used in events + metrics).
    fn name(&self) -> &'static str;

    /// Computes the current status.
    fn check(&self) -> HealthStatus;
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct ConstantCheck {
        name: &'static str,
        status: HealthStatus,
    }

    impl HealthCheck for ConstantCheck {
        fn name(&self) -> &'static str {
            self.name
        }

        fn check(&self) -> HealthStatus {
            self.status
        }
    }

    #[test]
    fn test_trait_is_object_safe_via_box() {
        let probe: Box<dyn HealthCheck> = Box::new(ConstantCheck {
            name: "test",
            status: HealthStatus::Degraded,
        });
        assert_eq!(probe.name(), "test");
        assert_eq!(probe.check(), HealthStatus::Degraded);
    }

    #[test]
    fn test_trait_is_object_safe_via_arc() {
        let probe: Arc<dyn HealthCheck> = Arc::new(ConstantCheck {
            name: "shared",
            status: HealthStatus::Critical,
        });
        let p2 = Arc::clone(&probe);
        assert_eq!(probe.check(), HealthStatus::Critical);
        assert_eq!(p2.name(), "shared");
    }
}
