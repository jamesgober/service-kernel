//! Owner of the kernel's current lifecycle state.
//!
//! [`LifecycleController`] holds the [`KernelState`] under an
//! `RwLock` and serializes [`transition`](LifecycleController::transition)
//! writes through that lock. It exposes read-only views via
//! [`state`](LifecycleController::state),
//! [`phase`](LifecycleController::phase), and
//! [`snapshot`](LifecycleController::snapshot).
//!
//! When wired to an [`EventHandle`], the controller emits a
//! [`LifecycleEvent::Transition`] on every successful transition.
//! When wired to a [`MetricsHandle`], the controller updates the
//! `kernel.lifecycle.phase` gauge and increments the
//! `kernel.lifecycle.transitions` counter on every successful
//! transition. Both are independent; either, both, or neither may be
//! attached.

use std::fmt;
use std::sync::RwLock;

use crate::events::{EventHandle, KernelEvent, LifecycleEvent};
use crate::metrics::{names as metric_names, MetricsHandle};
use crate::primitives::Instant;

use super::transition::{assert_legal, TransitionError};
use super::{KernelState, Phase};

/// Holder for the kernel's lifecycle state.
///
/// Reads return cheap copies of the state and phase. Writes go
/// through [`transition`](Self::transition), which validates the
/// proposed move against the legal-transition table before applying
/// it and refuses illegal moves with [`TransitionError`].
///
/// `LifecycleController` is `Send + Sync` and intended to be shared
/// behind an `Arc`. The controller's internal lock poisons under
/// panic, but the controller recovers transparently — a poisoned
/// lock still yields its inner value rather than propagating the
/// panic.
pub struct LifecycleController {
    inner: RwLock<Inner>,
    events: Option<EventHandle>,
    metrics: Option<MetricsHandle>,
}

struct Inner {
    state: KernelState,
    last_transition: Instant,
}

/// Read-only view of the controller's current state.
///
/// Includes the [`KernelState`], its containing [`Phase`], and the
/// [`Instant`] of the most recent transition. Snapshots are cheap
/// to clone and safe to pass around.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct LifecycleSnapshot {
    /// The fine-grained kernel state at snapshot time.
    pub state: KernelState,
    /// The coarse phase containing `state`.
    pub phase: Phase,
    /// Wall-clock instant of the most recent transition (or of
    /// the controller's construction, if no transition has yet
    /// occurred).
    pub last_transition: Instant,
}

impl LifecycleController {
    /// Constructs a new controller in [`KernelState::Created`] with
    /// no event wiring.
    ///
    /// `last_transition` is set to `Instant::now()` at construction.
    /// Use [`with_events`](Self::with_events) when you have an
    /// [`EventHandle`] available at construction time, or
    /// [`set_events`](Self::set_events) to attach one before the
    /// controller is shared across threads.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::fresh(None, None)
    }

    /// Constructs a controller wired to the given [`EventHandle`].
    ///
    /// Successful transitions will emit
    /// [`LifecycleEvent::Transition`].
    #[inline]
    #[must_use]
    pub fn with_events(events: EventHandle) -> Self {
        Self::fresh(Some(events), None)
    }

    /// Constructs a controller wired to the given [`MetricsHandle`].
    ///
    /// Successful transitions will update the
    /// `kernel.lifecycle.phase` gauge and increment the
    /// `kernel.lifecycle.transitions` counter.
    #[inline]
    #[must_use]
    pub fn with_metrics(metrics: MetricsHandle) -> Self {
        Self::fresh(None, Some(metrics))
    }

    /// Constructs a controller wired to both event and metric
    /// emitters in a single call.
    #[inline]
    #[must_use]
    pub fn with_events_and_metrics(events: EventHandle, metrics: MetricsHandle) -> Self {
        Self::fresh(Some(events), Some(metrics))
    }

    fn fresh(events: Option<EventHandle>, metrics: Option<MetricsHandle>) -> Self {
        Self {
            inner: RwLock::new(Inner {
                state: KernelState::Created,
                last_transition: Instant::now(),
            }),
            events,
            metrics,
        }
    }

    /// Attaches (or replaces) the controller's event handle.
    ///
    /// Takes `&mut self` so it can only be called while the
    /// controller is still uniquely owned — typically during
    /// bootstrap, before the controller is wrapped in `Arc` and
    /// handed to subsystems.
    pub fn set_events(&mut self, events: EventHandle) {
        self.events = Some(events);
    }

    /// Attaches (or replaces) the controller's metrics handle.
    ///
    /// Takes `&mut self` for the same reason as
    /// [`set_events`](Self::set_events).
    pub fn set_metrics(&mut self, metrics: MetricsHandle) {
        self.metrics = Some(metrics);
    }

    /// Returns the current [`KernelState`].
    #[inline]
    #[must_use]
    pub fn state(&self) -> KernelState {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.state
    }

    /// Returns the current [`Phase`].
    #[inline]
    #[must_use]
    pub fn phase(&self) -> Phase {
        self.state().phase()
    }

    /// Returns a [`LifecycleSnapshot`] of the current state.
    #[inline]
    #[must_use]
    pub fn snapshot(&self) -> LifecycleSnapshot {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        LifecycleSnapshot {
            state: guard.state,
            phase: guard.state.phase(),
            last_transition: guard.last_transition,
        }
    }

    /// Attempts to transition to `to`.
    ///
    /// On success, the controller's state is updated and
    /// `last_transition` is set to `Instant::now()`. On rejection,
    /// the state is unchanged and the error names both endpoints.
    ///
    /// Internally takes the write lock for the duration of the
    /// check-and-set; concurrent transitions are linearized.
    ///
    /// # Errors
    ///
    /// Returns [`TransitionError`] when the proposed move is
    /// not in the legal-transition table.
    pub fn transition(&self, to: KernelState) -> Result<(), TransitionError> {
        let (from, at) = {
            let mut guard = self
                .inner
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            assert_legal(guard.state, to)?;
            let from = guard.state;
            let at = Instant::now();
            guard.state = to;
            guard.last_transition = at;
            (from, at)
        };
        if let Some(events) = self.events.as_ref() {
            events.emit(KernelEvent::Lifecycle(LifecycleEvent::Transition {
                from,
                to,
                at,
            }));
        }
        if let Some(metrics) = self.metrics.as_ref() {
            metrics.gauge(
                metric_names::LIFECYCLE_PHASE,
                f64::from(to.phase().ordinal()),
                &[],
            );
            metrics.counter(
                metric_names::LIFECYCLE_TRANSITIONS,
                1,
                &[("to", to.as_str())],
            );
        }
        Ok(())
    }
}

impl Default for LifecycleController {
    /// Constructs a new controller, identical to [`LifecycleController::new`].
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for LifecycleController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let snap = self.snapshot();
        f.debug_struct("LifecycleController")
            .field("state", &snap.state)
            .field("phase", &snap.phase)
            .field("last_transition", &snap.last_transition)
            .finish()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn test_send_sync_bounds() {
        assert_send_sync::<LifecycleController>();
        assert_send_sync::<LifecycleSnapshot>();
    }

    #[test]
    fn test_new_starts_in_created_idle() {
        let c = LifecycleController::new();
        assert_eq!(c.state(), KernelState::Created);
        assert_eq!(c.phase(), Phase::Idle);
    }

    #[test]
    fn test_legal_transition_updates_state_phase_and_timestamp() {
        let c = LifecycleController::new();
        let before = c.snapshot().last_transition;
        std::thread::sleep(std::time::Duration::from_millis(2));
        c.transition(KernelState::Booting).unwrap();
        let after = c.snapshot();
        assert_eq!(after.state, KernelState::Booting);
        assert_eq!(after.phase, Phase::Boot);
        assert!(after.last_transition > before);
    }

    #[test]
    fn test_illegal_transition_returns_error_and_preserves_state() {
        let c = LifecycleController::new();
        let err = c.transition(KernelState::Running).unwrap_err();
        assert_eq!(err.from, KernelState::Created);
        assert_eq!(err.to, KernelState::Running);
        assert_eq!(c.state(), KernelState::Created);
    }

    #[test]
    fn test_full_happy_path() {
        let c = LifecycleController::new();
        for next in [
            KernelState::Booting,
            KernelState::Loading,
            KernelState::Running,
            KernelState::Stopping,
            KernelState::Stopped,
        ] {
            c.transition(next).unwrap();
            assert_eq!(c.state(), next);
        }
    }

    #[test]
    fn test_failure_path() {
        let c = LifecycleController::new();
        c.transition(KernelState::Booting).unwrap();
        c.transition(KernelState::Failed).unwrap();
        assert!(c.state().is_terminal());
    }

    #[test]
    fn test_degraded_path() {
        let c = LifecycleController::new();
        for next in [
            KernelState::Booting,
            KernelState::Loading,
            KernelState::Running,
            KernelState::Degraded,
            KernelState::Running,
            KernelState::Stopping,
            KernelState::Stopped,
        ] {
            c.transition(next).unwrap();
        }
        assert_eq!(c.state(), KernelState::Stopped);
    }

    #[test]
    fn test_terminal_state_rejects_all_outgoing_transitions() {
        let c = LifecycleController::new();
        c.transition(KernelState::Booting).unwrap();
        c.transition(KernelState::Failed).unwrap();
        for state in [
            KernelState::Created,
            KernelState::Booting,
            KernelState::Loading,
            KernelState::Running,
            KernelState::Degraded,
            KernelState::Stopping,
            KernelState::Stopped,
            KernelState::Failed,
        ] {
            assert!(
                c.transition(state).is_err(),
                "Failed accepted -> {:?}",
                state
            );
        }
    }

    #[test]
    fn test_snapshot_consistency() {
        let c = LifecycleController::new();
        c.transition(KernelState::Booting).unwrap();
        let snap = c.snapshot();
        assert_eq!(snap.state, c.state());
        assert_eq!(snap.phase, c.phase());
    }

    #[test]
    fn test_debug_includes_state_name() {
        let c = LifecycleController::new();
        let rendered = format!("{:?}", c);
        assert!(rendered.contains("LifecycleController"));
        assert!(rendered.contains("Created"));
    }

    #[test]
    fn test_default_starts_in_created() {
        let c = LifecycleController::default();
        assert_eq!(c.state(), KernelState::Created);
    }

    #[test]
    fn test_with_events_emits_transition_event() {
        use crate::events::{EventDispatcher, KernelEvent, LifecycleEvent};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let dispatcher = EventDispatcher::new();
        let count = Arc::new(AtomicUsize::new(0));
        let cb = Arc::clone(&count);
        let _ = dispatcher.subscribe("kernel.lifecycle.booting", move |event| {
            if let KernelEvent::Lifecycle(LifecycleEvent::Transition { from, to, .. }) = event {
                assert_eq!(*from, KernelState::Created);
                assert_eq!(*to, KernelState::Booting);
                let _ = cb.fetch_add(1, Ordering::Relaxed);
            }
        });

        let c = LifecycleController::with_events(dispatcher.handle());
        c.transition(KernelState::Booting).unwrap();
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_failed_transition_does_not_emit() {
        use crate::events::EventDispatcher;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let dispatcher = EventDispatcher::new();
        let count = Arc::new(AtomicUsize::new(0));
        let cb = Arc::clone(&count);
        let _ = dispatcher.subscribe("kernel.lifecycle.running", move |_| {
            let _ = cb.fetch_add(1, Ordering::Relaxed);
        });

        let c = LifecycleController::with_events(dispatcher.handle());
        assert!(c.transition(KernelState::Running).is_err());
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_set_events_attaches_handle_after_construction() {
        use crate::events::EventDispatcher;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let dispatcher = EventDispatcher::new();
        let count = Arc::new(AtomicUsize::new(0));
        let cb = Arc::clone(&count);
        let _ = dispatcher.subscribe("kernel.lifecycle.booting", move |_| {
            let _ = cb.fetch_add(1, Ordering::Relaxed);
        });

        let mut c = LifecycleController::new();
        c.set_events(dispatcher.handle());
        c.transition(KernelState::Booting).unwrap();
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_with_metrics_emits_gauge_and_counter() {
        use crate::metrics::{MetricsBackend, MetricsHandle};
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::Arc;
        use std::sync::Mutex;

        struct Capture {
            gauge: AtomicU64,
            counter: AtomicU64,
            last_label: Mutex<Option<String>>,
        }

        impl MetricsBackend for Capture {
            fn counter(&self, _name: &str, value: u64, labels: &[(&str, &str)]) {
                let _ = self.counter.fetch_add(value, Ordering::Relaxed);
                if let Some((_, v)) = labels.iter().find(|(k, _)| *k == "to") {
                    *self.last_label.lock().unwrap() = Some((*v).to_owned());
                }
            }
            fn gauge(&self, _name: &str, value: f64, _: &[(&str, &str)]) {
                self.gauge.store(value as u64, Ordering::Relaxed);
            }
            fn histogram(&self, _: &str, _: f64, _: &[(&str, &str)]) {}
        }

        let backend = Arc::new(Capture {
            gauge: AtomicU64::new(0),
            counter: AtomicU64::new(0),
            last_label: Mutex::new(None),
        });
        let handle: MetricsHandle = Arc::clone(&backend) as MetricsHandle;
        let c = LifecycleController::with_metrics(handle);
        c.transition(KernelState::Booting).unwrap();
        c.transition(KernelState::Loading).unwrap();

        assert_eq!(backend.counter.load(Ordering::Relaxed), 2);
        assert_eq!(
            backend.gauge.load(Ordering::Relaxed),
            u64::from(Phase::Load.ordinal())
        );
        assert_eq!(
            backend.last_label.lock().unwrap().as_deref(),
            Some("loading")
        );
    }

    #[test]
    fn test_failed_transition_does_not_touch_metrics() {
        use crate::metrics::{MetricsBackend, MetricsHandle};
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::Arc;

        struct Capture {
            calls: AtomicU64,
        }

        impl MetricsBackend for Capture {
            fn counter(&self, _: &str, _: u64, _: &[(&str, &str)]) {
                let _ = self.calls.fetch_add(1, Ordering::Relaxed);
            }
            fn gauge(&self, _: &str, _: f64, _: &[(&str, &str)]) {
                let _ = self.calls.fetch_add(1, Ordering::Relaxed);
            }
            fn histogram(&self, _: &str, _: f64, _: &[(&str, &str)]) {}
        }

        let backend = Arc::new(Capture {
            calls: AtomicU64::new(0),
        });
        let handle: MetricsHandle = Arc::clone(&backend) as MetricsHandle;
        let c = LifecycleController::with_metrics(handle);
        assert!(c.transition(KernelState::Running).is_err());
        assert_eq!(backend.calls.load(Ordering::Relaxed), 0);
    }
}
