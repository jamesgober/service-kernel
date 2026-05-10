//! Typed event payloads carried over the kernel's event bus.
//!
//! [`KernelEvent`] is the top-level enum delivered to subscribers.
//! Each variant carries a strongly-typed sub-event:
//!
//! - [`LifecycleEvent`] — emitted on every successful
//!   [`LifecycleController`](crate::lifecycle::LifecycleController) transition.
//! - [`WorkerEvent`] — placeholder; the full type lands in
//!   Milestone F when the supervisor ships.
//! - [`ErrorEvent`] — emitted by the error registry when classification
//!   produces an `EmitEvent` action (or any classification that the
//!   kernel routes to subscribers).
//! - [`HealthEvent`] — placeholder; the full type lands in Milestone D.
//! - [`MetricEvent`] — placeholder; the full type lands in Milestone D.
//! - [`CustomEvent`] — opaque payload for consumer-defined events.
//!   See the [`CustomEvent`] documentation for the cross-process
//!   serialization caveat.

use std::any::Any;
use std::fmt;
use std::time::Duration;

use crate::errors::{ErrorAction, Severity};
use crate::health::HealthStatus;
use crate::lifecycle::KernelState;
use crate::primitives::Instant;
use crate::worker::WorkerLifecycleEvent;

use super::topic;

/// Top-level event type carried by the kernel's event bus.
///
/// Every variant has a stable topic string returned by
/// [`KernelEvent::topic`]; subscribers register against that topic
/// and receive only matching events.
///
/// Marked `#[non_exhaustive]` so future variants land without
/// breaking SemVer.
#[non_exhaustive]
#[derive(Debug)]
pub enum KernelEvent {
    /// Lifecycle state change.
    Lifecycle(LifecycleEvent),
    /// Worker event (placeholder; expanded in Milestone F).
    Worker(WorkerEvent),
    /// Classified error event.
    Error(ErrorEvent),
    /// Health state change (placeholder; expanded in Milestone D).
    Health(HealthEvent),
    /// Metric update (placeholder; expanded in Milestone D).
    Metric(MetricEvent),
    /// Consumer-defined custom event.
    Custom(CustomEvent),
}

impl KernelEvent {
    /// Returns the topic string for this event.
    ///
    /// Routes through [`super::topic`] builders. The returned string
    /// is `&'static`; subscribers can compare it by pointer or value.
    #[inline]
    #[must_use]
    pub fn topic(&self) -> &'static str {
        match self {
            KernelEvent::Lifecycle(LifecycleEvent::Transition { to, .. }) => {
                topic::lifecycle_topic(to.as_str())
            }
            KernelEvent::Lifecycle(LifecycleEvent::ShutdownStarted { .. }) => {
                "kernel.lifecycle.shutdown_started"
            }
            KernelEvent::Lifecycle(LifecycleEvent::ShutdownCompleted { .. }) => {
                "kernel.lifecycle.shutdown_completed"
            }
            KernelEvent::Worker(event) => topic::worker_topic(event.event.kind()),
            KernelEvent::Error(event) => topic::error_topic(event.severity.as_str()),
            KernelEvent::Health(event) => match event {
                HealthEvent::AggregateChanged { .. } => topic::health_topic("aggregate"),
                HealthEvent::SubsystemChanged { subsystem, .. } => topic::health_topic(subsystem),
            },
            KernelEvent::Metric(event) => match event {
                MetricEvent::Counter { name, .. }
                | MetricEvent::Gauge { name, .. }
                | MetricEvent::Histogram { name, .. } => topic::metric_topic(name),
            },
            KernelEvent::Custom(_) => topic::CUSTOM_EVENT_ROOT_TOPIC,
        }
    }
}

/// Lifecycle state change events.
///
/// Emitted by [`LifecycleController`](crate::lifecycle::LifecycleController)
/// (`Transition`) and, when the `tokio` feature is enabled, the
/// shutdown coordinator (`ShutdownStarted` / `ShutdownCompleted`).
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub enum LifecycleEvent {
    /// A successful state transition occurred.
    Transition {
        /// State the kernel was in.
        from: KernelState,
        /// State the kernel moved to.
        to: KernelState,
        /// Wall-clock instant of the transition.
        at: Instant,
    },
    /// The shutdown coordinator started its sequence.
    ShutdownStarted {
        /// Wall-clock instant the sequence began.
        at: Instant,
    },
    /// The shutdown coordinator finished its sequence.
    ///
    /// Carries summary counts. With the `tokio` feature enabled, the
    /// full `ShutdownReport` is returned from the shutdown
    /// coordinator's `shutdown` method.
    ShutdownCompleted {
        /// Total time the shutdown sequence took.
        duration: Duration,
        /// Workers that drained cleanly within the grace period.
        workers_drained: usize,
        /// Workers aborted because the grace period expired.
        workers_aborted: usize,
        /// Wall-clock instant the sequence completed.
        at: Instant,
    },
}

/// Worker lifecycle event payload.
///
/// Carries a [`WorkerLifecycleEvent`] (defined in
/// [`crate::worker::event`]) so the supervisor's full vocabulary of
/// per-worker transitions reaches subscribers.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct WorkerEvent {
    /// Lifecycle event observed by the supervisor.
    pub event: WorkerLifecycleEvent,
}

impl WorkerEvent {
    /// Constructs a worker event from a [`WorkerLifecycleEvent`].
    #[inline]
    #[must_use]
    pub fn new(event: WorkerLifecycleEvent) -> Self {
        Self { event }
    }
}

/// Classified error event.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ErrorEvent {
    /// Severity of the classified error.
    pub severity: Severity,
    /// Recommended action returned by the classifier.
    pub action: ErrorAction,
    /// Operator-readable error message.
    pub message: String,
}

impl ErrorEvent {
    /// Constructs an error event with the given severity, action,
    /// and operator-readable message.
    #[inline]
    #[must_use]
    pub fn new(severity: Severity, action: ErrorAction, message: impl Into<String>) -> Self {
        Self {
            severity,
            action,
            message: message.into(),
        }
    }
}

/// Health state-change events.
///
/// Emitted by [`HealthRegistry`](crate::health::HealthRegistry) when
/// a subsystem's status changes or when the global aggregate moves.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub enum HealthEvent {
    /// The aggregate (worst-of-all-subsystems) status moved.
    AggregateChanged {
        /// Aggregate before the change.
        from: HealthStatus,
        /// Aggregate after the change.
        to: HealthStatus,
        /// Wall-clock instant of the change.
        at: Instant,
    },
    /// An individual subsystem's status moved.
    SubsystemChanged {
        /// Stable subsystem name.
        subsystem: &'static str,
        /// Status before the change.
        from: HealthStatus,
        /// Status after the change.
        to: HealthStatus,
        /// Wall-clock instant of the change.
        at: Instant,
    },
}

/// Metric-update event.
///
/// Carries one observation of a kernel metric. The metric's `name` is
/// also used as the topic suffix so subscribers can listen for
/// specific metrics.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub enum MetricEvent {
    /// Monotonic counter increment.
    Counter {
        /// Metric name (e.g. `kernel.lifecycle.transitions`).
        name: &'static str,
        /// Increment amount.
        value: u64,
    },
    /// Gauge update — replaces the previous value.
    Gauge {
        /// Metric name (e.g. `kernel.lifecycle.phase`).
        name: &'static str,
        /// New gauge value.
        value: f64,
    },
    /// Histogram observation.
    Histogram {
        /// Metric name (e.g. `kernel.shutdown.duration`).
        name: &'static str,
        /// Observation value.
        value: f64,
    },
}

/// Consumer-defined event.
///
/// `CustomEvent` carries an arbitrary `Box<dyn Any + Send + Sync>`
/// payload. Subscribers downcast the payload via
/// [`CustomEvent::downcast_ref`].
///
/// The payload is **in-process only**. `Box<dyn Any>` does not
/// serialize. Consumers that ship events across process or network
/// boundaries (replication, message bus exporters) wrap their event
/// in a serializable container at the boundary; the kernel does not
/// impose that cost on every event.
pub struct CustomEvent {
    /// Topic string. Consumers pick a namespace they own.
    pub topic: String,
    /// Type-erased payload.
    pub payload: Box<dyn Any + Send + Sync + 'static>,
}

impl CustomEvent {
    /// Constructs a `CustomEvent` with the given topic and payload.
    #[inline]
    #[must_use]
    pub fn new<T>(topic: impl Into<String>, payload: T) -> Self
    where
        T: Any + Send + Sync + 'static,
    {
        Self {
            topic: topic.into(),
            payload: Box::new(payload),
        }
    }

    /// Returns a reference to the payload as `&T` if it has the
    /// expected type, otherwise `None`.
    #[inline]
    #[must_use]
    pub fn downcast_ref<T: Any + Send + Sync + 'static>(&self) -> Option<&T> {
        self.payload.downcast_ref::<T>()
    }
}

impl fmt::Debug for CustomEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CustomEvent")
            .field("topic", &self.topic)
            .field("payload_type", &(*self.payload).type_id())
            .finish()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn test_kernel_event_is_send_sync_debug() {
        assert_send_sync::<KernelEvent>();
        assert_send_sync::<LifecycleEvent>();
        assert_send_sync::<WorkerEvent>();
        assert_send_sync::<ErrorEvent>();
        assert_send_sync::<HealthEvent>();
        assert_send_sync::<MetricEvent>();
        assert_send_sync::<CustomEvent>();
    }

    #[test]
    fn test_health_event_carries_status_fields() {
        let now = Instant::now();
        let event = HealthEvent::SubsystemChanged {
            subsystem: "storage",
            from: HealthStatus::Healthy,
            to: HealthStatus::Degraded,
            at: now,
        };
        match event {
            HealthEvent::SubsystemChanged {
                subsystem,
                from,
                to,
                at,
            } => {
                assert_eq!(subsystem, "storage");
                assert_eq!(from, HealthStatus::Healthy);
                assert_eq!(to, HealthStatus::Degraded);
                assert_eq!(at, now);
            }
            HealthEvent::AggregateChanged { .. } => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_metric_event_carries_name_value() {
        match (
            MetricEvent::Counter {
                name: "kernel.lifecycle.transitions",
                value: 5,
            },
            MetricEvent::Gauge {
                name: "kernel.lifecycle.phase",
                value: 2.0,
            },
        ) {
            (
                MetricEvent::Counter {
                    name: c_name,
                    value: c_value,
                },
                MetricEvent::Gauge {
                    name: g_name,
                    value: g_value,
                },
            ) => {
                assert_eq!(c_name, "kernel.lifecycle.transitions");
                assert_eq!(c_value, 5);
                assert_eq!(g_name, "kernel.lifecycle.phase");
                assert!((g_value - 2.0).abs() < f64::EPSILON);
            }
            _ => panic!("unreachable"),
        }
    }

    #[test]
    fn test_lifecycle_topic_routing() {
        let event = KernelEvent::Lifecycle(LifecycleEvent::Transition {
            from: KernelState::Booting,
            to: KernelState::Running,
            at: Instant::now(),
        });
        assert_eq!(event.topic(), "kernel.lifecycle.running");
    }

    #[test]
    fn test_error_topic_routing() {
        let event = KernelEvent::Error(ErrorEvent {
            severity: Severity::Critical,
            action: ErrorAction::BeginShutdown,
            message: "boom".to_owned(),
        });
        assert_eq!(event.topic(), "kernel.error.critical");
    }

    #[test]
    fn test_worker_topic_routing() {
        use crate::primitives::WorkerId;
        let event = KernelEvent::Worker(WorkerEvent::new(WorkerLifecycleEvent::Started {
            id: WorkerId::from_raw(1),
            name: "test",
            at: Instant::now(),
        }));
        assert_eq!(event.topic(), "kernel.worker.started");
    }

    #[test]
    fn test_health_aggregate_topic_routing() {
        let event = KernelEvent::Health(HealthEvent::AggregateChanged {
            from: HealthStatus::Healthy,
            to: HealthStatus::Degraded,
            at: Instant::now(),
        });
        assert_eq!(event.topic(), "kernel.health.aggregate");
    }

    #[test]
    fn test_health_subsystem_topic_routing() {
        let event = KernelEvent::Health(HealthEvent::SubsystemChanged {
            subsystem: "storage",
            from: HealthStatus::Healthy,
            to: HealthStatus::Critical,
            at: Instant::now(),
        });
        assert_eq!(event.topic(), "kernel.health.storage");
    }

    #[test]
    fn test_metric_counter_topic_routing() {
        let event = KernelEvent::Metric(MetricEvent::Counter {
            name: "kernel.lifecycle.transitions",
            value: 1,
        });
        assert_eq!(event.topic(), "kernel.metric.lifecycle.transitions");
    }

    #[test]
    fn test_metric_gauge_topic_routing() {
        let event = KernelEvent::Metric(MetricEvent::Gauge {
            name: "kernel.lifecycle.phase",
            value: 3.0,
        });
        assert_eq!(event.topic(), "kernel.metric.lifecycle.phase");
    }

    #[test]
    fn test_custom_event_routing_uses_root_topic() {
        let event = KernelEvent::Custom(CustomEvent::new("myapp.foo", 42_u32));
        assert_eq!(event.topic(), "kernel.custom");
    }

    #[test]
    fn test_custom_event_payload_round_trips() {
        #[derive(Debug, PartialEq)]
        struct Payload {
            x: u32,
        }

        let event = CustomEvent::new("myapp.payload", Payload { x: 7 });
        let downcast = event.downcast_ref::<Payload>().unwrap();
        assert_eq!(downcast.x, 7);
        assert_eq!(event.topic, "myapp.payload");

        // Wrong-type downcast returns None.
        assert!(event.downcast_ref::<u32>().is_none());
    }

    #[test]
    fn test_lifecycle_event_round_trip_via_match() {
        let now = Instant::now();
        let event = KernelEvent::Lifecycle(LifecycleEvent::Transition {
            from: KernelState::Created,
            to: KernelState::Booting,
            at: now,
        });

        match event {
            KernelEvent::Lifecycle(LifecycleEvent::Transition { from, to, at }) => {
                assert_eq!(from, KernelState::Created);
                assert_eq!(to, KernelState::Booting);
                assert_eq!(at, now);
            }
            _ => panic!("expected lifecycle transition"),
        }
    }
}
