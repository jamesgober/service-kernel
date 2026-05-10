//! Worker lifecycle events.
//!
//! [`WorkerLifecycleEvent`] is the payload carried by
//! [`KernelEvent::Worker`](crate::events::KernelEvent::Worker) once
//! Milestone F's promotion is in place.

use std::time::Duration;

use crate::primitives::{Instant, WorkerId};

use super::PanicReason;

/// Lifecycle events emitted by the supervisor for a single worker.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum WorkerLifecycleEvent {
    /// Worker spawned and started running.
    Started {
        /// Worker identifier.
        id: WorkerId,
        /// Worker name.
        name: &'static str,
        /// When the start happened.
        at: Instant,
    },
    /// Worker emitted a heartbeat tick (Milestone G).
    Heartbeat {
        /// Worker identifier.
        id: WorkerId,
        /// When the heartbeat was observed.
        at: Instant,
    },
    /// Worker transitioned to idle (Milestone G).
    BecameIdle {
        /// Worker identifier.
        id: WorkerId,
        /// When the transition happened.
        at: Instant,
    },
    /// Worker transitioned to busy (Milestone G).
    BecameBusy {
        /// Worker identifier.
        id: WorkerId,
        /// When the transition happened.
        at: Instant,
    },
    /// Worker returned an error from `run`.
    Failed {
        /// Worker identifier.
        id: WorkerId,
        /// Worker name.
        name: &'static str,
        /// Operator-readable failure description.
        reason: String,
        /// When the failure was observed.
        at: Instant,
    },
    /// Worker panicked inside `run`.
    Panicked {
        /// Worker identifier.
        id: WorkerId,
        /// Worker name.
        name: &'static str,
        /// Normalized panic payload.
        reason: PanicReason,
        /// When the panic was observed.
        at: Instant,
    },
    /// Worker re-spawned after a failure.
    Restarted {
        /// Worker identifier.
        id: WorkerId,
        /// Worker name.
        name: &'static str,
        /// 1-based attempt counter (the attempt that just started).
        attempt: u32,
        /// When the restart happened.
        at: Instant,
    },
    /// Worker is shutting down per cancellation.
    Stopping {
        /// Worker identifier.
        id: WorkerId,
        /// When stopping started.
        at: Instant,
    },
    /// Worker has fully stopped.
    Stopped {
        /// Worker identifier.
        id: WorkerId,
        /// Worker name.
        name: &'static str,
        /// When stopping completed.
        at: Instant,
    },
    /// Worker's circuit breaker tripped to Open.
    CircuitOpened {
        /// Worker identifier.
        id: WorkerId,
        /// Worker name.
        name: &'static str,
        /// Failure count that tripped the breaker.
        failures: u32,
        /// When the breaker opened.
        at: Instant,
    },
    /// Worker's circuit breaker advanced to HalfOpen for a trial.
    CircuitHalfOpened {
        /// Worker identifier.
        id: WorkerId,
        /// Worker name.
        name: &'static str,
        /// When HalfOpen began.
        at: Instant,
    },
    /// Worker's circuit breaker closed after a successful trial.
    CircuitClosed {
        /// Worker identifier.
        id: WorkerId,
        /// Worker name.
        name: &'static str,
        /// When the breaker closed.
        at: Instant,
    },
    /// Worker missed its heartbeat budget; reported as a timeout.
    Timeout {
        /// Worker identifier.
        id: WorkerId,
        /// Worker name.
        name: &'static str,
        /// How long the worker was silent before the watchdog
        /// reported the timeout.
        silent_for: Duration,
        /// When the timeout was observed.
        at: Instant,
    },
}

impl WorkerLifecycleEvent {
    /// Returns a stable lowercase suffix used for topic routing.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            WorkerLifecycleEvent::Started { .. } => "started",
            WorkerLifecycleEvent::Heartbeat { .. } => "heartbeat",
            WorkerLifecycleEvent::BecameIdle { .. } => "idle",
            WorkerLifecycleEvent::BecameBusy { .. } => "busy",
            WorkerLifecycleEvent::Failed { .. } => "failed",
            WorkerLifecycleEvent::Panicked { .. } => "panicked",
            WorkerLifecycleEvent::Restarted { .. } => "restarted",
            WorkerLifecycleEvent::Stopping { .. } => "stopping",
            WorkerLifecycleEvent::Stopped { .. } => "stopped",
            WorkerLifecycleEvent::CircuitOpened { .. } => "circuit_opened",
            WorkerLifecycleEvent::CircuitHalfOpened { .. } => "circuit_half_opened",
            WorkerLifecycleEvent::CircuitClosed { .. } => "circuit_closed",
            WorkerLifecycleEvent::Timeout { .. } => "timeout",
        }
    }

    /// Returns the worker identifier carried by every variant.
    #[must_use]
    pub const fn worker_id(&self) -> WorkerId {
        match self {
            WorkerLifecycleEvent::Started { id, .. }
            | WorkerLifecycleEvent::Heartbeat { id, .. }
            | WorkerLifecycleEvent::BecameIdle { id, .. }
            | WorkerLifecycleEvent::BecameBusy { id, .. }
            | WorkerLifecycleEvent::Failed { id, .. }
            | WorkerLifecycleEvent::Panicked { id, .. }
            | WorkerLifecycleEvent::Restarted { id, .. }
            | WorkerLifecycleEvent::Stopping { id, .. }
            | WorkerLifecycleEvent::Stopped { id, .. }
            | WorkerLifecycleEvent::CircuitOpened { id, .. }
            | WorkerLifecycleEvent::CircuitHalfOpened { id, .. }
            | WorkerLifecycleEvent::CircuitClosed { id, .. }
            | WorkerLifecycleEvent::Timeout { id, .. } => *id,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::primitives::IdGenerator;

    #[test]
    fn test_kind_names_are_unique() {
        let id_gen = IdGenerator::new();
        let id = id_gen.next_worker_id();
        let now = Instant::now();
        let events = [
            WorkerLifecycleEvent::Started {
                id,
                name: "n",
                at: now,
            }
            .kind(),
            WorkerLifecycleEvent::Heartbeat { id, at: now }.kind(),
            WorkerLifecycleEvent::BecameIdle { id, at: now }.kind(),
            WorkerLifecycleEvent::BecameBusy { id, at: now }.kind(),
            WorkerLifecycleEvent::Failed {
                id,
                name: "n",
                reason: "boom".to_owned(),
                at: now,
            }
            .kind(),
            WorkerLifecycleEvent::Panicked {
                id,
                name: "n",
                reason: PanicReason::StaticStr("boom"),
                at: now,
            }
            .kind(),
            WorkerLifecycleEvent::Restarted {
                id,
                name: "n",
                attempt: 1,
                at: now,
            }
            .kind(),
            WorkerLifecycleEvent::Stopping { id, at: now }.kind(),
            WorkerLifecycleEvent::Stopped {
                id,
                name: "n",
                at: now,
            }
            .kind(),
            WorkerLifecycleEvent::CircuitOpened {
                id,
                name: "n",
                failures: 3,
                at: now,
            }
            .kind(),
            WorkerLifecycleEvent::CircuitHalfOpened {
                id,
                name: "n",
                at: now,
            }
            .kind(),
            WorkerLifecycleEvent::CircuitClosed {
                id,
                name: "n",
                at: now,
            }
            .kind(),
            WorkerLifecycleEvent::Timeout {
                id,
                name: "n",
                silent_for: Duration::from_secs(1),
                at: now,
            }
            .kind(),
        ];
        let mut set = std::collections::HashSet::new();
        for k in events {
            assert!(set.insert(k), "duplicate kind: {}", k);
        }
    }

    #[test]
    fn test_worker_id_round_trips() {
        let id_gen = IdGenerator::new();
        let id = id_gen.next_worker_id();
        let event = WorkerLifecycleEvent::Started {
            id,
            name: "n",
            at: Instant::now(),
        };
        assert_eq!(event.worker_id(), id);
    }
}
