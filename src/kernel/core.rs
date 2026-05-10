//! [`Kernel`] — the assembled, boot-ready runtime.
//!
//! `Kernel` is constructed by
//! [`KernelBuilder::build`](super::KernelBuilder::build). It owns
//! all the kernel registries (lifecycle, events, errors, health,
//! metrics, shutdown) plus the topologically-sorted list of
//! subsystems. The methods on `Kernel` drive the boot, run, and
//! shutdown sequences.
//!
//! Subsystem panics during `boot`, `load`, and `shutdown` are caught
//! with [`std::panic::catch_unwind`] and turned into
//! [`KernelError::Subsystem`] with
//! [`KernelErrorCode::SubsystemBootFailed`] (boot/load) or the
//! `Shutdown` variant for the shutdown path.

use std::collections::HashMap;
use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::errors::ErrorRegistry;
use crate::errors::{KernelError, KernelErrorCode};
use crate::events::EventDispatcher;
use crate::health::{HealthRegistry, HealthSnapshot};
use crate::lifecycle::{KernelState, LifecycleController, LifecycleSnapshot};
use crate::metrics::MetricsHandle;
use crate::primitives::{IdGenerator, Instant, SubsystemId};

use super::context::KernelContext;
use super::handles::{ErrorHandle, LifecycleHandle, ShutdownHandle, ShutdownInner};
use super::subsystem::{Subsystem, SubsystemSnapshot};

/// Shared inner state of a [`Kernel`].
///
/// `Kernel` is a thin `Arc<KernelInner>` wrapper so that
/// [`Kernel::context`] and [`Kernel::shutdown`] can be called from
/// any thread holding a clone, while `Kernel::boot` and `Kernel::run`
/// take `&self` and mutate the subsystem list under an internal
/// `Mutex`.
pub(crate) struct KernelInner {
    pub(crate) name: &'static str,
    pub(crate) lifecycle: Arc<LifecycleController>,
    pub(crate) events: Arc<EventDispatcher>,
    pub(crate) errors: Arc<ErrorRegistry>,
    pub(crate) health: Arc<HealthRegistry>,
    pub(crate) metrics: MetricsHandle,
    pub(crate) shutdown: Arc<ShutdownInner>,
    pub(crate) subsystems: Mutex<Vec<Box<dyn Subsystem>>>,
    pub(crate) shutdown_grace: Duration,
    #[cfg(feature = "tokio")]
    pub(crate) workers: Mutex<Vec<super::builder::PendingWorker>>,
    #[cfg(feature = "tokio")]
    pub(crate) shutdown_coordinator: Arc<crate::shutdown::ShutdownCoordinator>,
    #[cfg(feature = "tokio")]
    pub(crate) signal_handler_requested: std::sync::atomic::AtomicBool,
}

/// Per-subsystem accounting (id + boot/load timestamps).
#[derive(Default)]
struct SubsystemBookkeeping {
    id: HashMap<&'static str, SubsystemId>,
    booted_at: HashMap<&'static str, Instant>,
    loaded_at: HashMap<&'static str, Instant>,
}

/// Read-only snapshot of a [`Kernel`].
#[derive(Debug, Clone)]
pub struct KernelSnapshot {
    /// Stable kernel name.
    pub name: &'static str,
    /// Lifecycle state at snapshot time.
    pub lifecycle: LifecycleSnapshot,
    /// Health snapshot at snapshot time.
    pub health: HealthSnapshot,
    /// Per-subsystem snapshots in boot order.
    pub subsystems: Vec<SubsystemSnapshot>,
    /// Wall-clock instant of this snapshot.
    pub timestamp: Instant,
}

/// Assembled kernel.
///
/// Construct via [`KernelBuilder::build`](super::KernelBuilder::build).
/// The kernel is `Send + Sync` and `Clone`-able (cheap `Arc` clone)
/// so the consumer can hand a clone to a separate shutdown thread
/// while the main thread is blocked in [`Kernel::run`].
#[derive(Clone)]
pub struct Kernel {
    inner: Arc<KernelInner>,
    bookkeeping: Arc<Mutex<SubsystemBookkeeping>>,
}

impl Kernel {
    pub(crate) fn from_inner(inner: Arc<KernelInner>) -> Self {
        let mut book = SubsystemBookkeeping::default();
        let id_gen = IdGenerator::new();
        if let Ok(guard) = inner.subsystems.lock() {
            for s in guard.iter() {
                let _ = book.id.insert(s.name(), id_gen.next_subsystem_id());
            }
        }
        Self {
            inner,
            bookkeeping: Arc::new(Mutex::new(book)),
        }
    }

    /// Returns the kernel's stable name.
    #[inline]
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.inner.name
    }

    /// Returns the configured shutdown grace period.
    #[inline]
    #[must_use]
    pub fn shutdown_grace(&self) -> Duration {
        self.inner.shutdown_grace
    }

    /// Returns a [`KernelContext`] suitable for handing to subsystems
    /// and workers.
    ///
    /// All handles inside the context share state with the kernel —
    /// e.g. `context().shutdown.signal()` is equivalent to
    /// [`Kernel::shutdown`].
    #[must_use]
    pub fn context(&self) -> KernelContext {
        KernelContext {
            events: self.inner.events.handle(),
            errors: ErrorHandle::new(Arc::clone(&self.inner.errors)),
            health: self.inner.health.handle(),
            metrics: Arc::clone(&self.inner.metrics),
            shutdown: ShutdownHandle::new(Arc::clone(&self.inner.shutdown)),
            lifecycle: LifecycleHandle::new(Arc::clone(&self.inner.lifecycle)),
            kernel_name: self.inner.name,
        }
    }

    /// Signals the kernel to begin graceful shutdown.
    ///
    /// Idempotent. Wakes any thread blocked in [`Kernel::run`] or
    /// [`ShutdownHandle::wait`](super::ShutdownHandle::wait).
    #[inline]
    pub fn shutdown(&self) {
        self.inner.shutdown.signal();
    }

    /// Registers a [`ShutdownHook`](crate::shutdown::ShutdownHook).
    ///
    /// Hooks run in registration order during the shutdown sequence,
    /// each bounded by the kernel's shutdown grace period. Failures
    /// are recorded in the [`ShutdownReport`](crate::shutdown::ShutdownReport)
    /// but do not stop the sequence.
    ///
    /// Available when the `tokio` feature is enabled.
    #[cfg(feature = "tokio")]
    pub fn register_shutdown_hook<H: crate::shutdown::ShutdownHook>(&self, hook: H) {
        self.inner.shutdown_coordinator.register_hook(hook);
    }

    /// Returns a clone of the kernel's
    /// [`ShutdownToken`](crate::shutdown::ShutdownToken).
    ///
    /// Subsystems and consumer code that prefer the typed token over
    /// [`ShutdownHandle::signal`](super::ShutdownHandle::signal) can
    /// hold onto it. Cancelling either path triggers the same
    /// shutdown sequence.
    #[cfg(feature = "tokio")]
    #[must_use]
    pub fn shutdown_token(&self) -> crate::shutdown::ShutdownToken {
        self.inner.shutdown_coordinator.token()
    }

    /// Asks the kernel to install an OS signal handler when it
    /// enters [`Phase::Exec`](crate::lifecycle::Phase::Exec).
    ///
    /// The handler listens for `SIGTERM` (Unix) and `Ctrl+C`
    /// (cross-platform) and signals shutdown when either fires.
    /// Idempotent — calling twice is a no-op.
    ///
    /// Available when the `tokio` feature is enabled. The handler
    /// task is spawned inside the runtime [`Kernel::run`] builds.
    #[cfg(feature = "tokio")]
    pub fn install_signal_handler(&self) {
        self.inner
            .signal_handler_requested
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// Returns a snapshot of the kernel's state.
    #[must_use]
    pub fn snapshot(&self) -> KernelSnapshot {
        let lifecycle = self.inner.lifecycle.snapshot();
        let health = self.inner.health.snapshot();
        let subsystems = self.subsystem_snapshots();
        KernelSnapshot {
            name: self.inner.name,
            lifecycle,
            health,
            subsystems,
            timestamp: Instant::now(),
        }
    }

    /// Boots the kernel: runs `boot` on every subsystem, then `load`
    /// on every subsystem, both in topological order.
    ///
    /// On success, the kernel is in [`KernelState::Running`].
    ///
    /// # Errors
    ///
    /// Returns the first [`KernelError`] from any subsystem's `boot`
    /// or `load`. On error during `load`, every subsystem already
    /// loaded is shut down in reverse order before the kernel
    /// transitions to [`KernelState::Failed`].
    pub fn boot(&self) -> Result<(), KernelError> {
        let lifecycle = Arc::clone(&self.inner.lifecycle);
        let ctx = self.context();

        lifecycle
            .transition(KernelState::Booting)
            .map_err(|e| KernelError::Lifecycle {
                code: KernelErrorCode::LifecycleIllegalTransition,
                source: e,
            })?;

        if let Err(err) = self.run_boot_phase(&ctx) {
            let _ = lifecycle.transition(KernelState::Failed);
            return Err(err);
        }

        if let Err(err) = lifecycle.transition(KernelState::Loading) {
            return Err(KernelError::Lifecycle {
                code: KernelErrorCode::LifecycleIllegalTransition,
                source: err,
            });
        }

        if let Err(err) = self.run_load_phase(&ctx) {
            self.run_shutdown_phase_to_loaded(&ctx);
            let _ = lifecycle.transition(KernelState::Failed);
            return Err(err);
        }

        lifecycle
            .transition(KernelState::Running)
            .map_err(|e| KernelError::Lifecycle {
                code: KernelErrorCode::LifecycleIllegalTransition,
                source: e,
            })?;

        Ok(())
    }

    /// Boots the kernel and blocks the calling thread until the
    /// kernel's [`ShutdownHandle`] is signalled.
    ///
    /// On wakeup, runs every subsystem's `shutdown` in reverse boot
    /// order, transitions the kernel through
    /// `Stopping → Stopped`, and returns.
    ///
    /// When the `tokio` feature is enabled and workers have been
    /// registered via the `KernelBuilder::with_worker` or
    /// `KernelBuilder::with_async_worker` builder methods,
    /// `run` constructs a Tokio runtime internally and drives the
    /// supervisor through Phase::Exec. Without workers, `run` blocks
    /// on the same `Condvar` it has used since Milestone E.
    ///
    /// # Errors
    ///
    /// Returns the boot error if `boot` fails. After Exec, returns
    /// the first shutdown error if any subsystem's `shutdown`
    /// returns `Err` (all shutdowns still run).
    pub fn run(&self) -> Result<(), KernelError> {
        self.boot()?;

        self.run_exec_phase()?;

        let lifecycle = Arc::clone(&self.inner.lifecycle);
        let ctx = self.context();

        if let Err(err) = lifecycle.transition(KernelState::Stopping) {
            return Err(KernelError::Lifecycle {
                code: KernelErrorCode::LifecycleIllegalTransition,
                source: err,
            });
        }

        let shutdown_result = self.run_shutdown_phase(&ctx);

        if let Err(err) = lifecycle.transition(KernelState::Stopped) {
            return Err(KernelError::Lifecycle {
                code: KernelErrorCode::LifecycleIllegalTransition,
                source: err,
            });
        }

        shutdown_result
    }

    /// Runs the Exec-phase wait. When `tokio` is enabled and
    /// workers are registered, this drives the supervisor on a
    /// freshly-built Tokio runtime; otherwise it blocks on the
    /// kernel's shutdown `Condvar`.
    #[cfg(feature = "tokio")]
    fn run_exec_phase(&self) -> Result<(), KernelError> {
        let has_workers = {
            let workers = self
                .inner
                .workers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            !workers.is_empty()
        };
        let signal_requested = self
            .inner
            .signal_handler_requested
            .load(std::sync::atomic::Ordering::Acquire);

        if !has_workers && !signal_requested {
            // No async work needed — coordinator runs hooks
            // synchronously after the wait.
            self.inner.shutdown.wait_for_signal();
            self.run_coordinator_blocking();
            return Ok(());
        }

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| KernelError::Internal {
                code: KernelErrorCode::Internal,
                message: format!("failed to build tokio runtime: {}", e),
            })?;

        runtime.block_on(self.run_supervisor());
        Ok(())
    }

    /// Runs the coordinator on a tiny single-threaded runtime so
    /// the shutdown sequence can emit `ShutdownStarted` /
    /// `ShutdownCompleted` events and run hooks even when the kernel
    /// otherwise has no async work to do.
    #[cfg(feature = "tokio")]
    fn run_coordinator_blocking(&self) {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(_) => return,
        };
        runtime.block_on(async {
            let _report = self.inner.shutdown_coordinator.shutdown().await;
        });
    }

    #[cfg(not(feature = "tokio"))]
    fn run_exec_phase(&self) -> Result<(), KernelError> {
        self.inner.shutdown.wait_for_signal();
        Ok(())
    }

    /// Spawns the supervisor and waits for either the supervisor to
    /// exit or the kernel's shutdown signal to fire. Drives the
    /// shutdown coordinator on the way out.
    #[cfg(feature = "tokio")]
    async fn run_supervisor(&self) {
        use crate::worker::Supervisor;

        let mut supervisor = Supervisor::new(
            self.inner.events.handle(),
            self.inner.health.handle(),
            Arc::clone(&self.inner.metrics),
            super::handles::ShutdownHandle::new(Arc::clone(&self.inner.shutdown)),
        );

        // Move pending workers into the supervisor.
        let pending: Vec<super::builder::PendingWorker> = {
            let mut guard = self
                .inner
                .workers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut *guard)
        };
        for w in pending {
            match w {
                super::builder::PendingWorker::Sync(spec, worker) => {
                    let _ = supervisor.register(spec, worker);
                }
                super::builder::PendingWorker::Async(spec, worker) => {
                    let _ = supervisor.register_async(spec, worker);
                }
            }
        }

        let cancel = supervisor.cancel_token();
        let shutdown = Arc::clone(&self.inner.shutdown);

        // Bridge the kernel's sync shutdown signal into the async
        // path: a dedicated blocking task waits on the `Condvar` and
        // cancels the supervisor when it fires.
        let watcher_cancel = cancel.clone();
        let watcher_shutdown = Arc::clone(&shutdown);
        let watcher = tokio::task::spawn_blocking(move || {
            watcher_shutdown.wait_for_signal();
            watcher_cancel.cancel();
        });

        // Optional OS signal handler.
        if self
            .inner
            .signal_handler_requested
            .load(std::sync::atomic::Ordering::Acquire)
        {
            self.spawn_signal_handler();
        }

        let _ = supervisor.run().await;

        // Run the shutdown coordinator: emit ShutdownStarted, run
        // hooks, emit ShutdownCompleted. Worker counts are not
        // available from the coordinator's perspective; the kernel
        // can extend the report with supervisor stats in a future
        // milestone if needed.
        let _report = self.inner.shutdown_coordinator.shutdown().await;

        // If the supervisor exited before shutdown was signalled
        // (e.g. all workers stopped permanently), wake the watcher so
        // the kernel can transition to Stopping.
        shutdown.signal();
        let _ = watcher.await;
    }

    /// Spawns a Tokio task that listens for SIGTERM (Unix) and
    /// Ctrl+C (cross-platform) and signals shutdown when either
    /// fires.
    #[cfg(feature = "tokio")]
    fn spawn_signal_handler(&self) {
        let shutdown = Arc::clone(&self.inner.shutdown);
        drop(tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                let mut sigterm = match signal(SignalKind::terminate()) {
                    Ok(s) => s,
                    Err(_) => return,
                };
                tokio::select! {
                    _ = sigterm.recv() => {}
                    _ = tokio::signal::ctrl_c() => {}
                }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
            shutdown.signal();
        }));
    }

    fn run_boot_phase(&self, ctx: &KernelContext) -> Result<(), KernelError> {
        let mut guard = self
            .inner
            .subsystems
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for s in guard.iter_mut() {
            let result = catch_unwind(AssertUnwindSafe(|| s.boot(ctx))).map_err(|payload| {
                KernelError::Subsystem {
                    code: KernelErrorCode::SubsystemBootFailed,
                    name: s.name(),
                    source: panic_message(&payload).into(),
                }
            })?;
            result?;
            self.record_booted(s.name());
        }
        Ok(())
    }

    fn run_load_phase(&self, ctx: &KernelContext) -> Result<(), KernelError> {
        let mut guard = self
            .inner
            .subsystems
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for s in guard.iter_mut() {
            let result = catch_unwind(AssertUnwindSafe(|| s.load(ctx))).map_err(|payload| {
                KernelError::Subsystem {
                    code: KernelErrorCode::SubsystemBootFailed,
                    name: s.name(),
                    source: panic_message(&payload).into(),
                }
            })?;
            result?;
            self.record_loaded(s.name());
        }
        Ok(())
    }

    /// Runs `shutdown` on every subsystem in reverse boot order,
    /// regardless of individual failures. Returns the first error
    /// encountered (if any).
    fn run_shutdown_phase(&self, ctx: &KernelContext) -> Result<(), KernelError> {
        let mut guard = self
            .inner
            .subsystems
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut first_err: Option<KernelError> = None;
        for s in guard.iter_mut().rev() {
            let outcome = catch_unwind(AssertUnwindSafe(|| s.shutdown(ctx)));
            match outcome {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    if first_err.is_none() {
                        first_err = Some(err);
                    }
                }
                Err(payload) => {
                    if first_err.is_none() {
                        first_err = Some(KernelError::Subsystem {
                            code: KernelErrorCode::SubsystemBootFailed,
                            name: s.name(),
                            source: panic_message(&payload).into(),
                        });
                    }
                }
            }
        }
        match first_err {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    /// Reverse-shutdown only those subsystems that successfully
    /// loaded. Used as a rollback after a load-phase failure.
    fn run_shutdown_phase_to_loaded(&self, ctx: &KernelContext) {
        let loaded: Vec<&'static str> = {
            let book = self
                .bookkeeping
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            book.loaded_at.keys().copied().collect()
        };
        let mut guard = self
            .inner
            .subsystems
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for s in guard.iter_mut().rev() {
            if loaded.contains(&s.name()) {
                let _ = catch_unwind(AssertUnwindSafe(|| s.shutdown(ctx)));
            }
        }
    }

    fn record_booted(&self, name: &'static str) {
        let mut book = self
            .bookkeeping
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = book.booted_at.insert(name, Instant::now());
    }

    fn record_loaded(&self, name: &'static str) {
        let mut book = self
            .bookkeeping
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = book.loaded_at.insert(name, Instant::now());
    }

    fn subsystem_snapshots(&self) -> Vec<SubsystemSnapshot> {
        let book = self
            .bookkeeping
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let guard = self
            .inner
            .subsystems
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard
            .iter()
            .map(|s| SubsystemSnapshot {
                id: book
                    .id
                    .get(s.name())
                    .copied()
                    .unwrap_or(SubsystemId::from_raw(0)),
                name: s.name(),
                dependencies: s.dependencies(),
                health: s.health(),
                booted_at: book.booted_at.get(s.name()).copied(),
                loaded_at: book.loaded_at.get(s.name()).copied(),
            })
            .collect()
    }
}

impl fmt::Debug for Kernel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let snap = self.snapshot();
        f.debug_struct("Kernel")
            .field("name", &snap.name)
            .field("state", &snap.lifecycle.state)
            .field("subsystems", &snap.subsystems.len())
            .finish_non_exhaustive()
    }
}

/// Best-effort extraction of a panic payload's message. Recognizes
/// `&'static str` and `String` payloads (the common cases produced
/// by `panic!`); anything else maps to `"<unknown panic>"`.
fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(msg) = payload.downcast_ref::<&'static str>() {
        format!("subsystem panicked: {}", msg)
    } else if let Some(msg) = payload.downcast_ref::<String>() {
        format!("subsystem panicked: {}", msg)
    } else {
        "subsystem panicked: <unknown panic>".to_owned()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::super::builder::KernelBuilder;
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn test_kernel_is_send_sync() {
        assert_send_sync::<Kernel>();
        assert_send_sync::<KernelSnapshot>();
    }

    #[test]
    fn test_new_kernel_starts_in_created_state() {
        let kernel = KernelBuilder::new("test").build().unwrap();
        assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Created);
    }

    #[test]
    fn test_boot_drives_lifecycle_to_running() {
        let kernel = KernelBuilder::new("test").build().unwrap();
        kernel.boot().unwrap();
        assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Running);
    }

    #[test]
    fn test_shutdown_signal_idempotent_no_op_when_not_running() {
        let kernel = KernelBuilder::new("test").build().unwrap();
        kernel.shutdown();
        kernel.shutdown();
        assert!(kernel.context().shutdown.is_signalled());
    }

    #[test]
    fn test_run_blocks_until_shutdown_then_terminates_cleanly() {
        let kernel = KernelBuilder::new("test").build().unwrap();
        let kernel_clone = kernel.clone();

        let join = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            kernel_clone.shutdown();
        });

        kernel.run().unwrap();
        join.join().unwrap();
        assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Stopped);
    }

    #[test]
    fn test_panicking_boot_is_caught_and_reported() {
        struct PanicSubsystem;

        impl Subsystem for PanicSubsystem {
            fn name(&self) -> &'static str {
                "panicker"
            }
            fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
                panic!("intentional");
            }
        }

        let kernel = KernelBuilder::new("test")
            .with_subsystem(PanicSubsystem)
            .build()
            .unwrap();
        let err = kernel.boot().unwrap_err();
        match err {
            KernelError::Subsystem { name, .. } => assert_eq!(name, "panicker"),
            other => panic!("expected Subsystem error, got {:?}", other),
        }
        assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Failed);
    }

    #[test]
    fn test_failing_load_runs_shutdown_on_loaded_subsystems() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static SHUTDOWN_COUNT: AtomicUsize = AtomicUsize::new(0);

        struct CountedShutdown;

        impl Subsystem for CountedShutdown {
            fn name(&self) -> &'static str {
                "counter"
            }
            fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
                Ok(())
            }
            fn load(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
                Ok(())
            }
            fn shutdown(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
                let _ = SHUTDOWN_COUNT.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
        }

        struct FailLoad;

        impl Subsystem for FailLoad {
            fn name(&self) -> &'static str {
                "fail_load"
            }
            fn dependencies(&self) -> &'static [&'static str] {
                &["counter"]
            }
            fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
                Ok(())
            }
            fn load(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
                Err(KernelError::Internal {
                    code: KernelErrorCode::Internal,
                    message: "load failed".to_owned(),
                })
            }
        }

        SHUTDOWN_COUNT.store(0, Ordering::Relaxed);

        let kernel = KernelBuilder::new("test")
            .with_subsystem(CountedShutdown)
            .with_subsystem(FailLoad)
            .build()
            .unwrap();
        let err = kernel.boot().unwrap_err();
        assert!(matches!(err, KernelError::Internal { .. }));
        assert_eq!(SHUTDOWN_COUNT.load(Ordering::Relaxed), 1);
        assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Failed);
    }

    #[test]
    fn test_snapshot_includes_subsystem_metadata() {
        struct Plain;

        impl Subsystem for Plain {
            fn name(&self) -> &'static str {
                "plain"
            }
            fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
                Ok(())
            }
        }

        let kernel = KernelBuilder::new("test")
            .with_subsystem(Plain)
            .build()
            .unwrap();
        kernel.boot().unwrap();
        let snap = kernel.snapshot();
        let plain = snap.subsystems.iter().find(|s| s.name == "plain").unwrap();
        assert!(plain.booted_at.is_some());
        assert!(plain.loaded_at.is_some());
    }

    #[test]
    fn test_context_returns_fresh_handles() {
        let kernel = KernelBuilder::new("test").build().unwrap();
        let ctx1 = kernel.context();
        let ctx2 = kernel.context();
        ctx1.shutdown.signal();
        assert!(ctx2.shutdown.is_signalled());
        assert_eq!(ctx2.kernel_name(), "test");
    }
}
