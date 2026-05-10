# Changelog

All notable changes to `service-kernel` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Versioning Policy

This project starts at `0.1.0` and never publishes `0.0.x` versions.

- **Minor versions** (`0.1.0`, `0.2.0`, `0.3.0`, ...) introduce new features or break the API.
- **Patch versions** (`0.1.1`, `0.1.2`, ...) are bug fixes only — no API changes.

`1.0.0` will mark the public API stabilization commitment.

## [Unreleased]

### Added

- `worker::adapter::proc_daemon` module (feature-gated behind
  `daemon`):
  - `DaemonConfig` — fluent-builder configuration with `pid_file`,
    `working_dir`, `user`, `group`, `stdout`, `stderr`. The user,
    group, and redirect paths are reserved for the future
    double-fork integration; the current adapter handles PID file
    + working-dir lifecycle.
  - `DaemonAdapter` — wraps the `proc-daemon` dependency.
    `daemonize()` switches the working directory and writes a PID
    file (RAII-cleaned on drop). `run(kernel)` installs the
    kernel's signal handler and runs it; on exit the PID file is
    removed automatically.
  - Single daemon per kernel (one kernel = one process = one
    daemon, per `.dev/PLAN.md` §6.8).
  - Documented signal-handling coordination: the adapter delegates
    to `Kernel::install_signal_handler` to avoid double-registering
    handlers with `proc-daemon`.
- Three runnable examples under `examples/`:
  - `minimal.rs` — smallest possible kernel, prints lifecycle
    transitions, auto-shuts-down after 1 second.
  - `workers.rs` — supervised heartbeat + flakey workers
    demonstrating restart with exponential backoff (requires the
    `tokio` feature).
  - `full.rs` — every kernel feature exercised end-to-end: custom
    subsystem, custom error classifier, custom metrics backend,
    supervised worker with circuit policy, shutdown hook, OS
    signal handling (requires the `tokio` feature).
- Crate-level documentation refresh in `lib.rs`:
  - "Quick start" section with a runnable doc test.
  - Module map and feature-flag tables.
  - Pre-stable status note for the `0.1.x` series.
- `HookError` added to the prelude so consumers can `impl
  ShutdownHook for ...` without reaching into
  `service_kernel::shutdown::HookError` directly.
- `[[example]]` entries with `required-features = ["tokio"]` for
  `workers` and `full` so `cargo build --all-targets
  --no-default-features` no longer attempts to compile them.
- `[dev-dependencies]` additions: `thiserror = "1"` (used by
  `examples/full.rs`) and `async-trait = "0.1"` (mandatory for
  async-trait-using tests and examples).

- `shutdown` module (feature-gated behind `tokio`) — graceful
  shutdown coordination:
  - `ShutdownToken` — clean wrapper around
    `tokio_util::sync::CancellationToken`. `new`, `child`, `signal`,
    `is_signalled`, `signalled().await`. Cancelling the parent
    cancels children; cancelling a child does not propagate up.
  - `ShutdownHook` trait — async hook called during shutdown.
    Implementations live behind `Box<dyn ShutdownHook>` and run in
    registration order, each bounded by the remaining grace period.
  - `ShutdownContext` — `{ events, deadline }` bundle handed to
    every hook. `remaining()` returns time left before the deadline.
  - `HookError` — typed error returned from hooks; failures are
    recorded in the report but do not stop the sequence.
  - `drain` function — generic `JoinSet<T>` drain with grace +
    abort. Returns `DrainOutcome { drained, aborted, elapsed }`.
    Pattern lifted from `hive-system`'s `drain_connections` and
    generalized.
  - `ShutdownCoordinator` — orchestrates the shutdown sequence.
    `register_hook` / `shutdown` / `token` / `grace` /
    `hook_count` methods. `shutdown` is single-shot — second calls
    return a no-op report.
  - `ShutdownReport` — full record of what happened during
    shutdown: workers drained vs aborted, hooks succeeded vs
    failed, subsystems shutdown vs failed, total duration.
    `is_clean()` returns `true` only when nothing went wrong.
- `LifecycleEvent` extended with two new variants:
  `ShutdownStarted { at }` and `ShutdownCompleted { duration,
  workers_drained, workers_aborted, at }`. Topics route to
  `kernel.lifecycle.shutdown_started` and
  `kernel.lifecycle.shutdown_completed`.
- `Kernel::register_shutdown_hook<H: ShutdownHook>(hook)` — register
  a consumer-supplied shutdown hook that runs during the kernel's
  shutdown sequence.
- `Kernel::shutdown_token()` — clone of the kernel's
  `ShutdownToken`. Equivalent to `ShutdownHandle::signal()`; both
  trigger the same sequence.
- `Kernel::install_signal_handler()` — opt-in helper that listens
  for `SIGTERM` (Unix) and `Ctrl+C` (cross-platform) and signals
  shutdown when either fires. Idempotent. Cross-platform via
  `tokio::signal`.
- `Kernel::run()` integration:
  - When workers exist or a signal handler was requested, the
    kernel builds a multi-thread Tokio runtime; otherwise a tiny
    current-thread runtime hosts the coordinator's hook-running
    pass on its way out.
  - The coordinator's `shutdown()` runs after the supervisor
    exits (or after the wait completes for worker-less kernels),
    emitting `ShutdownStarted` / `ShutdownCompleted` events and
    running registered hooks.
- `Supervisor::shutdown_all` now returns `(drained, aborted)`
  counts. `Supervisor::drain_all` is the public wrapper.
- Drain stress test: 1000 mixed quick/slow tasks drained in a
  single call with a 200 ms grace; verifies the count split and
  bounded elapsed time.
- Prelude re-exports `ShutdownContext`, `ShutdownCoordinator`,
  `ShutdownHook`, `ShutdownReport`, `ShutdownToken` (gated on
  `tokio`).

- `worker::watchdog` — single-Tokio-interval liveness scanner driven
  from the supervisor's `select!` loop. Pure `check` function takes a
  list of [`WatchdogTarget`]s plus a `now_nanos` and returns
  [`WatchdogTimeout`]s for workers silent longer than 2× their
  configured `heartbeat_interval` (the 2× grace factor catches fully
  unresponsive workers without false-flagging the slightly slow).
  Default tick period 1s.
- `worker::policy::circuit` module:
  - `CircuitState` — `Closed / Open / HalfOpen`.
  - `CircuitPolicy` — `failure_threshold`, `failure_window`,
    `open_duration`. Default `(3, 60s, 30s)`.
  - `CircuitBreaker` — atomic state and failure counter, `Mutex`-guarded
    timestamps. `record_failure` / `record_success` / `tick` /
    `allow` / `state` methods. Failures outside the sliding window
    reset the counter. `tick` advances `Open → HalfOpen` after the
    open duration has elapsed.
- `WorkerSpec::circuit(policy)` and `no_circuit()` builders, plus a
  `circuit: Option<CircuitPolicy>` field on the spec.
- `WorkerLifecycleEvent` extended with four new variants:
  `CircuitOpened { failures }`, `CircuitHalfOpened`,
  `CircuitClosed`, and `Timeout { silent_for }`. `kind()` returns
  `circuit_opened`, `circuit_half_opened`, `circuit_closed`, and
  `timeout` respectively, with corresponding entries in
  `events::topic::worker_topic`.
- Supervisor wiring:
  - The `select!` loop now awaits `watchdog.tick()` alongside the
    `JoinSet` and cancellation paths.
  - `watchdog_tick` builds a target list from registered workers,
    advances open circuit breakers (emitting `CircuitHalfOpened`
    and spawning a trial run), and treats heartbeat-silent workers
    as failures by cancelling their child tokens.
  - The failure-handling path runs the worker's circuit breaker
    before consulting the restart policy. An `Open` circuit
    suppresses restart even if `RestartPolicy` would permit it.
  - `record_success` on a HalfOpen breaker emits `CircuitClosed`
    and the worker continues normally.
- New `Supervisor::with_watchdog_period` constructor for tests that
  want a faster watchdog cadence than the 1-second default.
- Prelude re-exports `CircuitPolicy` and `CircuitState` so consumer
  call sites see the typed enum without reaching into
  `service_kernel::worker::policy`.
- Stress test: 200 always-heartbeating workers establish a quiet
  baseline; a subset is then silenced and reported as `Timeout`
  events within the watchdog's grace window.

- `worker` module — supervised long-running work and the supervisor:
  - `Worker` and `AsyncWorker` traits — sync and async entry points,
    object-safe behind `Arc<dyn Worker>` / `Arc<dyn AsyncWorker>`.
    `AsyncWorker` uses the `async_trait` macro for object-safety
    (stable Rust's native `async fn in trait` is not yet object-safe).
  - `WorkerError` — typed worker failure with optional source. `Display`
    routes through `lang_lib::t!`.
  - `WorkerSpec` — fluent-builder configuration: `critical()`,
    `essential()`, `optional()`, `background()`,
    `restart_never()`, `restart_on_failure()`, `restart_always()`,
    `restart_max_retries()`, `backoff_none()`, `backoff_fixed()`,
    `backoff_exponential()`, `heartbeat()`, `timeout()`. Default
    spec: `Optional` criticality, `OnFailure` restart, exponential
    backoff (100 ms base, 30 s cap).
  - `Criticality` — four-level enum (`Background < Optional <
    Essential < Critical`) with `Ord` for worst-case aggregation.
  - `WorkerState` — nine-variant state vocabulary
    (`Created / Starting / Running / Idle / Busy / Failed /
    Restarting / Stopping / Stopped`).
  - `RestartPolicy` — `Never / OnFailure / Always / MaxRetries`.
  - `BackoffPolicy` — `None / Fixed / Exponential` with saturating
    arithmetic so a runaway attempt counter cannot wrap.
  - `PanicReason` + `catch_panic` — typed normalization of
    `catch_unwind` payloads.
  - `WorkerLifecycleEvent` — nine-variant event vocabulary
    (`Started / Heartbeat / BecameIdle / BecameBusy / Failed /
    Panicked / Restarted / Stopping / Stopped`). `WorkerEvent`
    promoted from the Milestone C placeholder to wrap a real
    `WorkerLifecycleEvent`.
  - `WorkerContext` (gated on `tokio`) — per-worker handle bundle
    with cooperative cancellation (sync + async), heartbeat
    tracking, and access to `events`, `metrics`, `health` handles.
  - `WorkerHandle` (gated on `tokio`) — observable view of one
    worker's state, queryable from `KernelSnapshot`.
  - `Supervisor` (gated on `tokio`) — single-Tokio-task supervisor
    driving a `tokio::select!` loop over a `tokio::task::JoinSet`.
    Catches sync-worker panics through `catch_panic` inside the
    `spawn_blocking` closure; catches async-worker panics through
    `JoinError::is_panic` with task-id-to-worker-id mapping. Applies
    restart policy with backoff sleep, applies criticality on
    non-restartable failures (Critical → kernel shutdown,
    Essential → health Unhealthy, Optional → health Degraded,
    Background → no-op), emits `kernel.workers.failed` and
    `kernel.workers.restarted` counters.
- Value-type subset of the worker module (`Criticality`,
  `WorkerState`, `WorkerSpec`, `WorkerLifecycleEvent`, `PanicReason`,
  `RestartPolicy`, `BackoffPolicy`) is **always** compiled, even
  without the `tokio` feature, so `KernelEvent::Worker` carries the
  same payload regardless of feature configuration.
- `KernelBuilder::with_worker` and `with_async_worker` (gated on
  `tokio`) — register sync and async workers respectively.
- `Kernel::run()` extended: when the `tokio` feature is enabled and
  workers have been registered, the kernel constructs a multi-thread
  Tokio runtime internally, drives the supervisor through
  `Phase::Exec`, and bridges the kernel's sync shutdown signal into
  the supervisor's cancellation token. Without workers, the existing
  Condvar-based wait still serves.
- `WorkerSubsystem` continues as a topo-sort placeholder (the
  supervisor itself lives directly on the `Kernel`, not the
  subsystem) — the subsystem is upgraded to a real implementation in
  Milestone H alongside the shutdown coordinator.
- New optional dependency: `async-trait = "0.1"` (gated on `tokio`).
- Stress test: 200 mixed sync/async workers with mixed criticality,
  restart-never policy, and a fraction that fail or panic. Capped at
  60 seconds wall-clock.
- Prelude re-exports the worker module's consumer-facing types,
  with the Tokio-bound types (`Worker`, `AsyncWorker`,
  `WorkerContext`, `WorkerError`, `WorkerHandle`) gated on the
  `tokio` feature.

### Changed

- `events::WorkerEvent` is no longer the `{ kind: &'static str }`
  placeholder from Milestone C; it now wraps a
  `WorkerLifecycleEvent`. The `WorkerEvent::new` constructor takes
  a `WorkerLifecycleEvent` instead of a static-string `kind`.
- `events::topic::worker_topic` recognizes the full
  `WorkerLifecycleEvent` kind set
  (`started/heartbeat/idle/busy/failed/panicked/restarted/stopping/stopped`).

### Added

- `kernel` module — the consumer-facing builder and the assembled
  runtime that wires lifecycle, events, errors, health, and metrics
  into one composed kernel:
  - `KernelBuilder` — fluent builder with `with_subsystem`,
    `with_error_classifier`, `with_metrics_backend`, and
    `with_shutdown_grace`. `build()` validates the configuration,
    rejects duplicate names, missing dependencies, dependency cycles,
    empty kernel names, and consumer use of reserved built-in names,
    then topologically sorts the subsystem list with Kahn's
    algorithm. Sort is deterministic for a given input — Kahn's
    queue seeds nodes in declaration order.
  - `Subsystem` trait — `name`, `dependencies`, `boot`, `load`,
    `shutdown`, `health`. Default impls cover `dependencies`,
    `load`, `shutdown`, and `health`. Object-safe and
    `Send + Sync + 'static`.
  - Seven kernel built-in subsystems registered automatically by
    `KernelBuilder::build`:
    `LifecycleSubsystem`, `EventSubsystem`, `ErrorSubsystem`,
    `HealthSubsystem`, `MetricsSubsystem`, `WorkerSubsystem`
    (placeholder pending Milestone F), and `ShutdownSubsystem`
    (placeholder pending Milestone H). Their names
    (`lifecycle`, `events`, `errors`, `health`, `metrics`,
    `workers`, `shutdown`) are reserved; consumer subsystems
    declaring them as dependencies sort after the corresponding
    built-in.
  - `Kernel` — the assembled runtime. `boot()` runs each
    subsystem's `boot` then `load` in topological order;
    `run()` adds Exec and blocks the calling thread until
    `shutdown()` is signalled, then runs each subsystem's
    `shutdown` in reverse order; `snapshot()` returns a
    `KernelSnapshot` of lifecycle + health + per-subsystem
    state; `context()` produces a fresh `KernelContext`.
    `Kernel` is `Clone` (cheap `Arc` clone) so a separate
    thread can call `shutdown()` while the main thread is
    blocked in `run()`.
  - `KernelContext` — flat bundle of `Clone`-able handles passed
    to every subsystem method (`boot`, `load`, `shutdown`).
    Direct field access: `ctx.events`, `ctx.errors`, `ctx.health`,
    `ctx.metrics`, `ctx.shutdown`, `ctx.lifecycle`, plus
    `ctx.kernel_name`.
  - `LifecycleHandle`, `ErrorHandle`, `ShutdownHandle` —
    focused, `Clone`-able views into the underlying registries.
    `LifecycleHandle` exposes only read methods (`phase`,
    `state`, `snapshot`); transitions stay with the `Kernel`
    itself. `ErrorHandle` exposes only `classify`. `ShutdownHandle`
    exposes `signal`, `is_signalled`, `wait`, `wait_timeout`.
  - `KernelSnapshot` — read-only view including
    `LifecycleSnapshot`, `HealthSnapshot`, and a per-subsystem
    `SubsystemSnapshot` list with id, dependencies, health, and
    boot/load timestamps. Subsystem identifiers reuse
    `primitives::SubsystemId`.
  - `BuildError` — typed builder failures: `DuplicateSubsystem`,
    `MissingDependency`, `DependencyCycle`, `EmptyName`,
    `ReservedName`. `Display` translates through `lang_lib::t!`
    under `kernel.builder.<variant>` keys.
  - Subsystem panic isolation: every `boot`, `load`, and
    `shutdown` invocation is wrapped in `std::panic::catch_unwind`.
    A panicking subsystem becomes
    `KernelError::Subsystem { name, source: ... }` rather than
    crashing the kernel. Panic payloads are surfaced through the
    error's source.
- `Kernel::run()` blocks via a `Condvar` — adequate for this
  milestone where Exec does no work; replaced with a Tokio-based
  loop in Milestone F.
- Stress test for kernel boot ordering: 100-subsystem graph
  (10 levels × 10 peers, full inter-level dependencies) booted
  and shut down 50 times in a loop, capped at 30 seconds.
- Prelude re-exports the consumer-facing kernel types: `Kernel`,
  `KernelBuilder`, `KernelContext`, `KernelSnapshot`, `Subsystem`,
  and the three new handles (`ErrorHandle`, `LifecycleHandle`,
  `ShutdownHandle`).

### Added

- `health` module with the kernel's per-subsystem health and
  global aggregation:
  - `HealthStatus` — five-level enum (`Healthy / Degraded / Unhealthy /
    Critical / Unknown`) with derived `PartialOrd`/`Ord`. The aggregate
    rule is "worst wins" via `max`; `Unknown` ranks past `Critical` so a
    never-reported subsystem is treated as fail-safe rather than assumed
    healthy. `is_healthy` and `is_actionable` predicates round out the
    type. Default is `Unknown`.
  - `HealthCheck` trait — synchronous, on-demand probe for components
    that compute health on the fly rather than push it. Object-safe and
    `Send + Sync + 'static`.
  - `HealthRegistry` and `HealthHandle` — push-based registry where
    subsystems call `report(name, status)` and the aggregate is
    recomputed eagerly. The aggregate is held in an `AtomicU8`-backed
    cell, so `aggregate()` reads are lock-free. Reports that cross the
    aggregate boundary fan out to event subscribers. Optimization: the
    recompute scans the subsystem map only when the change actually
    moves the aggregate (status > current aggregate, or status drops
    below the previous max from a subsystem that held the max).
  - `HealthSnapshot` — read-only owned view of the registry with
    `count_by_status` and `unhealthy_subsystems` (sorted) helpers.
- `metrics` module with the kernel's pluggable metrics protocol:
  - `MetricsBackend` trait — backend-agnostic counter/gauge/histogram
    interface with primitive arguments (`&str`, `u64`/`f64`, label
    tuples). Object-safe; held behind
    `MetricsHandle = Arc<dyn MetricsBackend>`. Documented no-panic
    contract: backends MUST NOT panic, since the kernel emits from hot
    paths and does not wrap these calls in `catch_unwind`.
  - `NoopMetricsBackend` — default backend that the compiler optimizes
    to nothing. Used when no consumer-supplied backend is wired in.
  - `KernelMetric` + `MetricKind` — typed enum of the kernel's stable
    internal metrics with `name()` and `kind()` accessors. Companion
    `names` module exports the same names as `const &str` so emit sites
    use the literals directly. `kernel_metric_names()` returns the full
    list for admin endpoints and exporters.
  - `MetricsSnapshot` + `MetricValue` — owned snapshot type for a
    future kernel-wide `Kernel::snapshot()` API.
  - `MetricsHandle` type alias for `Arc<dyn MetricsBackend>`.
- `HealthEvent` promoted from a placeholder to a real two-variant
  enum: `AggregateChanged { from, to, at }` and
  `SubsystemChanged { subsystem, from, to, at }`. Topics route to
  `kernel.health.aggregate` and `kernel.health.<subsystem>`.
- `MetricEvent` promoted from a placeholder to a real three-variant
  enum: `Counter { name, value }`, `Gauge { name, value }`,
  `Histogram { name, value }`. Topics route to
  `kernel.metric.<short_name>` (the metric's `kernel.` prefix is
  swapped for `kernel.metric.`).
- `events::topic` updated to recognize the new health and metric
  suffixes plus the kernel's stable subsystem names (`storage`,
  `events`, `errors`, `metrics`, `workers`, `shutdown`,
  `aggregate`).
- `LifecycleController` extended with optional `MetricsHandle`. Three
  new constructors:
  - `with_metrics(metrics)` — events: None, metrics: Some.
  - `with_events_and_metrics(events, metrics)` — both wired.
  - `set_metrics(&mut self, metrics)` — attach metrics post-construction.
  When wired, `transition()` updates the `kernel.lifecycle.phase`
  gauge and increments `kernel.lifecycle.transitions` (labelled by
  destination state) on every successful transition. Failed
  transitions emit nothing.
- Prelude re-exports the consumer-facing health and metrics types:
  `HealthHandle`, `HealthSnapshot`, `HealthStatus`, `MetricsBackend`,
  `MetricsHandle`, `NoopMetricsBackend`. Internal types
  (`HealthCheck`, `KernelMetric`, `MetricKind`, `MetricsSnapshot`)
  remain at their home modules.
- Stress test for `HealthRegistry` covering 32 threads × 100_000
  reports each (3.2M total) into 50 subsystems with a concurrent
  reader thread polling the aggregate every 100µs. Capped at a
  30-second wall-clock.

### Changed

- `events::HealthEvent` and `events::MetricEvent` are now real enums
  rather than the Milestone C `{ kind: &'static str }` placeholder
  structs. Code that constructed the placeholders by struct literal
  (or via the placeholder `new(kind)` constructors) needs to switch
  to the new variants. The `WorkerEvent` placeholder remains pending
  Milestone F.

### Added (continued)

- `errors` module with the kernel's classification spine:
  - `Severity` — six-level enum (`Debug` through `Fatal`) with derived
    `PartialOrd`/`Ord` so callers can compare directly. `Display` is
    internal output (uppercase variant name); user-visible messages
    travel via the error types that own them.
  - `ErrorAction` — nine-variant policy enum (`LogOnly`, `EmitEvent`,
    `MarkWorkerDegraded`, `MarkServiceDegraded`, `RestartWorker`,
    `OpenCircuit`, `EnterReadOnlyMode`, `BeginShutdown`, `AbortProcess`)
    with `is_terminal` predicate for the two shutdown-leaning actions.
  - `Classification` — `{ severity, action, event_topic }` triple
    returned by classifiers. `Copy` for cheap pass-by-value.
  - `ErrorClassifier` trait — maps any `&dyn Error` to a `Classification`.
    Object-safe; held behind `Arc<dyn ErrorClassifier>`. `NoopClassifier`
    ships as the default that returns `Classification::default()` for
    every error.
  - `KernelError` + `KernelErrorCode` — typed kernel error enum with
    stable `KER-NNNNN` codes. `Display` routes through `lang_lib::t!()`
    under the convention `kernel.error.<category>.<numeric_code>`,
    falling back to `"<category>: KER-<padded_code>"` so logs stay
    readable when no locale has been loaded. Implements
    `std::error::Error` with proper `source()` chaining.
  - `ErrorRegistry` — `RwLock`-backed holder for the active classifier,
    swappable at runtime via `set_classifier`. Read path clones an
    `Arc` and drops the lock before invoking the classifier so a slow
    classifier never blocks writers.
- `events` module with the typed event bus:
  - `KernelEvent` — top-level enum (`Lifecycle`, `Worker`, `Error`,
    `Health`, `Metric`, `Custom`). Each variant maps to a stable
    topic via `KernelEvent::topic`. `Worker`, `Health`, and `Metric`
    are placeholder variants; their full payloads land in the owning
    milestones (D and F).
  - `LifecycleEvent::Transition { from, to, at }` — emitted on every
    successful `LifecycleController::transition`.
  - `ErrorEvent`, `WorkerEvent`, `HealthEvent`, `MetricEvent` — typed
    event payloads with `new()` constructors. All `#[non_exhaustive]`
    so future fields land without breaking SemVer.
  - `CustomEvent` — `{ topic: String, payload: Box<dyn Any + Send + Sync> }`
    for consumer-defined events. In-process only; consumers wrap in a
    serializable container at any cross-process boundary.
  - `EventDispatcher` — synchronous topic-keyed dispatcher. `mod-events`
    dispatches by `TypeId`, which does not fit the kernel's
    topic-string subscription model; the dispatcher in this module
    owns its own routing table and mirrors the panic-isolation pattern
    from `mod-events` (`std::panic::catch_unwind` per handler so a
    panicking handler never escapes to the emitter).
  - `EventHandle` — `Clone`able view into the dispatcher with the same
    emit / subscribe / unsubscribe surface, minus `new()`. Subsystems
    and workers receive a handle, never the dispatcher.
  - `SubscriptionId` — opaque newtype returned from `subscribe`,
    accepted by `unsubscribe`.
  - `events::topic` — `fn` topic-string builders that produce stable
    namespace-prefixed event topics (`kernel.lifecycle.*`,
    `kernel.worker.*`, `kernel.error.*`, `kernel.health.*`,
    `kernel.metric.*`, `kernel.custom`). Unknown suffixes fall through
    to a generic `kernel.<category>.unknown` topic.
- `LifecycleController` extended with optional `EventHandle`. Three
  constructors now exist:
  - `LifecycleController::new()` — no event wiring, controller runs
    silent (correct during early bootstrap before the dispatcher exists).
  - `LifecycleController::with_events(handle)` — wires the dispatcher
    handle at construction.
  - `LifecycleController::set_events(&mut self, handle)` — attaches a
    handle after construction (`&mut self` so it can only run before
    the controller is shared across threads).
  When wired, `transition()` emits `KernelEvent::Lifecycle(LifecycleEvent::Transition { from, to, at })`
  on every successful state change. Failed transitions emit nothing.
- Prelude updated to re-export the consumer-facing subset of the new
  types: `Classification`, `ErrorAction`, `ErrorClassifier`,
  `KernelError`, `Severity`, `EventDispatcher`, `EventHandle`,
  `KernelEvent`, `LifecycleEvent`. Internal-leaning types
  (`SubscriptionId`, `KernelErrorCode`, `NoopClassifier`, the
  placeholder event types) remain at their home modules.
- Stress test for `EventDispatcher` covering 1 publisher × 100
  subscribers × 100_000 events on the same topic, capped at a
  30-second wall-clock.
- `primitives` module with the foundational kernel primitives:
  - `Global<T>` — `OnceLock`-backed once-set container with `set` /
    `try_set` (equivalent dual API), a `get_unchecked` fast path that
    panics through `lang_lib::t!()` under the
    `kernel.primitives.global.uninitialized` key, and a `Debug` impl that
    distinguishes initialized from uninitialized state. Lifted from the
    Hive runtime (`hive/sys/global.rs`) and audited: removed unnecessary
    `unsafe` `Send`/`Sync` impls (the bounds come transitively from
    `OnceLock`), routed the panic message through `lang_lib::t!()`, and
    dropped the `init` closure helper that the kernel does not need.
  - `KernelId`, `WorkerId`, `SubsystemId` — strongly-typed newtype IDs
    over `u64`. Each is `Copy + Eq + Hash + Display`, with a
    `from_raw` adapter constructor for tests and external numbering
    schemes. Generated through the shared `IdGenerator` (atomic
    `fetch_add`, lock-free, `const fn` constructible).
  - `Instant`, `Deadline`, `Interval` — kernel-owned time primitives.
    `Instant` wraps `std::time::Instant` so the kernel does not leak
    `std::time` into its public API. `Deadline` exposes `is_expired`
    and `remaining`; `Interval` is a value-typed period (the actual
    ticking lives in `worker::watchdog`, in a later milestone).
- `lifecycle` module with the kernel run-cycle machine:
  - `Phase` — coarse run-cycle stage (`Idle / Boot / Load / Exec /
    Shutdown`). Lifted from `hive/runtime.rs` and audited: marked
    `#[non_exhaustive]`, added `as_str` and `ordinal` accessors, kept
    the existing uppercase `Display` for log/metrics output.
  - `KernelState` — fine-grained runtime status (`Created / Booting /
    Loading / Running / Degraded / Stopping / Stopped / Failed`). Maps
    to `Phase` via `KernelState::phase()`, exposes `is_terminal` and
    `is_running` predicates.
  - `is_legal` and `assert_legal` — pure functions that validate state
    transitions against a constant 8×8 legal-transition matrix. The
    matrix lives once in `transition.rs`; documentation and code cannot
    drift.
  - `TransitionError` — typed error returned from `assert_legal` and
    `LifecycleController::transition` on illegal moves. `Display`
    routes the prefix through `lang_lib::t!()` under
    `kernel.lifecycle.transition.illegal`; the `from`/`to` endpoints
    are appended after the translated text so operators see exactly
    which move was rejected. Implements `std::error::Error`.
  - `LifecycleController` — `RwLock`-backed state holder with `state`,
    `phase`, `snapshot`, and `transition` methods. Transitions are
    serialized through the write lock; reads share a read lock. Lock
    poisoning is recovered transparently via `PoisonError::into_inner`.
    `Send + Sync`. Emits no events at this milestone — event wiring
    lands in Milestone C.
  - `LifecycleSnapshot` — read-only `Copy` view of the controller's
    current state, phase, and last-transition timestamp.
- `prelude` updated to re-export the consumer-facing subset of
  primitives and lifecycle types: `KernelState`, `LifecycleSnapshot`,
  `Phase`, `Deadline`, `Instant`, `KernelId`, `SubsystemId`,
  `WorkerId`. `Global<T>`, `Interval`, and `IdGenerator` are
  intentionally not re-exported — they are internal-leaning and
  consumers pull them from their home modules.
- Stress test for `LifecycleController` covering 16 threads × 25_000
  mixed read/transition operations against a single shared controller,
  capped at a 10-second wall-clock to surface lock-contention bugs.
- `lang-lib` (`1.0`) registered as a mandatory dependency. All kernel-emitted
  user-visible strings will route through `lang_lib::t!()`. Translation
  defaults to identity until a consumer explicitly initializes a locale —
  the kernel never hard-codes user-visible English.
- Expanded non-negotiables in `.dev/PLAN.md` covering: cross-platform
  mandate (Linux/macOS/Windows first-class), enterprise/distributed/clustered
  consumer support, mandatory localization through `lang-lib`, comprehensive
  per-milestone testing (unit + edge + stress + lint + doc + cross-platform
  CI), changelog discipline, and clean-code mandate (SOLID, KISS, DRY, YAGNI,
  human-written-looking docs).
- Editor constraints expanded with: cross-platform requirement (constraint
  #12), localization routing (#13), CI-green close on every milestone (#14),
  and the "audit-and-enhance" rule for code lifted from Hive or fsys.
- Milestones explicitly framed as pauses in development, not version
  boundaries — the crate stays at `0.1.0` until the full feature set ships.
- `.dev/milestones/` directory with per-milestone work orders (B through I).

## [0.1.0] - 2026-05-05

### Added

- Initial scaffolding and name reservation.
- Repository layout, license dual-grant (MIT OR Apache-2.0),
  workspace-aligned `rustfmt.toml`, and the canonical `REPS.md`
  governance document.
- `Cargo.toml` declares the planned feature surface:
  - `default` — lean core, no async runtime, no integrations.
  - `tokio` — async worker supervision, watchdog, drain.
  - `daemon` — `proc-daemon`-backed host layer.
  - `hardware` — `fsys` re-export for hardware probing.
  - `errors` — default `error-forge`-backed classifier.
  - `metrics` — default `metrics-lib`-backed backend.
- `mod-events` (`0.9`) registered as the only mandatory dep — the
  typed event bus is foundational and present in every build.
- Crate root with the project's standard `#![deny]` policy
  (REPS-aligned warnings, missing docs, panicking macros, untyped
  unsafe).
- Empty `prelude` module ready to populate as modules land.
- `.dev/PROMPT.md`, `.dev/DIRECTIVES.md`, and `.dev/PLAN.md`
  carrying the project scope, build process, AI-collaboration
  guard rails, and the full architectural roadmap.

[Unreleased]: https://github.com/jamesgober/service-kernel/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/jamesgober/service-kernel/releases/tag/v0.1.0
