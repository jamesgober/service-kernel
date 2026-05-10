//! Focused handles passed into [`KernelContext`](super::KernelContext).
//!
//! Each handle is a `Clone`-able view that exposes only the
//! operations its bearer is allowed to perform. Subsystems and
//! workers receive handles, never the underlying registries — so
//! e.g. only the kernel itself drives lifecycle transitions even
//! though every subsystem can read the current state.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant as StdInstant};

use crate::errors::{Classification, ErrorRegistry};
use crate::lifecycle::{KernelState, LifecycleController, LifecycleSnapshot, Phase};

/// Read-only view of the kernel's lifecycle.
///
/// Subsystems can observe the kernel's [`Phase`] and
/// [`KernelState`] but cannot drive transitions themselves — the
/// handle deliberately omits a `transition` method. Only the
/// [`Kernel`](super::Kernel) itself moves the controller.
#[derive(Clone)]
pub struct LifecycleHandle {
    inner: Arc<LifecycleController>,
}

impl LifecycleHandle {
    /// Constructs a handle wrapping the given controller.
    #[inline]
    #[must_use]
    pub(crate) fn new(inner: Arc<LifecycleController>) -> Self {
        Self { inner }
    }

    /// Returns the current coarse [`Phase`].
    #[inline]
    #[must_use]
    pub fn phase(&self) -> Phase {
        self.inner.phase()
    }

    /// Returns the current fine-grained [`KernelState`].
    #[inline]
    #[must_use]
    pub fn state(&self) -> KernelState {
        self.inner.state()
    }

    /// Returns a [`LifecycleSnapshot`] of the controller.
    #[inline]
    #[must_use]
    pub fn snapshot(&self) -> LifecycleSnapshot {
        self.inner.snapshot()
    }
}

impl fmt::Debug for LifecycleHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LifecycleHandle")
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

/// Classifier-only view of the kernel's [`ErrorRegistry`].
///
/// Subsystems classify errors through this handle but cannot replace
/// the registered classifier — that is reserved for the kernel
/// itself, set up at build time.
#[derive(Clone)]
pub struct ErrorHandle {
    inner: Arc<ErrorRegistry>,
}

impl ErrorHandle {
    /// Constructs a handle wrapping the given registry.
    #[inline]
    #[must_use]
    pub(crate) fn new(inner: Arc<ErrorRegistry>) -> Self {
        Self { inner }
    }

    /// Classifies an error using the registered classifier.
    #[inline]
    pub fn classify(&self, err: &(dyn std::error::Error + 'static)) -> Classification {
        self.inner.classify(err)
    }
}

impl fmt::Debug for ErrorHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ErrorHandle").finish_non_exhaustive()
    }
}

/// Inner state behind a [`ShutdownHandle`].
///
/// Holds the boolean signal plus a mutex/condvar pair so a thread
/// blocked in [`ShutdownInner::wait_for_signal`] is notified when
/// any other thread calls [`ShutdownInner::signal`]. Idempotent: a
/// second signal is a no-op.
pub(crate) struct ShutdownInner {
    signalled: AtomicBool,
    parker: Mutex<()>,
    cvar: Condvar,
}

impl ShutdownInner {
    pub(crate) fn new() -> Self {
        Self {
            signalled: AtomicBool::new(false),
            parker: Mutex::new(()),
            cvar: Condvar::new(),
        }
    }

    pub(crate) fn signal(&self) {
        // Briefly acquire the mutex so a waiter that has just checked
        // `signalled == false` and is about to enter `cvar.wait`
        // cannot miss the store. The guard drops at the close of this
        // block; we then notify outside the lock to keep the waiter's
        // re-acquire path short.
        {
            let _guard = self
                .parker
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            self.signalled.store(true, Ordering::Release);
        }
        self.cvar.notify_all();
    }

    pub(crate) fn is_signalled(&self) -> bool {
        self.signalled.load(Ordering::Acquire)
    }

    pub(crate) fn wait_for_signal(&self) {
        let mut guard = self
            .parker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while !self.signalled.load(Ordering::Acquire) {
            guard = self
                .cvar
                .wait(guard)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// Blocks until either the signal fires or `deadline` is reached.
    /// Returns `true` if the signal arrived in time.
    pub(crate) fn wait_for_signal_until(&self, deadline: StdInstant) -> bool {
        let mut guard = self
            .parker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while !self.signalled.load(Ordering::Acquire) {
            let now = StdInstant::now();
            if now >= deadline {
                return false;
            }
            let timeout = deadline.saturating_duration_since(now);
            let (g, result) = self
                .cvar
                .wait_timeout(guard, timeout)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard = g;
            if result.timed_out() && !self.signalled.load(Ordering::Acquire) {
                return false;
            }
        }
        true
    }
}

/// Cooperative shutdown trigger for the kernel.
///
/// Any subsystem or worker holding a [`ShutdownHandle`] can request
/// the kernel to begin its shutdown sequence by calling
/// [`signal`](ShutdownHandle::signal). The kernel's `run()` loop
/// observes the signal, transitions through `Stopping → Stopped`, and
/// returns. Multiple calls are safe — the handle is idempotent.
#[derive(Clone)]
pub struct ShutdownHandle {
    inner: Arc<ShutdownInner>,
}

impl ShutdownHandle {
    /// Constructs a handle wrapping the given inner state.
    #[inline]
    #[must_use]
    pub(crate) fn new(inner: Arc<ShutdownInner>) -> Self {
        Self { inner }
    }

    /// Signals the kernel to begin graceful shutdown. Idempotent.
    #[inline]
    pub fn signal(&self) {
        self.inner.signal();
    }

    /// Returns `true` once any thread has called
    /// [`signal`](Self::signal).
    #[inline]
    #[must_use]
    pub fn is_signalled(&self) -> bool {
        self.inner.is_signalled()
    }

    /// Blocks the calling thread until the signal fires.
    ///
    /// Used internally by [`Kernel::run`](super::Kernel::run).
    /// Exposed publicly so consumer code that drives the kernel from
    /// a custom main loop can wait on the same primitive.
    #[inline]
    pub fn wait(&self) {
        self.inner.wait_for_signal();
    }

    /// Blocks until the signal fires or the timeout elapses. Returns
    /// `true` if the signal arrived.
    #[inline]
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let deadline = StdInstant::now() + timeout;
        self.inner.wait_for_signal_until(deadline)
    }
}

impl fmt::Debug for ShutdownHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShutdownHandle")
            .field("signalled", &self.is_signalled())
            .finish()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::errors::{ErrorAction, NoopClassifier, Severity};
    use std::fmt;
    use std::sync::Arc;

    fn assert_send_sync<T: Send + Sync + Clone>() {}

    #[test]
    fn test_handles_are_send_sync_clone() {
        assert_send_sync::<LifecycleHandle>();
        assert_send_sync::<ErrorHandle>();
        assert_send_sync::<ShutdownHandle>();
    }

    #[test]
    fn test_lifecycle_handle_reflects_controller() {
        let controller = Arc::new(LifecycleController::new());
        let handle = LifecycleHandle::new(Arc::clone(&controller));
        assert_eq!(handle.state(), KernelState::Created);
        assert_eq!(handle.phase(), Phase::Idle);
    }

    #[test]
    fn test_error_handle_classifies() {
        #[derive(Debug)]
        struct DummyError;

        impl fmt::Display for DummyError {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("dummy")
            }
        }

        impl std::error::Error for DummyError {}

        let registry = Arc::new(ErrorRegistry::with_classifier(Arc::new(NoopClassifier)));
        let handle = ErrorHandle::new(registry);
        let result = handle.classify(&DummyError);
        assert_eq!(result.severity, Severity::Error);
        assert_eq!(result.action, ErrorAction::LogOnly);
    }

    #[test]
    fn test_shutdown_handle_signal_is_idempotent() {
        let handle = ShutdownHandle::new(Arc::new(ShutdownInner::new()));
        assert!(!handle.is_signalled());
        handle.signal();
        assert!(handle.is_signalled());
        handle.signal();
        assert!(handle.is_signalled());
    }

    #[test]
    fn test_shutdown_handle_wait_returns_after_signal() {
        let inner = Arc::new(ShutdownInner::new());
        let handle = ShutdownHandle::new(Arc::clone(&inner));
        let signaller = ShutdownHandle::new(inner);

        let join = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            signaller.signal();
        });

        handle.wait();
        join.join().unwrap();
        assert!(handle.is_signalled());
    }

    #[test]
    fn test_shutdown_handle_wait_timeout_returns_false_when_no_signal() {
        let handle = ShutdownHandle::new(Arc::new(ShutdownInner::new()));
        let arrived = handle.wait_timeout(std::time::Duration::from_millis(20));
        assert!(!arrived);
        assert!(!handle.is_signalled());
    }

    #[test]
    fn test_shutdown_handle_wait_timeout_returns_true_when_signal_arrives() {
        let inner = Arc::new(ShutdownInner::new());
        let handle = ShutdownHandle::new(Arc::clone(&inner));
        let signaller = ShutdownHandle::new(inner);

        let join = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(10));
            signaller.signal();
        });

        let arrived = handle.wait_timeout(std::time::Duration::from_secs(1));
        join.join().unwrap();
        assert!(arrived);
    }
}
