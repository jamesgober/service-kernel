//! Integration tests for kernel boot failure paths.

#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use service_kernel::errors::{KernelError, KernelErrorCode};
use service_kernel::kernel::{BuildError, KernelBuilder, KernelContext, Subsystem};
use service_kernel::lifecycle::KernelState;

struct AlwaysOk {
    name: &'static str,
    deps: &'static [&'static str],
    shutdown_count: Arc<AtomicUsize>,
}

impl Subsystem for AlwaysOk {
    fn name(&self) -> &'static str {
        self.name
    }
    fn dependencies(&self) -> &'static [&'static str] {
        self.deps
    }
    fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
        Ok(())
    }
    fn shutdown(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
        let _ = self.shutdown_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

struct FailBoot;

impl Subsystem for FailBoot {
    fn name(&self) -> &'static str {
        "fail_boot"
    }
    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }
    fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
        Err(KernelError::Internal {
            code: KernelErrorCode::Internal,
            message: "boot failed".to_owned(),
        })
    }
}

struct FailLoad {
    deps: &'static [&'static str],
}

impl Subsystem for FailLoad {
    fn name(&self) -> &'static str {
        "fail_load"
    }
    fn dependencies(&self) -> &'static [&'static str] {
        self.deps
    }
    fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
        Ok(())
    }
    fn load(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
        Err(KernelError::Internal {
            code: KernelErrorCode::Internal,
            message: "load failed".to_owned(),
        })
    }
}

#[test]
fn test_boot_failure_transitions_to_failed() {
    let kernel = KernelBuilder::new("test")
        .with_subsystem(FailBoot)
        .build()
        .unwrap();
    let err = kernel.boot().unwrap_err();
    assert!(matches!(err, KernelError::Internal { .. }));
    assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Failed);
}

#[test]
fn test_load_failure_runs_shutdown_on_loaded_subsystems_in_reverse() {
    let count = Arc::new(AtomicUsize::new(0));

    let kernel = KernelBuilder::new("test")
        .with_subsystem(AlwaysOk {
            name: "alpha",
            deps: &[],
            shutdown_count: Arc::clone(&count),
        })
        .with_subsystem(AlwaysOk {
            name: "beta",
            deps: &["alpha"],
            shutdown_count: Arc::clone(&count),
        })
        .with_subsystem(FailLoad { deps: &["beta"] })
        .build()
        .unwrap();

    let err = kernel.boot().unwrap_err();
    assert!(matches!(err, KernelError::Internal { .. }));
    assert_eq!(count.load(Ordering::Relaxed), 2);
    assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Failed);
}

#[test]
fn test_panicking_boot_caught_and_reported() {
    struct Panics;

    impl Subsystem for Panics {
        fn name(&self) -> &'static str {
            "panics"
        }
        fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
            panic!("intentional");
        }
    }

    let kernel = KernelBuilder::new("test")
        .with_subsystem(Panics)
        .build()
        .unwrap();
    let err = kernel.boot().unwrap_err();
    match err {
        KernelError::Subsystem { name, .. } => assert_eq!(name, "panics"),
        other => panic!("expected Subsystem error, got {:?}", other),
    }
    assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Failed);
}

#[test]
fn test_build_error_displays_through_lang_lib() {
    let err = KernelBuilder::new("test")
        .with_subsystem(AlwaysOk {
            name: "missing_dep",
            deps: &["does_not_exist"],
            shutdown_count: Arc::new(AtomicUsize::new(0)),
        })
        .build()
        .unwrap_err();
    let rendered = err.to_string();
    assert!(rendered.contains("missing_dep"));
    assert!(rendered.contains("does_not_exist"));
    assert!(matches!(err, BuildError::MissingDependency { .. }));
}

#[test]
fn test_build_error_for_empty_name() {
    let err = KernelBuilder::new("").build().unwrap_err();
    assert!(matches!(err, BuildError::EmptyName));
    let rendered = err.to_string();
    assert!(!rendered.is_empty());
}

#[test]
fn test_build_error_for_reserved_name() {
    let err = KernelBuilder::new("test")
        .with_subsystem(AlwaysOk {
            name: "events",
            deps: &[],
            shutdown_count: Arc::new(AtomicUsize::new(0)),
        })
        .build()
        .unwrap_err();
    assert!(matches!(err, BuildError::ReservedName { name: "events" }));
}
