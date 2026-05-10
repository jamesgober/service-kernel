//! Integration tests for the kernel's health registry.

#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use service_kernel::events::{EventDispatcher, HealthEvent, KernelEvent};
use service_kernel::health::{HealthHandle, HealthRegistry, HealthStatus};

#[test]
fn test_empty_registry_aggregate_is_healthy() {
    let r = HealthRegistry::new();
    assert_eq!(r.aggregate(), HealthStatus::Healthy);
}

#[test]
fn test_handles_share_state_with_registry() {
    let r = HealthRegistry::new();
    let h: HealthHandle = r.handle();
    h.report("storage", HealthStatus::Degraded);
    assert_eq!(r.subsystem("storage"), Some(HealthStatus::Degraded));
    assert_eq!(r.aggregate(), HealthStatus::Degraded);
}

#[test]
fn test_aggregate_takes_max_across_subsystems() {
    let r = HealthRegistry::new();
    r.report("a", HealthStatus::Healthy);
    r.report("b", HealthStatus::Critical);
    r.report("c", HealthStatus::Degraded);
    assert_eq!(r.aggregate(), HealthStatus::Critical);
}

#[test]
fn test_unknown_outranks_critical_in_aggregate() {
    let r = HealthRegistry::new();
    r.report("a", HealthStatus::Critical);
    r.report("b", HealthStatus::Unknown);
    assert_eq!(r.aggregate(), HealthStatus::Unknown);
}

#[test]
fn test_snapshot_lists_actionable_subsystems_sorted() {
    let r = HealthRegistry::new();
    r.report("zebra", HealthStatus::Unhealthy);
    r.report("apple", HealthStatus::Healthy);
    r.report("mango", HealthStatus::Critical);
    let snap = r.snapshot();
    assert_eq!(snap.unhealthy_subsystems(), vec!["mango", "zebra"]);
    assert_eq!(snap.count_by_status(HealthStatus::Healthy), 1);
}

#[test]
fn test_aggregate_change_event_fires_only_when_max_changes() {
    let dispatcher = EventDispatcher::new();
    let count = Arc::new(AtomicUsize::new(0));
    let cb = Arc::clone(&count);
    let _ = dispatcher.subscribe("kernel.health.aggregate", move |event| {
        if matches!(
            event,
            KernelEvent::Health(HealthEvent::AggregateChanged { .. })
        ) {
            let _ = cb.fetch_add(1, Ordering::Relaxed);
        }
    });

    let r = HealthRegistry::with_events(dispatcher.handle());
    r.report("a", HealthStatus::Degraded); // aggregate Healthy -> Degraded
    r.report("b", HealthStatus::Healthy); // aggregate stays Degraded
    r.report("a", HealthStatus::Critical); // aggregate Degraded -> Critical
    assert_eq!(count.load(Ordering::Relaxed), 2);
}

#[test]
fn test_subsystem_change_event_carries_from_to() {
    let dispatcher = EventDispatcher::new();
    let observed: Arc<std::sync::Mutex<Option<(HealthStatus, HealthStatus)>>> =
        Arc::new(std::sync::Mutex::new(None));
    let cb = Arc::clone(&observed);
    let _ = dispatcher.subscribe("kernel.health.storage", move |event| {
        if let KernelEvent::Health(HealthEvent::SubsystemChanged {
            subsystem: "storage",
            from,
            to,
            ..
        }) = event
        {
            *cb.lock().unwrap() = Some((*from, *to));
        }
    });

    let r = HealthRegistry::with_events(dispatcher.handle());
    r.report("storage", HealthStatus::Degraded);
    assert_eq!(
        *observed.lock().unwrap(),
        Some((HealthStatus::Unknown, HealthStatus::Degraded))
    );
}
