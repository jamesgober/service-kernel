//! [`ShutdownCoordinator`] — orchestrator for the shutdown sequence.
//!
//! The coordinator is intentionally narrow:
//!
//! - Owns the [`ShutdownToken`] cancelled to signal "shut down now".
//! - Owns the registered [`ShutdownHook`] list.
//! - Provides [`shutdown`](ShutdownCoordinator::shutdown), which
//!   signals the token, runs hooks, and returns a partial
//!   [`ShutdownReport`].
//!
//! The kernel uses the coordinator as one piece of its full shutdown
//! sequence. The coordinator itself does not touch the supervisor
//! or the subsystem list — those higher-level concerns are
//! orchestrated by the kernel's `run_supervisor` path, which fills
//! in the worker / subsystem fields of the returned report.

use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, Instant as StdInstant};

use crate::events::{EventHandle, KernelEvent, LifecycleEvent};
use crate::primitives::Instant;

use super::hook::{ShutdownContext, ShutdownHook};
use super::report::ShutdownReport;
use super::token::ShutdownToken;

/// Default grace cap when constructing without an explicit value.
const DEFAULT_GRACE: Duration = Duration::from_secs(10);

/// Single-shot orchestrator of the shutdown sequence.
///
/// `register_hook` is callable any time before
/// [`shutdown`](Self::shutdown) is invoked. After `shutdown` returns,
/// further calls to `shutdown` are no-ops returning a "nothing to do"
/// report.
pub struct ShutdownCoordinator {
    token: ShutdownToken,
    grace: Duration,
    hooks: Mutex<Vec<Box<dyn ShutdownHook>>>,
    events: EventHandle,
    completed: Mutex<bool>,
}

impl ShutdownCoordinator {
    /// Constructs a coordinator with the given event handle and
    /// grace period.
    #[must_use]
    pub fn new(events: EventHandle, grace: Duration) -> Self {
        Self {
            token: ShutdownToken::new(),
            grace,
            hooks: Mutex::new(Vec::new()),
            events,
            completed: Mutex::new(false),
        }
    }

    /// Constructs a coordinator with the default grace period.
    #[inline]
    #[must_use]
    pub fn with_default_grace(events: EventHandle) -> Self {
        Self::new(events, DEFAULT_GRACE)
    }

    /// Returns a clone of the coordinator's [`ShutdownToken`].
    #[inline]
    #[must_use]
    pub fn token(&self) -> ShutdownToken {
        self.token.clone()
    }

    /// Returns the configured grace period.
    #[inline]
    #[must_use]
    pub fn grace(&self) -> Duration {
        self.grace
    }

    /// Registers a [`ShutdownHook`].
    ///
    /// Hooks run in registration order during
    /// [`shutdown`](Self::shutdown). Late registration after a
    /// shutdown has completed is allowed but the hook will not run
    /// (subsequent shutdowns are no-ops).
    pub fn register_hook<H: ShutdownHook>(&self, hook: H) {
        let mut guard = self
            .hooks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.push(Box::new(hook));
    }

    /// Returns the number of registered hooks.
    #[inline]
    #[must_use]
    pub fn hook_count(&self) -> usize {
        self.hooks.lock().map(|g| g.len()).unwrap_or(0)
    }

    /// Runs the shutdown sequence: signal the token, run hooks,
    /// emit lifecycle events, and return a partial [`ShutdownReport`].
    ///
    /// The report's `workers_*` and `subsystems_*` fields are set
    /// to zero / empty; the kernel fills them in after running its
    /// supervisor drain and subsystem shutdown.
    pub async fn shutdown(&self) -> ShutdownReport {
        let started_at = Instant::now();

        let already_done = {
            let mut guard = self
                .completed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let was_done = *guard;
            *guard = true;
            was_done
        };

        if already_done {
            return ShutdownReport {
                started_at,
                completed_at: Instant::now(),
                workers_drained: 0,
                workers_aborted: 0,
                hooks_succeeded: Vec::new(),
                hooks_failed: Vec::new(),
                subsystems_shutdown: Vec::new(),
                subsystems_failed: Vec::new(),
            };
        }

        let std_started = StdInstant::now();
        let deadline = std_started + self.grace;

        self.events
            .emit(KernelEvent::Lifecycle(LifecycleEvent::ShutdownStarted {
                at: started_at,
            }));

        self.token.signal();

        // Take the registered hooks; subsequent registrations after
        // shutdown begins do not run.
        let hooks: Vec<Box<dyn ShutdownHook>> = {
            let mut guard = self
                .hooks
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut *guard)
        };

        let mut succeeded: Vec<&'static str> = Vec::with_capacity(hooks.len());
        let mut failed: Vec<(&'static str, String)> = Vec::new();

        for hook in hooks {
            let name = hook.name();
            let now = StdInstant::now();
            let remaining = deadline.checked_duration_since(now).unwrap_or_default();
            if remaining.is_zero() {
                failed.push((name, "deadline expired before hook ran".to_owned()));
                continue;
            }
            let ctx = ShutdownContext {
                events: self.events.clone(),
                deadline,
            };

            let outcome = tokio::time::timeout(remaining, hook.run(&ctx)).await;
            match outcome {
                Ok(Ok(())) => succeeded.push(name),
                Ok(Err(err)) => failed.push((name, err.to_string())),
                Err(_) => failed.push((name, format!("hook timed out after {:?}", remaining))),
            }
        }

        let completed_at = Instant::now();
        let report = ShutdownReport {
            started_at,
            completed_at,
            workers_drained: 0,
            workers_aborted: 0,
            hooks_succeeded: succeeded,
            hooks_failed: failed,
            subsystems_shutdown: Vec::new(),
            subsystems_failed: Vec::new(),
        };

        self.events
            .emit(KernelEvent::Lifecycle(LifecycleEvent::ShutdownCompleted {
                duration: report.duration(),
                workers_drained: report.workers_drained,
                workers_aborted: report.workers_aborted,
                at: completed_at,
            }));

        report
    }
}

impl fmt::Debug for ShutdownCoordinator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShutdownCoordinator")
            .field("grace", &self.grace)
            .field("hooks", &self.hook_count())
            .field("signalled", &self.token.is_signalled())
            .finish()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::events::EventDispatcher;
    use crate::shutdown::HookError;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn make_coordinator(grace: Duration) -> (ShutdownCoordinator, Arc<EventDispatcher>) {
        let dispatcher = Arc::new(EventDispatcher::new());
        let coord = ShutdownCoordinator::new(dispatcher.handle(), grace);
        (coord, dispatcher)
    }

    struct CountedHook {
        name: &'static str,
        ran: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl ShutdownHook for CountedHook {
        fn name(&self) -> &'static str {
            self.name
        }
        async fn run(&self, _ctx: &ShutdownContext) -> Result<(), HookError> {
            let _ = self.ran.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    struct FailingHook {
        name: &'static str,
    }

    #[async_trait::async_trait]
    impl ShutdownHook for FailingHook {
        fn name(&self) -> &'static str {
            self.name
        }
        async fn run(&self, _ctx: &ShutdownContext) -> Result<(), HookError> {
            Err(HookError::from_message(self.name, "boom"))
        }
    }

    struct SlowHook;

    #[async_trait::async_trait]
    impl ShutdownHook for SlowHook {
        fn name(&self) -> &'static str {
            "slow"
        }
        async fn run(&self, _ctx: &ShutdownContext) -> Result<(), HookError> {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_no_hooks_returns_clean_report() {
        let (coord, _dispatcher) = make_coordinator(Duration::from_secs(1));
        let report = coord.shutdown().await;
        assert!(report.is_clean());
        assert!(coord.token().is_signalled());
    }

    #[tokio::test]
    async fn test_successful_hooks_listed_in_order() {
        let (coord, _dispatcher) = make_coordinator(Duration::from_secs(1));
        let ran = Arc::new(AtomicUsize::new(0));
        coord.register_hook(CountedHook {
            name: "first",
            ran: Arc::clone(&ran),
        });
        coord.register_hook(CountedHook {
            name: "second",
            ran: Arc::clone(&ran),
        });
        let report = coord.shutdown().await;
        assert_eq!(report.hooks_succeeded, vec!["first", "second"]);
        assert_eq!(ran.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn test_failing_hook_does_not_stop_sequence() {
        let (coord, _dispatcher) = make_coordinator(Duration::from_secs(1));
        let ran = Arc::new(AtomicUsize::new(0));
        coord.register_hook(FailingHook { name: "fail" });
        coord.register_hook(CountedHook {
            name: "after-fail",
            ran: Arc::clone(&ran),
        });
        let report = coord.shutdown().await;
        assert_eq!(report.hooks_failed.len(), 1);
        assert_eq!(report.hooks_failed[0].0, "fail");
        assert_eq!(ran.load(Ordering::Relaxed), 1);
        assert!(!report.is_clean());
    }

    #[tokio::test]
    async fn test_slow_hook_is_timed_out() {
        let (coord, _dispatcher) = make_coordinator(Duration::from_millis(20));
        coord.register_hook(SlowHook);
        let report = coord.shutdown().await;
        assert_eq!(report.hooks_failed.len(), 1);
        assert_eq!(report.hooks_failed[0].0, "slow");
        assert!(report.hooks_failed[0].1.contains("timed out"));
    }

    #[tokio::test]
    async fn test_second_shutdown_is_noop() {
        let (coord, _dispatcher) = make_coordinator(Duration::from_secs(1));
        let _ = coord.shutdown().await;
        let report = coord.shutdown().await;
        assert_eq!(report.hooks_succeeded.len(), 0);
        assert_eq!(report.hooks_failed.len(), 0);
    }

    #[tokio::test]
    async fn test_emits_started_and_completed_events() {
        let (coord, dispatcher) = make_coordinator(Duration::from_millis(50));
        let started = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));
        let s_cb = Arc::clone(&started);
        let c_cb = Arc::clone(&completed);
        let _ = dispatcher.subscribe("kernel.lifecycle.shutdown_started", move |_| {
            let _ = s_cb.fetch_add(1, Ordering::Relaxed);
        });
        let _ = dispatcher.subscribe("kernel.lifecycle.shutdown_completed", move |_| {
            let _ = c_cb.fetch_add(1, Ordering::Relaxed);
        });
        let _ = coord.shutdown().await;
        assert_eq!(started.load(Ordering::Relaxed), 1);
        assert_eq!(completed.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_signal_token_fires_on_shutdown() {
        let (coord, _dispatcher) = make_coordinator(Duration::from_secs(1));
        let token = coord.token();
        let _ = coord.shutdown().await;
        assert!(token.is_signalled());
    }
}
