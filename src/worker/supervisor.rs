//! Single-task worker supervisor.
//!
//! [`Supervisor`] holds every registered worker, spawns each into a
//! [`tokio::task::JoinSet`], and drives a single
//! [`tokio::select!`] loop over join completions and an external
//! cancellation token. Restart policy, panic isolation, and
//! criticality response all live here.
//!
//! The supervisor is intentionally one-task. A per-worker timer
//! storm would scale poorly under a thousand workers; one
//! `select!` loop scales linearly with worker churn.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::AtomicI64;
use std::sync::Arc;
use std::time::Duration;

use tokio::task::{Id as TaskId, JoinSet};
use tokio_util::sync::CancellationToken;

use crate::events::{EventHandle, KernelEvent};
use crate::health::{HealthHandle, HealthStatus};
use crate::kernel::ShutdownHandle;
use crate::metrics::{names as metric_names, MetricsHandle};
use crate::primitives::{IdGenerator, Instant, WorkerId};

use super::context::WorkerContext;
use super::criticality::Criticality;
use super::event::WorkerLifecycleEvent;
use super::handle::WorkerHandle;
use super::panic::{catch_panic, PanicReason};
use super::policy::{CircuitBreaker, CircuitState};
use super::spec::WorkerSpec;
use super::state::WorkerState;
use super::traits::{AsyncWorker, Worker, WorkerError};
use super::watchdog::{Watchdog, WatchdogTarget};

/// Default grace period the supervisor gives in-flight tasks to
/// finish after cancellation, before it aborts stragglers. Wraps a
/// real shutdown drain in Milestone H.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// Default sliding window for `MaxRetries` failure counting.
const FAILURE_WINDOW_NANOS: i64 = 60 * 1_000_000_000; // 60 s

/// Outcome of a single worker run, returned by the spawned task.
enum SupervisorOutcome {
    Completed,
    Failed(WorkerError),
    Panicked(PanicReason),
}

/// Sync or async worker held behind a trait object.
enum WorkerRunner {
    Sync(Arc<dyn Worker>),
    Async(Arc<dyn AsyncWorker>),
}

/// Supervisor-side state for a single registered worker.
struct RegisteredWorker {
    handle: WorkerHandle,
    spec: WorkerSpec,
    runner: WorkerRunner,
    last_heartbeat: Arc<AtomicI64>,
    attempts: u32,
    circuit: Option<Arc<CircuitBreaker>>,
}

impl fmt::Debug for RegisteredWorker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegisteredWorker")
            .field("name", &self.handle.name())
            .field("attempts", &self.attempts)
            .finish_non_exhaustive()
    }
}

/// The supervisor itself.
///
/// Construct via [`Supervisor::new`], register workers via
/// [`Supervisor::register`] / [`Supervisor::register_async`], then
/// drive the loop with [`Supervisor::run`]. The kernel calls
/// `run().await` during `Phase::Exec` and arranges for the
/// supervisor's cancel token to fire on kernel shutdown.
pub struct Supervisor {
    registered: HashMap<WorkerId, RegisteredWorker>,
    join_set: JoinSet<(WorkerId, SupervisorOutcome)>,
    task_to_worker: HashMap<TaskId, WorkerId>,
    cancel_token: CancellationToken,
    events: EventHandle,
    health: HealthHandle,
    metrics: MetricsHandle,
    shutdown: ShutdownHandle,
    id_gen: IdGenerator,
    handles: Vec<WorkerHandle>,
    watchdog: Watchdog,
}

impl Supervisor {
    /// Constructs a new supervisor with empty registration.
    #[must_use]
    pub fn new(
        events: EventHandle,
        health: HealthHandle,
        metrics: MetricsHandle,
        shutdown: ShutdownHandle,
    ) -> Self {
        Self {
            registered: HashMap::new(),
            join_set: JoinSet::new(),
            task_to_worker: HashMap::new(),
            cancel_token: CancellationToken::new(),
            events,
            health,
            metrics,
            shutdown,
            id_gen: IdGenerator::new(),
            handles: Vec::new(),
            watchdog: Watchdog::new(super::watchdog::DEFAULT_TICK),
        }
    }

    /// Constructs a supervisor with a custom watchdog tick period.
    ///
    /// Used by tests that want a faster watchdog cadence.
    #[must_use]
    pub fn with_watchdog_period(
        events: EventHandle,
        health: HealthHandle,
        metrics: MetricsHandle,
        shutdown: ShutdownHandle,
        period: Duration,
    ) -> Self {
        let mut s = Self::new(events, health, metrics, shutdown);
        s.watchdog = Watchdog::new(period);
        s
    }

    /// Returns the supervisor's external cancellation token.
    ///
    /// The kernel cancels this token on shutdown to wind the
    /// supervisor down.
    #[inline]
    #[must_use]
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel_token.clone()
    }

    /// Returns one [`WorkerHandle`] per registered worker.
    #[inline]
    #[must_use]
    pub fn handles(&self) -> Vec<WorkerHandle> {
        self.handles.clone()
    }

    /// Registers a synchronous worker.
    pub fn register(&mut self, spec: WorkerSpec, worker: Arc<dyn Worker>) -> WorkerHandle {
        self.register_inner(spec, WorkerRunner::Sync(worker))
    }

    /// Registers an asynchronous worker.
    pub fn register_async(
        &mut self,
        spec: WorkerSpec,
        worker: Arc<dyn AsyncWorker>,
    ) -> WorkerHandle {
        self.register_inner(spec, WorkerRunner::Async(worker))
    }

    fn register_inner(&mut self, spec: WorkerSpec, runner: WorkerRunner) -> WorkerHandle {
        let id = self.id_gen.next_worker_id();
        let token = self.cancel_token.child_token();
        let handle = WorkerHandle::new(id, spec.name, token);
        let last_heartbeat = Arc::new(AtomicI64::new(0));
        let circuit = spec
            .circuit
            .as_ref()
            .map(|policy| Arc::new(CircuitBreaker::new(policy.clone())));
        let registered = RegisteredWorker {
            handle: handle.clone(),
            spec,
            runner,
            last_heartbeat,
            attempts: 0,
            circuit,
        };
        let _ = self.registered.insert(id, registered);
        self.handles.push(handle.clone());
        handle
    }

    /// Drives the supervisor loop until the external cancel token
    /// fires (or every worker has stopped permanently).
    pub async fn run(&mut self) -> Result<(), SupervisorError> {
        // Spawn every registered worker.
        let ids: Vec<WorkerId> = self.registered.keys().copied().collect();
        for id in ids {
            self.spawn_one(id);
        }

        loop {
            tokio::select! {
                biased;
                () = self.cancel_token.cancelled() => {
                    let _ = self.shutdown_all().await;
                    return Ok(());
                }
                _ = self.watchdog.tick() => {
                    self.watchdog_tick().await;
                }
                result = self.join_set.join_next_with_id() => {
                    match result {
                        Some(Ok((task_id, (worker_id, outcome)))) => {
                            let _ = self.task_to_worker.remove(&task_id);
                            self.handle_outcome(worker_id, outcome).await;
                        }
                        Some(Err(join_err)) => {
                            let task_id = join_err.id();
                            let worker_id = self.task_to_worker.remove(&task_id);
                            if let Some(id) = worker_id {
                                if join_err.is_panic() {
                                    let payload = join_err.into_panic();
                                    let reason = PanicReason::from_payload(payload);
                                    self.handle_outcome(id, SupervisorOutcome::Panicked(reason))
                                        .await;
                                }
                                // is_cancelled / aborted -> shutdown path; ignored.
                            }
                        }
                        None => {
                            // No active workers; wait for cancellation or
                            // a watchdog tick (which may discover open
                            // breakers ready to advance to HalfOpen).
                            tokio::select! {
                                () = self.cancel_token.cancelled() => {
                                    let _ = self.shutdown_all().await;
                                    return Ok(());
                                }
                                _ = self.watchdog.tick() => {
                                    self.watchdog_tick().await;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Watchdog tick handler: scans heartbeats, advances open
    /// circuits to HalfOpen, and treats heartbeat-silent workers as
    /// failures (which feeds back into the same restart/circuit path).
    async fn watchdog_tick(&mut self) {
        // Step 1: build the heartbeat target list and check.
        let targets: Vec<WatchdogTarget> = self
            .registered
            .values()
            .filter_map(|r| {
                let interval = r.spec.heartbeat_interval?;
                Some(WatchdogTarget {
                    id: r.handle.id(),
                    name: r.spec.name,
                    last_heartbeat_nanos: r
                        .last_heartbeat
                        .load(std::sync::atomic::Ordering::Acquire),
                    heartbeat_interval: interval,
                })
            })
            .collect();
        let now_nanos = unix_nanos();
        let timeouts = Watchdog::check(&targets, now_nanos);

        // Step 2: advance any open circuit breakers.
        let mut closed_to_halfopen: Vec<(WorkerId, &'static str)> = Vec::new();
        for (id, registered) in self.registered.iter() {
            if let Some(breaker) = registered.circuit.as_ref() {
                let prior = breaker.state();
                let after = breaker.tick();
                if prior == CircuitState::Open && after == CircuitState::HalfOpen {
                    closed_to_halfopen.push((*id, registered.spec.name));
                }
            }
        }
        for (id, name) in closed_to_halfopen {
            let now = Instant::now();
            self.events
                .emit(KernelEvent::Worker(crate::events::WorkerEvent {
                    event: WorkerLifecycleEvent::CircuitHalfOpened { id, name, at: now },
                }));
            // Spawn a trial run for the worker, if it isn't already
            // running.
            self.spawn_one_if_idle(id);
        }

        // Step 3: treat heartbeat-silent workers as failures.
        for timeout in timeouts {
            self.events
                .emit(KernelEvent::Worker(crate::events::WorkerEvent {
                    event: WorkerLifecycleEvent::Timeout {
                        id: timeout.id,
                        name: timeout.name,
                        silent_for: timeout.silent_for,
                        at: Instant::now(),
                    },
                }));
            // Cancel the worker's child token so its task exits, then
            // let the join_set pick up the failure on the next loop.
            if let Some(reg) = self.registered.get(&timeout.id) {
                reg.handle.cancel_token().cancel();
            }
        }
    }

    fn spawn_one_if_idle(&mut self, worker_id: WorkerId) {
        // If a task for this worker is already in-flight, skip.
        let already_running = self
            .task_to_worker
            .values()
            .any(|id| *id == worker_id);
        if !already_running {
            self.spawn_one(worker_id);
        }
    }

    /// Spawns one registered worker into the JoinSet.
    fn spawn_one(&mut self, worker_id: WorkerId) {
        let registered = match self.registered.get_mut(&worker_id) {
            Some(r) => r,
            None => return,
        };

        let ctx = WorkerContext::new(
            worker_id,
            registered.handle.name(),
            registered.handle.cancel_token(),
            Arc::clone(&registered.last_heartbeat),
            Arc::clone(&self.metrics),
            self.events.clone(),
            self.health.clone(),
        );

        registered.handle.set_state(WorkerState::Starting);

        let task_id = match &registered.runner {
            WorkerRunner::Sync(worker) => {
                let worker = Arc::clone(worker);
                let id = worker_id;
                self.join_set
                    .spawn_blocking(move || run_sync(id, worker, ctx))
                    .id()
            }
            WorkerRunner::Async(worker) => {
                let worker = Arc::clone(worker);
                let id = worker_id;
                self.join_set.spawn(run_async(id, worker, ctx)).id()
            }
        };

        let _ = self.task_to_worker.insert(task_id, worker_id);
        if let Some(reg) = self.registered.get(&worker_id) {
            reg.handle.set_state(WorkerState::Running);
            self.events
                .emit(KernelEvent::Worker(crate::events::WorkerEvent {
                    event: WorkerLifecycleEvent::Started {
                        id: worker_id,
                        name: reg.spec.name,
                        at: Instant::now(),
                    },
                }));
        }
    }

    async fn handle_outcome(&mut self, worker_id: WorkerId, outcome: SupervisorOutcome) {
        let now = Instant::now();
        let (event, has_failed) = match outcome {
            SupervisorOutcome::Completed => {
                let registered = match self.registered.get(&worker_id) {
                    Some(r) => r,
                    None => return,
                };
                let event = WorkerLifecycleEvent::Stopped {
                    id: worker_id,
                    name: registered.spec.name,
                    at: now,
                };
                (event, false)
            }
            SupervisorOutcome::Failed(err) => {
                let registered = match self.registered.get(&worker_id) {
                    Some(r) => r,
                    None => return,
                };
                let event = WorkerLifecycleEvent::Failed {
                    id: worker_id,
                    name: registered.spec.name,
                    reason: err.message,
                    at: now,
                };
                (event, true)
            }
            SupervisorOutcome::Panicked(reason) => {
                let registered = match self.registered.get(&worker_id) {
                    Some(r) => r,
                    None => return,
                };
                let event = WorkerLifecycleEvent::Panicked {
                    id: worker_id,
                    name: registered.spec.name,
                    reason,
                    at: now,
                };
                (event, true)
            }
        };

        self.events
            .emit(KernelEvent::Worker(crate::events::WorkerEvent {
                event: event.clone(),
            }));

        if has_failed {
            self.metrics.counter(
                metric_names::WORKERS_FAILED,
                1,
                &[("worker", self.worker_name_for(worker_id))],
            );
        }

        self.maybe_restart(worker_id, has_failed).await;
    }

    fn worker_name_for(&self, worker_id: WorkerId) -> &'static str {
        self.registered
            .get(&worker_id)
            .map(|r| r.spec.name)
            .unwrap_or("")
    }

    async fn maybe_restart(&mut self, worker_id: WorkerId, has_failed: bool) {
        // Step A: run the circuit breaker and decide whether
        // restart is permitted.
        let circuit_open = self.advance_circuit(worker_id, has_failed);

        let (should_restart, delay, attempts, name, criticality) = {
            let registered = match self.registered.get_mut(&worker_id) {
                Some(r) => r,
                None => return,
            };
            let attempts = if has_failed {
                registered.handle.record_failure(FAILURE_WINDOW_NANOS)
            } else {
                0
            };
            let policy_allows =
                registered.spec.restart.should_restart(has_failed, attempts);
            let should_restart = policy_allows && !circuit_open;
            let delay = registered.spec.backoff.delay(attempts);
            registered.attempts = attempts;
            (
                should_restart,
                delay,
                attempts,
                registered.spec.name,
                registered.spec.criticality,
            )
        };

        if should_restart {
            if let Some(reg) = self.registered.get(&worker_id) {
                reg.handle.set_state(WorkerState::Restarting);
            }
            if !delay.is_zero() {
                tokio::select! {
                    () = tokio::time::sleep(delay) => {}
                    () = self.cancel_token.cancelled() => return,
                }
            }
            self.events
                .emit(KernelEvent::Worker(crate::events::WorkerEvent {
                    event: WorkerLifecycleEvent::Restarted {
                        id: worker_id,
                        name,
                        attempt: attempts.saturating_add(1),
                        at: Instant::now(),
                    },
                }));
            self.metrics
                .counter(metric_names::WORKERS_RESTARTED, 1, &[("worker", name)]);
            self.spawn_one(worker_id);
        } else {
            if let Some(reg) = self.registered.get(&worker_id) {
                reg.handle.set_state(WorkerState::Stopped);
            }
            self.events
                .emit(KernelEvent::Worker(crate::events::WorkerEvent {
                    event: WorkerLifecycleEvent::Stopped {
                        id: worker_id,
                        name,
                        at: Instant::now(),
                    },
                }));
            if has_failed {
                self.apply_criticality(name, criticality);
            }
        }
    }

    /// Records the outcome on the worker's circuit breaker (if any)
    /// and emits state-transition events.
    ///
    /// Returns `true` when the breaker is currently `Open` after the
    /// update — the caller suppresses any restart in that case.
    fn advance_circuit(&self, worker_id: WorkerId, has_failed: bool) -> bool {
        let registered = match self.registered.get(&worker_id) {
            Some(r) => r,
            None => return false,
        };
        let breaker = match registered.circuit.as_ref() {
            Some(b) => b,
            None => return false,
        };

        let prior = breaker.state();
        let next = if has_failed {
            breaker.record_failure()
        } else {
            breaker.record_success()
        };
        let now = Instant::now();
        match (prior, next) {
            (CircuitState::Closed, CircuitState::Open)
            | (CircuitState::HalfOpen, CircuitState::Open) => {
                let failures = registered.attempts.saturating_add(1);
                self.events
                    .emit(KernelEvent::Worker(crate::events::WorkerEvent {
                        event: WorkerLifecycleEvent::CircuitOpened {
                            id: worker_id,
                            name: registered.spec.name,
                            failures,
                            at: now,
                        },
                    }));
            }
            (CircuitState::HalfOpen, CircuitState::Closed) => {
                self.events
                    .emit(KernelEvent::Worker(crate::events::WorkerEvent {
                        event: WorkerLifecycleEvent::CircuitClosed {
                            id: worker_id,
                            name: registered.spec.name,
                            at: now,
                        },
                    }));
            }
            _ => {}
        }
        matches!(breaker.state(), CircuitState::Open)
    }

    fn apply_criticality(&self, worker_name: &'static str, criticality: Criticality) {
        match criticality {
            Criticality::Critical => {
                self.shutdown.signal();
            }
            Criticality::Essential => {
                self.health.report(worker_name, HealthStatus::Unhealthy);
            }
            Criticality::Optional => {
                self.health.report(worker_name, HealthStatus::Degraded);
            }
            Criticality::Background => {
                // Best effort — no health side effects.
            }
        }
    }

    /// Cancels every worker, waits up to `SHUTDOWN_GRACE` for them
    /// to finish, then aborts stragglers. Returns
    /// `(drained, aborted)` counts.
    async fn shutdown_all(&mut self) -> (usize, usize) {
        for registered in self.registered.values() {
            registered.handle.set_state(WorkerState::Stopping);
            registered.handle.cancel_token().cancel();
            self.events
                .emit(KernelEvent::Worker(crate::events::WorkerEvent {
                    event: WorkerLifecycleEvent::Stopping {
                        id: registered.handle.id(),
                        at: Instant::now(),
                    },
                }));
        }

        let drained = std::sync::atomic::AtomicUsize::new(0);
        let aborted = std::sync::atomic::AtomicUsize::new(0);

        let drain_fut = async {
            while let Some(result) = self.join_set.join_next_with_id().await {
                match &result {
                    Ok((task_id, _)) => {
                        let _ = drained.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let _ = self.task_to_worker.remove(task_id);
                    }
                    Err(err) => {
                        let task_id = err.id();
                        if err.is_cancelled() {
                            let _ = aborted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        } else {
                            let _ = drained.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                        let _ = self.task_to_worker.remove(&task_id);
                    }
                }
            }
        };

        let timeout_result = tokio::time::timeout(SHUTDOWN_GRACE, drain_fut).await;
        if timeout_result.is_err() {
            self.join_set.abort_all();
            while let Some(result) = self.join_set.join_next_with_id().await {
                match result {
                    Ok((task_id, _)) => {
                        let _ = drained.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let _ = self.task_to_worker.remove(&task_id);
                    }
                    Err(err) => {
                        let task_id = err.id();
                        if err.is_cancelled() {
                            let _ = aborted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        } else {
                            let _ = drained.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                        let _ = self.task_to_worker.remove(&task_id);
                    }
                }
            }
        }

        for registered in self.registered.values() {
            registered.handle.set_state(WorkerState::Stopped);
            self.events
                .emit(KernelEvent::Worker(crate::events::WorkerEvent {
                    event: WorkerLifecycleEvent::Stopped {
                        id: registered.handle.id(),
                        name: registered.spec.name,
                        at: Instant::now(),
                    },
                }));
        }

        (
            drained.load(std::sync::atomic::Ordering::Relaxed),
            aborted.load(std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// Public wrapper around the internal shutdown drain. Returns
    /// `(drained, aborted)` counts.
    pub async fn drain_all(&mut self) -> (usize, usize) {
        self.shutdown_all().await
    }
}

impl fmt::Debug for Supervisor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Supervisor")
            .field("registered", &self.registered.len())
            .finish_non_exhaustive()
    }
}

/// Errors returned by [`Supervisor::run`].
#[non_exhaustive]
#[derive(Debug)]
pub enum SupervisorError {
    /// Currently unused; reserved for future variants.
    InternalUnused,
}

impl fmt::Display for SupervisorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SupervisorError::InternalUnused => f.write_str("supervisor internal error"),
        }
    }
}

impl std::error::Error for SupervisorError {}

fn run_sync(
    worker_id: WorkerId,
    worker: Arc<dyn Worker>,
    ctx: WorkerContext,
) -> (WorkerId, SupervisorOutcome) {
    let outcome = catch_panic(std::panic::AssertUnwindSafe(|| worker.run(ctx)));
    let outcome = match outcome {
        Ok(Ok(())) => SupervisorOutcome::Completed,
        Ok(Err(e)) => SupervisorOutcome::Failed(e),
        Err(reason) => SupervisorOutcome::Panicked(reason),
    };
    (worker_id, outcome)
}

async fn run_async(
    worker_id: WorkerId,
    worker: Arc<dyn AsyncWorker>,
    ctx: WorkerContext,
) -> (WorkerId, SupervisorOutcome) {
    match worker.run(ctx).await {
        Ok(()) => (worker_id, SupervisorOutcome::Completed),
        Err(e) => (worker_id, SupervisorOutcome::Failed(e)),
    }
}

fn unix_nanos() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::errors::ErrorRegistry;
    use crate::events::EventDispatcher;
    use crate::health::HealthRegistry;
    use crate::kernel::handles::ShutdownInner;
    use crate::metrics::NoopMetricsBackend;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn assert_send_sync<T: Send + Sync>() {}

    fn build_supervisor() -> (Supervisor, Arc<EventDispatcher>, Arc<ShutdownInner>) {
        let dispatcher = Arc::new(EventDispatcher::new());
        let health = Arc::new(HealthRegistry::new());
        let metrics: MetricsHandle = Arc::new(NoopMetricsBackend);
        let shutdown_inner = Arc::new(ShutdownInner::new());
        let shutdown = ShutdownHandle::new(Arc::clone(&shutdown_inner));
        let _ = ErrorRegistry::new(); // unused but ensures the symbol resolves
        (
            Supervisor::new(dispatcher.handle(), health.handle(), metrics, shutdown),
            dispatcher,
            shutdown_inner,
        )
    }

    #[test]
    fn test_supervisor_is_send_sync() {
        assert_send_sync::<Supervisor>();
    }

    struct OkSync {
        ran: Arc<AtomicUsize>,
    }

    impl Worker for OkSync {
        fn name(&self) -> &'static str {
            "ok-sync"
        }
        fn run(&self, _ctx: WorkerContext) -> Result<(), WorkerError> {
            let _ = self.ran.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_sync_worker_runs_to_completion() {
        let (mut sup, dispatcher, _shutdown) = build_supervisor();
        let ran = Arc::new(AtomicUsize::new(0));
        let _ = sup.register(
            WorkerSpec::new("ok-sync").restart_never(),
            Arc::new(OkSync {
                ran: Arc::clone(&ran),
            }),
        );

        let started = Arc::new(AtomicUsize::new(0));
        let started_cb = Arc::clone(&started);
        let _ = dispatcher.subscribe("kernel.worker.started", move |_| {
            let _ = started_cb.fetch_add(1, Ordering::Relaxed);
        });

        let cancel = sup.cancel_token();
        drop(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            cancel.cancel();
        }));

        sup.run().await.unwrap();

        assert_eq!(ran.load(Ordering::Relaxed), 1);
        assert!(started.load(Ordering::Relaxed) >= 1);
    }

    struct OkAsync {
        ran: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl AsyncWorker for OkAsync {
        fn name(&self) -> &'static str {
            "ok-async"
        }
        async fn run(&self, _ctx: WorkerContext) -> Result<(), WorkerError> {
            let _ = self.ran.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_async_worker_runs_to_completion() {
        let (mut sup, _dispatcher, _shutdown) = build_supervisor();
        let ran = Arc::new(AtomicUsize::new(0));
        let _ = sup.register_async(
            WorkerSpec::new("ok-async").restart_never(),
            Arc::new(OkAsync {
                ran: Arc::clone(&ran),
            }),
        );

        let cancel = sup.cancel_token();
        drop(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            cancel.cancel();
        }));

        sup.run().await.unwrap();
        assert_eq!(ran.load(Ordering::Relaxed), 1);
    }

    struct FailingSync {
        attempts: Arc<AtomicUsize>,
    }

    impl Worker for FailingSync {
        fn name(&self) -> &'static str {
            "failing-sync"
        }
        fn run(&self, _ctx: WorkerContext) -> Result<(), WorkerError> {
            let n = self.attempts.fetch_add(1, Ordering::Relaxed);
            if n < 2 {
                Err(WorkerError::new("fail"))
            } else {
                Ok(())
            }
        }
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn test_failing_sync_worker_restarts_then_succeeds() {
        let (mut sup, _dispatcher, _shutdown) = build_supervisor();
        let attempts = Arc::new(AtomicUsize::new(0));
        let _ = sup.register(
            WorkerSpec::new("failing-sync")
                .restart_max_retries(5, Duration::from_secs(60))
                .backoff_fixed(Duration::from_millis(1)),
            Arc::new(FailingSync {
                attempts: Arc::clone(&attempts),
            }),
        );

        let cancel = sup.cancel_token();
        drop(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            cancel.cancel();
        }));

        sup.run().await.unwrap();
        assert!(attempts.load(Ordering::Relaxed) >= 3);
    }

    struct PanickingAsync;

    #[async_trait::async_trait]
    impl AsyncWorker for PanickingAsync {
        fn name(&self) -> &'static str {
            "panicking-async"
        }
        async fn run(&self, _ctx: WorkerContext) -> Result<(), WorkerError> {
            panic!("intentional");
        }
    }

    #[tokio::test]
    async fn test_panicking_async_worker_emits_panicked_event() {
        let (mut sup, dispatcher, _shutdown) = build_supervisor();
        let panicked = Arc::new(AtomicUsize::new(0));
        let cb = Arc::clone(&panicked);
        let _ = dispatcher.subscribe("kernel.worker.panicked", move |_| {
            let _ = cb.fetch_add(1, Ordering::Relaxed);
        });
        let _ = sup.register_async(
            WorkerSpec::new("panicking-async").restart_never(),
            Arc::new(PanickingAsync),
        );

        let cancel = sup.cancel_token();
        drop(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            cancel.cancel();
        }));

        sup.run().await.unwrap();
        assert_eq!(panicked.load(Ordering::Relaxed), 1);
    }

    struct CriticalFailing;

    impl Worker for CriticalFailing {
        fn name(&self) -> &'static str {
            "critical"
        }
        fn run(&self, _ctx: WorkerContext) -> Result<(), WorkerError> {
            Err(WorkerError::new("can't run"))
        }
    }

    #[tokio::test]
    async fn test_critical_worker_failure_signals_shutdown() {
        let (mut sup, _dispatcher, shutdown_inner) = build_supervisor();
        let _ = sup.register(
            WorkerSpec::new("critical").critical().restart_never(),
            Arc::new(CriticalFailing),
        );

        let cancel = sup.cancel_token();
        drop(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            cancel.cancel();
        }));

        sup.run().await.unwrap();
        assert!(shutdown_inner.is_signalled());
    }
}
