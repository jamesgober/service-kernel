//! Integration tests for the basic kernel boot path.

#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use service_kernel::errors::KernelError;
use service_kernel::kernel::{KernelBuilder, KernelContext, Subsystem};
use service_kernel::lifecycle::KernelState;

#[test]
fn test_kernel_with_no_consumer_subsystems_boots_to_running() {
    let kernel = KernelBuilder::new("test").build().unwrap();
    assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Created);
    kernel.boot().unwrap();
    assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Running);
}

#[test]
fn test_consumer_subsystem_receives_boot_load_in_order() {
    static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    struct OrderTracker {
        boot_at: Arc<AtomicUsize>,
        load_at: Arc<AtomicUsize>,
    }

    impl Subsystem for OrderTracker {
        fn name(&self) -> &'static str {
            "tracker"
        }
        fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
            self.boot_at
                .store(SEQUENCE.fetch_add(1, Ordering::Relaxed), Ordering::Relaxed);
            Ok(())
        }
        fn load(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
            self.load_at
                .store(SEQUENCE.fetch_add(1, Ordering::Relaxed), Ordering::Relaxed);
            Ok(())
        }
    }

    SEQUENCE.store(0, Ordering::Relaxed);
    let boot_at = Arc::new(AtomicUsize::new(usize::MAX));
    let load_at = Arc::new(AtomicUsize::new(usize::MAX));

    let kernel = KernelBuilder::new("test")
        .with_subsystem(OrderTracker {
            boot_at: Arc::clone(&boot_at),
            load_at: Arc::clone(&load_at),
        })
        .build()
        .unwrap();
    kernel.boot().unwrap();
    assert!(boot_at.load(Ordering::Relaxed) < load_at.load(Ordering::Relaxed));
}

#[test]
fn test_subsystem_can_emit_through_event_handle() {
    static EVENT_COUNT: AtomicUsize = AtomicUsize::new(0);

    struct Emitter;

    impl Subsystem for Emitter {
        fn name(&self) -> &'static str {
            "emitter"
        }
        fn boot(&self, ctx: &KernelContext) -> Result<(), KernelError> {
            ctx.events.emit(service_kernel::events::KernelEvent::Custom(
                service_kernel::events::CustomEvent::new("emitter.boot", 1u32),
            ));
            Ok(())
        }
    }

    EVENT_COUNT.store(0, Ordering::Relaxed);

    let kernel = KernelBuilder::new("test")
        .with_subsystem(Emitter)
        .build()
        .unwrap();

    let handle = kernel.context().events;
    let _ = handle.subscribe("kernel.custom", |_event| {
        let _ = EVENT_COUNT.fetch_add(1, Ordering::Relaxed);
    });

    kernel.boot().unwrap();
    assert_eq!(EVENT_COUNT.load(Ordering::Relaxed), 1);
}

#[test]
fn test_subsystem_can_report_health() {
    use service_kernel::health::HealthStatus;

    struct Reporter;

    impl Subsystem for Reporter {
        fn name(&self) -> &'static str {
            "reporter"
        }
        fn dependencies(&self) -> &'static [&'static str] {
            &["health"]
        }
        fn boot(&self, ctx: &KernelContext) -> Result<(), KernelError> {
            ctx.health.report("reporter", HealthStatus::Degraded);
            Ok(())
        }
    }

    let kernel = KernelBuilder::new("test")
        .with_subsystem(Reporter)
        .build()
        .unwrap();
    kernel.boot().unwrap();
    let snap = kernel.snapshot();
    assert_eq!(snap.health.aggregate, HealthStatus::Degraded);
}

#[test]
fn test_kernel_name_round_trips_into_context() {
    let kernel = KernelBuilder::new("hivedb").build().unwrap();
    let ctx = kernel.context();
    assert_eq!(ctx.kernel_name(), "hivedb");
}
