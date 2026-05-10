//! Typed event bus on top of [`mod_events`](https://crates.io/crates/mod-events).
//!
//! `mod-events` dispatches by Rust `TypeId`; the kernel needs
//! topic-string routing so consumers can subscribe to
//! `kernel.lifecycle.*`, `kernel.worker.<id>.*`, and consumer-defined
//! topics without having to know the concrete `KernelEvent` variant.
//! This module bridges those models: every kernel event flows through
//! a single typed channel, then fans out to topic-keyed handlers.
//!
//! Handlers are panic-isolated — a panicking handler does not prevent
//! other handlers on the same topic from running, and never escapes
//! to the emitter. This mirrors the panic-safety guarantee
//! `mod-events` provides on its own dispatch path.
//!
//! Asynchronous handlers and event delivery on a Tokio runtime are
//! deferred to the `tokio` feature work in Milestone F. At this
//! milestone, all subscribers run synchronously on the emitter's
//! thread.

pub mod dispatcher;
pub mod kernel_event;
pub mod topic;

pub use dispatcher::{EventDispatcher, EventHandle, SubscriptionId};
pub use kernel_event::{
    CustomEvent, ErrorEvent, HealthEvent, KernelEvent, LifecycleEvent, MetricEvent, WorkerEvent,
};
