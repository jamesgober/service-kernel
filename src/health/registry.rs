//! Push-based per-subsystem health registry.
//!
//! [`HealthRegistry`] owns the shared state; subsystems and consumers
//! reach it through [`HealthHandle`]. The aggregate is recomputed
//! eagerly on every `report` and kept in an [`AtomicU8`]-backed
//! cell, so [`aggregate`](HealthRegistry::aggregate) is a single
//! lock-free atomic load.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, RwLock};

use crate::events::{EventHandle, HealthEvent, KernelEvent};
use crate::primitives::Instant;

use super::{HealthSnapshot, HealthStatus};

/// Lock-free atomic cell holding a [`HealthStatus`].
struct AtomicHealthStatus(AtomicU8);

impl AtomicHealthStatus {
    #[inline]
    const fn new(initial: HealthStatus) -> Self {
        Self(AtomicU8::new(initial as u8))
    }

    #[inline]
    fn load(&self) -> HealthStatus {
        HealthStatus::from_u8(self.0.load(Ordering::Acquire))
    }

    #[inline]
    fn store(&self, status: HealthStatus) {
        self.0.store(status as u8, Ordering::Release);
    }
}

/// Internal state shared between a [`HealthRegistry`] and its derived
/// [`HealthHandle`]s.
struct HealthInner {
    subsystems: RwLock<HashMap<&'static str, HealthStatus>>,
    aggregate: AtomicHealthStatus,
    events: Option<EventHandle>,
}

impl HealthInner {
    fn new(events: Option<EventHandle>) -> Self {
        Self {
            subsystems: RwLock::new(HashMap::new()),
            aggregate: AtomicHealthStatus::new(HealthStatus::Healthy),
            events,
        }
    }

    fn aggregate(&self) -> HealthStatus {
        self.aggregate.load()
    }

    fn subsystem(&self, name: &'static str) -> Option<HealthStatus> {
        let guard = self
            .subsystems
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.get(name).copied()
    }

    fn snapshot(&self) -> HealthSnapshot {
        let guard = self
            .subsystems
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        HealthSnapshot {
            aggregate: self.aggregate.load(),
            subsystems: guard.clone(),
            timestamp: Instant::now(),
        }
    }

    fn report(&self, subsystem: &'static str, status: HealthStatus) {
        let prev_aggregate = self.aggregate.load();
        let (subsystem_changed, prev_subsystem, new_aggregate) = {
            let mut guard = self
                .subsystems
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);

            let prev_subsystem = guard.insert(subsystem, status);
            let subsystem_changed = prev_subsystem != Some(status);

            let new_aggregate = if status > prev_aggregate {
                status
            } else if matches!(prev_subsystem, Some(prev) if prev == prev_aggregate)
                && status < prev_aggregate
            {
                guard.values().copied().max().unwrap_or(HealthStatus::Healthy)
            } else {
                prev_aggregate
            };

            if new_aggregate != prev_aggregate {
                self.aggregate.store(new_aggregate);
            }

            (subsystem_changed, prev_subsystem, new_aggregate)
        };

        if !subsystem_changed {
            return;
        }

        if let Some(events) = self.events.as_ref() {
            let at = Instant::now();
            let from = prev_subsystem.unwrap_or(HealthStatus::Unknown);
            events.emit(KernelEvent::Health(HealthEvent::SubsystemChanged {
                subsystem,
                from,
                to: status,
                at,
            }));
            if new_aggregate != prev_aggregate {
                events.emit(KernelEvent::Health(HealthEvent::AggregateChanged {
                    from: prev_aggregate,
                    to: new_aggregate,
                    at,
                }));
            }
        }
    }
}

/// Push-based health registry.
///
/// Construct one per kernel and hand out [`HealthHandle`]s to
/// subsystems. The registry owns the aggregation state; handles share
/// it via an `Arc`. `HealthRegistry` is `Send + Sync`.
pub struct HealthRegistry {
    inner: Arc<HealthInner>,
}

impl HealthRegistry {
    /// Constructs an empty registry without event wiring.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(HealthInner::new(None)),
        }
    }

    /// Constructs a registry wired to the given [`EventHandle`].
    ///
    /// Reports that change a subsystem's status emit
    /// [`HealthEvent::SubsystemChanged`]. Reports that move the
    /// aggregate emit [`HealthEvent::AggregateChanged`].
    #[inline]
    #[must_use]
    pub fn with_events(events: EventHandle) -> Self {
        Self {
            inner: Arc::new(HealthInner::new(Some(events))),
        }
    }

    /// Returns a cheap, cloneable handle that shares this registry's
    /// state.
    #[inline]
    #[must_use]
    pub fn handle(&self) -> HealthHandle {
        HealthHandle {
            inner: Arc::clone(&self.inner),
        }
    }

    /// Returns the current aggregate status.
    ///
    /// Lock-free atomic load.
    #[inline]
    #[must_use]
    pub fn aggregate(&self) -> HealthStatus {
        self.inner.aggregate()
    }

    /// Returns a snapshot of every subsystem's status plus the
    /// aggregate.
    #[inline]
    #[must_use]
    pub fn snapshot(&self) -> HealthSnapshot {
        self.inner.snapshot()
    }

    /// Returns the status of a single subsystem, or `None` if the
    /// subsystem has not reported.
    #[inline]
    #[must_use]
    pub fn subsystem(&self, name: &'static str) -> Option<HealthStatus> {
        self.inner.subsystem(name)
    }

    /// Pushes a subsystem's status into the registry.
    ///
    /// The aggregate is updated eagerly. If wired to an
    /// `EventHandle`, the registry emits subsystem-change and
    /// aggregate-change events as appropriate.
    #[inline]
    pub fn report(&self, subsystem: &'static str, status: HealthStatus) {
        self.inner.report(subsystem, status);
    }
}

impl Default for HealthRegistry {
    /// Returns a new registry without event wiring.
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for HealthRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HealthRegistry")
            .field("aggregate", &self.aggregate())
            .finish_non_exhaustive()
    }
}

/// Cheap, cloneable handle to a [`HealthRegistry`].
///
/// All clones share one set of subsystem states. A subsystem reports
/// through its own handle and the rest of the kernel sees the new
/// state through any other handle (or through the registry itself).
#[derive(Clone)]
pub struct HealthHandle {
    inner: Arc<HealthInner>,
}

impl HealthHandle {
    /// Pushes a subsystem's status. See
    /// [`HealthRegistry::report`].
    ///
    /// # Examples
    ///
    /// ```
    /// use service_kernel::health::{HealthHandle, HealthRegistry, HealthStatus};
    ///
    /// let registry = HealthRegistry::new();
    /// let handle: HealthHandle = registry.handle();
    /// handle.report("storage", HealthStatus::Degraded);
    /// assert_eq!(registry.subsystem("storage"), Some(HealthStatus::Degraded));
    /// ```
    #[inline]
    pub fn report(&self, subsystem: &'static str, status: HealthStatus) {
        self.inner.report(subsystem, status);
    }

    /// Returns the current aggregate status.
    #[inline]
    #[must_use]
    pub fn aggregate(&self) -> HealthStatus {
        self.inner.aggregate()
    }

    /// Returns a snapshot of every subsystem's status plus the
    /// aggregate.
    #[inline]
    #[must_use]
    pub fn snapshot(&self) -> HealthSnapshot {
        self.inner.snapshot()
    }

    /// Returns the status of a single subsystem, or `None` if the
    /// subsystem has not reported.
    #[inline]
    #[must_use]
    pub fn subsystem(&self, name: &'static str) -> Option<HealthStatus> {
        self.inner.subsystem(name)
    }
}

impl fmt::Debug for HealthHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HealthHandle")
            .field("aggregate", &self.aggregate())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::events::{EventDispatcher, KernelEvent, LifecycleEvent};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn test_registry_send_sync() {
        assert_send_sync::<HealthRegistry>();
        assert_send_sync::<HealthHandle>();
    }

    #[test]
    fn test_empty_registry_aggregate_is_healthy() {
        let r = HealthRegistry::new();
        assert_eq!(r.aggregate(), HealthStatus::Healthy);
        assert_eq!(r.snapshot().subsystems.len(), 0);
    }

    #[test]
    fn test_single_report_drives_aggregate() {
        let r = HealthRegistry::new();
        r.report("a", HealthStatus::Degraded);
        assert_eq!(r.aggregate(), HealthStatus::Degraded);
        assert_eq!(r.subsystem("a"), Some(HealthStatus::Degraded));
    }

    #[test]
    fn test_aggregate_is_max_across_subsystems() {
        let r = HealthRegistry::new();
        r.report("a", HealthStatus::Degraded);
        r.report("b", HealthStatus::Critical);
        r.report("c", HealthStatus::Healthy);
        assert_eq!(r.aggregate(), HealthStatus::Critical);
    }

    #[test]
    fn test_repeated_report_replaces_state() {
        let r = HealthRegistry::new();
        r.report("a", HealthStatus::Degraded);
        r.report("a", HealthStatus::Critical);
        assert_eq!(r.subsystem("a"), Some(HealthStatus::Critical));
        assert_eq!(r.aggregate(), HealthStatus::Critical);
    }

    #[test]
    fn test_aggregate_drops_when_only_subsystem_recovers() {
        let r = HealthRegistry::new();
        r.report("a", HealthStatus::Critical);
        assert_eq!(r.aggregate(), HealthStatus::Critical);
        r.report("a", HealthStatus::Healthy);
        assert_eq!(r.aggregate(), HealthStatus::Healthy);
    }

    #[test]
    fn test_aggregate_holds_when_other_subsystem_is_worst() {
        let r = HealthRegistry::new();
        r.report("a", HealthStatus::Critical);
        r.report("b", HealthStatus::Critical);
        r.report("a", HealthStatus::Healthy);
        assert_eq!(r.aggregate(), HealthStatus::Critical);
    }

    #[test]
    fn test_unknown_drives_aggregate_above_critical() {
        let r = HealthRegistry::new();
        r.report("a", HealthStatus::Critical);
        r.report("b", HealthStatus::Unknown);
        assert_eq!(r.aggregate(), HealthStatus::Unknown);
    }

    #[test]
    fn test_subsystem_changed_event_fires_per_unique_state() {
        let dispatcher = EventDispatcher::new();
        let count = Arc::new(AtomicUsize::new(0));
        let cb = Arc::clone(&count);
        let _ = dispatcher.subscribe("kernel.health.storage", move |event| {
            if matches!(
                event,
                KernelEvent::Health(HealthEvent::SubsystemChanged { subsystem: "storage", .. })
            ) {
                let _ = cb.fetch_add(1, Ordering::Relaxed);
            }
        });

        let r = HealthRegistry::with_events(dispatcher.handle());
        r.report("storage", HealthStatus::Degraded);
        r.report("storage", HealthStatus::Degraded);
        r.report("storage", HealthStatus::Critical);
        assert_eq!(count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_aggregate_changed_event_fires_only_on_max_change() {
        let dispatcher = EventDispatcher::new();
        let captured: Arc<Mutex<Vec<(HealthStatus, HealthStatus)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let cb = Arc::clone(&captured);
        let _ = dispatcher.subscribe("kernel.health.aggregate", move |event| {
            if let KernelEvent::Health(HealthEvent::AggregateChanged { from, to, .. }) = event {
                cb.lock().unwrap().push((*from, *to));
            }
        });

        let r = HealthRegistry::with_events(dispatcher.handle());
        r.report("a", HealthStatus::Degraded);
        r.report("b", HealthStatus::Degraded);
        r.report("a", HealthStatus::Critical);
        r.report("b", HealthStatus::Healthy);
        r.report("a", HealthStatus::Healthy);

        let log = captured.lock().unwrap();
        assert_eq!(
            *log,
            vec![
                (HealthStatus::Healthy, HealthStatus::Degraded),
                (HealthStatus::Degraded, HealthStatus::Critical),
                (HealthStatus::Critical, HealthStatus::Healthy),
            ]
        );
    }

    #[test]
    fn test_handle_clone_shares_state() {
        let r = HealthRegistry::new();
        let h1 = r.handle();
        let h2 = h1.clone();
        h1.report("a", HealthStatus::Critical);
        assert_eq!(h2.aggregate(), HealthStatus::Critical);
    }

    #[test]
    fn test_no_event_when_status_unchanged() {
        let dispatcher = EventDispatcher::new();
        let count = Arc::new(AtomicUsize::new(0));
        let cb = Arc::clone(&count);
        let _ = dispatcher.subscribe("kernel.health.storage", move |event| {
            if matches!(event, KernelEvent::Health(_)) {
                let _ = cb.fetch_add(1, Ordering::Relaxed);
            }
        });

        let r = HealthRegistry::with_events(dispatcher.handle());
        r.report("storage", HealthStatus::Healthy);
        let after_first = count.load(Ordering::Relaxed);
        r.report("storage", HealthStatus::Healthy);
        assert_eq!(count.load(Ordering::Relaxed), after_first);
    }

    #[test]
    fn test_unrelated_subscriber_unaffected() {
        // Sanity check: lifecycle event subscribers don't see health events.
        let dispatcher = EventDispatcher::new();
        let count = Arc::new(AtomicUsize::new(0));
        let cb = Arc::clone(&count);
        let _ = dispatcher.subscribe("kernel.lifecycle.running", move |event| {
            if matches!(event, KernelEvent::Lifecycle(LifecycleEvent::Transition { .. })) {
                let _ = cb.fetch_add(1, Ordering::Relaxed);
            }
        });

        let r = HealthRegistry::with_events(dispatcher.handle());
        r.report("storage", HealthStatus::Critical);
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }
}
