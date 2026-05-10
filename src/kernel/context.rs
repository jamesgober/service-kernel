//! [`KernelContext`] — the bundle of handles every subsystem
//! receives.
//!
//! The context is intentionally a flat struct of `Clone`-able
//! handles. Subsystems and workers reach into it by direct field
//! access (`ctx.events.emit(...)`, `ctx.health.report(...)`). There
//! is no god-object indirection.

use std::fmt;

use crate::events::EventHandle;
use crate::health::HealthHandle;
use crate::metrics::MetricsHandle;

use super::handles::{ErrorHandle, LifecycleHandle, ShutdownHandle};

/// Bundle of handles exposed to subsystems and workers.
///
/// Constructed by the kernel and handed to every subsystem method
/// (`boot`, `load`, `shutdown`). Each field is independently
/// `Clone`-able, so subsystems can stash whichever handles they need
/// long-term without keeping the whole context alive.
#[derive(Clone)]
pub struct KernelContext {
    /// Topic-keyed event bus.
    pub events: EventHandle,
    /// Error classification entry point.
    pub errors: ErrorHandle,
    /// Push-based health registry.
    pub health: HealthHandle,
    /// Metrics backend.
    pub metrics: MetricsHandle,
    /// Cooperative shutdown trigger.
    pub shutdown: ShutdownHandle,
    /// Read-only view of the kernel's lifecycle.
    pub lifecycle: LifecycleHandle,
    /// Stable name of the owning kernel.
    pub kernel_name: &'static str,
}

impl KernelContext {
    /// Returns the kernel's stable name.
    #[inline]
    #[must_use]
    pub fn kernel_name(&self) -> &'static str {
        self.kernel_name
    }
}

impl fmt::Debug for KernelContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KernelContext")
            .field("kernel_name", &self.kernel_name)
            .field("lifecycle_state", &self.lifecycle.state())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::errors::ErrorRegistry;
    use crate::events::EventDispatcher;
    use crate::health::HealthRegistry;
    use crate::lifecycle::LifecycleController;
    use crate::metrics::NoopMetricsBackend;

    use super::super::handles::ShutdownInner;

    use std::sync::Arc;

    fn assert_send_sync_clone<T: Send + Sync + Clone>() {}

    fn make_ctx() -> KernelContext {
        let dispatcher = EventDispatcher::new();
        let lifecycle = Arc::new(LifecycleController::new());
        let errors = Arc::new(ErrorRegistry::new());
        let health = HealthRegistry::new();
        let metrics: MetricsHandle = Arc::new(NoopMetricsBackend);
        let shutdown_inner = Arc::new(ShutdownInner::new());

        KernelContext {
            events: dispatcher.handle(),
            errors: ErrorHandle::new(errors),
            health: health.handle(),
            metrics,
            shutdown: ShutdownHandle::new(shutdown_inner),
            lifecycle: LifecycleHandle::new(lifecycle),
            kernel_name: "test-kernel",
        }
    }

    #[test]
    fn test_context_is_send_sync_clone() {
        assert_send_sync_clone::<KernelContext>();
    }

    #[test]
    fn test_kernel_name_round_trips() {
        let ctx = make_ctx();
        assert_eq!(ctx.kernel_name(), "test-kernel");
    }

    #[test]
    fn test_clone_is_independent_view_into_shared_state() {
        let ctx = make_ctx();
        let clone = ctx.clone();
        assert!(!ctx.shutdown.is_signalled());
        clone.shutdown.signal();
        assert!(ctx.shutdown.is_signalled());
    }
}
