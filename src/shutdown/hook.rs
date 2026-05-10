//! Consumer-supplied shutdown hooks.
//!
//! [`ShutdownHook`] runs during the kernel's shutdown sequence,
//! after the cancellation token fires but before the supervisor's
//! worker drain. Hooks are useful for: flushing buffers, draining
//! HTTP listeners, persisting in-memory state, sending termination
//! events to a service registry.
//!
//! Hook failures are recorded in the [`ShutdownReport`](super::ShutdownReport)
//! but do not stop the sequence — the kernel always reaches `Stopped`.

use std::error::Error;
use std::fmt;
use std::time::Instant;

use lang_lib::Lang;

use crate::events::EventHandle;

/// Bundle of state passed to every shutdown hook.
///
/// `events` lets the hook emit events (e.g. progress notifications);
/// `deadline` is the absolute instant by which the hook should
/// finish — beyond it the coordinator treats the hook as timed out.
#[derive(Clone)]
pub struct ShutdownContext {
    /// Event handle for emitting custom shutdown events.
    pub events: EventHandle,
    /// Absolute deadline for this hook to complete.
    pub deadline: Instant,
}

impl ShutdownContext {
    /// Returns the time remaining before the deadline.
    #[must_use]
    pub fn remaining(&self) -> std::time::Duration {
        self.deadline
            .checked_duration_since(Instant::now())
            .unwrap_or_default()
    }
}

impl fmt::Debug for ShutdownContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShutdownContext")
            .field("remaining", &self.remaining())
            .finish_non_exhaustive()
    }
}

/// Async hook called during shutdown.
///
/// Implementations are stored behind `Box<dyn ShutdownHook>` by the
/// [`ShutdownCoordinator`](super::ShutdownCoordinator) and run in
/// registration order. Returning `Err` does not stop the sequence;
/// the failure is captured in the [`ShutdownReport`](super::ShutdownReport)
/// for the operator to inspect.
///
/// # Examples
///
/// ```
/// use service_kernel::shutdown::{HookError, ShutdownContext, ShutdownHook};
///
/// struct FlushBuffers;
///
/// #[async_trait::async_trait]
/// impl ShutdownHook for FlushBuffers {
///     fn name(&self) -> &'static str { "flush-buffers" }
///     async fn run(&self, _ctx: &ShutdownContext) -> Result<(), HookError> {
///         // Flush in-memory buffers to disk here.
///         Ok(())
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait ShutdownHook: Send + Sync + 'static {
    /// Stable hook name. Used in events and the shutdown report.
    fn name(&self) -> &'static str;

    /// Hook body.
    async fn run(&self, ctx: &ShutdownContext) -> Result<(), HookError>;
}

/// Error returned by a [`ShutdownHook`].
#[derive(Debug)]
pub struct HookError {
    /// Stable name of the hook that failed.
    pub hook_name: &'static str,
    /// Underlying cause.
    pub source: Box<dyn Error + Send + Sync + 'static>,
}

impl HookError {
    /// Constructs a `HookError` with the given name and source.
    #[inline]
    #[must_use]
    pub fn new(hook_name: &'static str, source: impl Error + Send + Sync + 'static) -> Self {
        Self {
            hook_name,
            source: Box::new(source),
        }
    }

    /// Constructs a `HookError` from a string message.
    #[inline]
    #[must_use]
    pub fn from_message(hook_name: &'static str, message: impl Into<String>) -> Self {
        Self {
            hook_name,
            source: message.into().into(),
        }
    }
}

impl fmt::Display for HookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = Lang::translate(
            "kernel.shutdown.hook.failed",
            None,
            Some("shutdown hook failed"),
        );
        write!(f, "{}: {}: {}", prefix, self.hook_name, self.source)
    }
}

impl Error for HookError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&*self.source)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::events::EventDispatcher;
    use std::time::Duration;

    fn assert_send_sync<T: Send + Sync + ?Sized>() {}

    struct DummyHook;

    #[async_trait::async_trait]
    impl ShutdownHook for DummyHook {
        fn name(&self) -> &'static str {
            "dummy"
        }
        async fn run(&self, _ctx: &ShutdownContext) -> Result<(), HookError> {
            Ok(())
        }
    }

    #[test]
    fn test_trait_is_object_safe() {
        let _: Box<dyn ShutdownHook> = Box::new(DummyHook);
    }

    #[test]
    fn test_hook_error_send_sync() {
        assert_send_sync::<HookError>();
    }

    #[test]
    fn test_context_remaining_decreases() {
        let dispatcher = EventDispatcher::new();
        let ctx = ShutdownContext {
            events: dispatcher.handle(),
            deadline: Instant::now() + Duration::from_millis(50),
        };
        let r1 = ctx.remaining();
        std::thread::sleep(Duration::from_millis(5));
        let r2 = ctx.remaining();
        assert!(r2 < r1);
    }

    #[test]
    fn test_context_remaining_saturates_at_zero_after_deadline() {
        let dispatcher = EventDispatcher::new();
        let ctx = ShutdownContext {
            events: dispatcher.handle(),
            deadline: Instant::now() - Duration::from_secs(1),
        };
        assert_eq!(ctx.remaining(), Duration::ZERO);
    }

    #[test]
    fn test_hook_error_display_includes_name_and_source() {
        let err = HookError::from_message("flush", "disk full");
        let s = err.to_string();
        assert!(s.contains("flush"));
        assert!(s.contains("disk full"));
    }
}
