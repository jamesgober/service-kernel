<h1 align="center">
  <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
  <br>
  <code>SERVICE KERNEL</code>
  <br>
  <sub>RUNTIME CONTROL FOR RUST SERVICES
</h1>
<p align="center">
  <strong>Lifecycle. Workers. Events. Errors. Health. Metrics. Shutdown.</strong>
</p>
<p align="center">
  <a href="https://crates.io/crates/service-kernel" alt="service-kernel on Crates.io"><img alt="Crates.io" src="https://img.shields.io/crates/v/service-kernel"></a>
  <a href="https://crates.io/crates/service-kernel" alt="Download"><img alt="Crates.io Downloads" src="https://img.shields.io/crates/d/service-kernel?color=%230099ff"></a>
  <a href="https://docs.rs/service-kernel" title="service-kernel documentation"><img alt="docs.rs" src="https://img.shields.io/docsrs/service-kernel"></a>
  <img alt="License" src="https://img.shields.io/crates/l/service-kernel">
  <img alt="MSRV" src="https://img.shields.io/badge/rustc-1.75%2B-blue.svg?style=flat-square">
</p>

**`service-kernel`** is a runtime control plane for resilient Rust services.
It provides the shared backbone every long-running Rust program ends up
rebuilding from scratch: lifecycle phase management, supervised workers,
typed event routing, error policy, health and metrics registries, and
graceful shutdown coordination.

It is the layer **above** Tokio and **below** your service. It does not
replace Tokio. It does not own business logic. It coordinates the runtime
concerns so the consumer can focus on what its service actually does.


---

> ⚠️ **Status:** Active development toward initial public release `0.1.0`.
> The crate compiles, but no runtime modules have shipped yet.
> The public API is **not yet stable** and will change across the
> `0.1.0` development cycle.
> Do not depend on this from production code yet.

&nbsp;


## What `service-kernel` provides

The crate will grow into the following layers. Nothing below scaffolding
exists in code yet — see the [planned layers](#planned-layers) section
for the full roadmap.

- **Lifecycle** &mdash; explicit phase machine (`Idle → Boot → Load → Exec → Shutdown`) with documented transition rules.
- **Subsystem loader** &mdash; boots config, logging, errors, metrics, and consumer-registered subsystems in declared order. Holds the assembled state for the running process.
- **Typed event bus** &mdash; built on top of [`mod-events`](https://crates.io/crates/mod-events). Defines `KernelEvent`, `WorkerEvent`, `ErrorEvent`, `HealthEvent`, plus a `Custom` variant for consumer events.
- **Worker supervision** &mdash; trait-based worker abstraction with restart policy, criticality levels (`Critical` / `Essential` / `Optional` / `Background`), panic boundaries, watchdog timeout detection, and circuit-breaker containment. Gated behind the `tokio` feature.
- **Error policy** &mdash; `ErrorClassifier` trait, severity vocabulary, and an action policy that maps errors to runtime behavior (`LogOnly`, `MarkDegraded`, `RestartWorker`, `OpenCircuit`, `BeginShutdown`, `AbortProcess`). Default impl over `error-forge` is gated behind the `errors` feature.
- **Health registry** &mdash; per-subsystem health states (`Healthy` / `Degraded` / `Unhealthy` / `Critical` / `Unknown`) aggregated into a global view.
- **Metrics protocol** &mdash; `MetricsBackend` trait kernel emits into. Default impl over `metrics-lib` is gated behind the `metrics` feature.
- **Shutdown coordinator** &mdash; drain coordinator with configurable grace-period semantics. Lifted from a production runtime that ships in [hive-system](https://github.com/jamesgober).

&nbsp;

## Why this exists

Every serious Rust service ends up writing the same scaffolding by hand:

```text
load config
initialise logging and metrics
start workers
catch panics
detect hung workers
route errors through severity / policy
track per-subsystem health
emit metrics
drain in-flight work on shutdown
```

Doing all of that well is hard. Doing it again per-project is wasteful,
and the partial reimplementations end up subtly different in the failure
modes that matter most.

`service-kernel` is the generic extraction of a battle-tested runtime
that already powers a production database. The kernel coordinates; the
consumer owns business logic.

```text
        Tokio
          │
   service-kernel
          │
  consumer (database, daemon, queue worker, app server, ...)
```

&nbsp;

## What `service-kernel` is NOT

- Not a Tokio replacement. The kernel runs on Tokio.
- Not a daemon framework by default. The `daemon` feature opts into `proc-daemon`-backed hosting.
- Not a CLI framework. Command dispatch is a consumer concern.
- Not a web framework, not a database, not a query engine, not a job queue. Those are consumers.
- Not a plugin system. Subsystems are explicit and known at compile time.
- Not a distributed coordinator. One service, one process, one kernel instance.

&nbsp;

## Cargo features

| Feature   | Purpose                                                    | Implies   |
|-----------|------------------------------------------------------------|-----------|
| `default` | Lean core: lifecycle, events, errors, health, shutdown.    | &mdash;   |
| `tokio`   | Async worker supervision, watchdog, drain coordinator.     | &mdash;   |
| `daemon`  | `proc-daemon`-backed host layer for detached processes.    | `tokio`   |
| `hardware`| `fsys` re-export for drive / NVMe / page-size probing.     | &mdash;   |
| `errors`  | Default `error-forge`-backed `ErrorClassifier` impl.       | &mdash;   |
| `metrics` | Default `metrics-lib`-backed `MetricsBackend` impl.        | &mdash;   |

&nbsp;

## Planned layers

The crate will land iteratively. Order:

1. **`lifecycle`** &mdash; phase machine + transition rules + snapshot.
2. **`kernel`** &mdash; subsystem loader, boot orchestration.
3. **`events`** &mdash; typed event bus on top of `mod-events`.
4. **`errors`** &mdash; classifier trait, severity vocabulary, action policy.
5. **`health`** &mdash; status enum, per-subsystem registry, global aggregate.
6. **`metrics`** &mdash; backend trait + kernel-defined counters.
7. **`worker`** &mdash; trait + supervisor, restart policy, panic boundary, watchdog. Gated behind `tokio`.
8. **`shutdown`** &mdash; drain coordinator with grace-period semantics.
9. **`host`** &mdash; daemon process layer. Gated behind `daemon`.

&nbsp;

## License

Dual-licensed under your choice of:

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

&nbsp;

## Contributing

This crate is in early development and the public API is not yet stable. Issues and discussion are welcome; PRs against runtime modules will land more reliably once the `0.1.0` initial release ships.

&nbsp;

---

&nbsp;

<p align="center">
  <strong>Author:</strong> James Gober &mdash; <a href="https://github.com/jamesgober">@jamesgober</a>
</p>
