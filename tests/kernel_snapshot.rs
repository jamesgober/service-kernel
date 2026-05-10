//! Integration tests for `Kernel::snapshot()` across the lifecycle.

#![allow(clippy::unwrap_used)]

use std::thread;
use std::time::Duration;

use service_kernel::errors::KernelError;
use service_kernel::kernel::{KernelBuilder, KernelContext, Subsystem};
use service_kernel::lifecycle::KernelState;

struct Plain {
    name: &'static str,
}

impl Subsystem for Plain {
    fn name(&self) -> &'static str {
        self.name
    }
    fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
        Ok(())
    }
}

#[test]
fn test_snapshot_before_boot_lists_subsystems_with_no_timestamps() {
    let kernel = KernelBuilder::new("test")
        .with_subsystem(Plain { name: "alpha" })
        .build()
        .unwrap();
    let snap = kernel.snapshot();
    assert_eq!(snap.lifecycle.state, KernelState::Created);
    let alpha = snap.subsystems.iter().find(|s| s.name == "alpha").unwrap();
    assert!(alpha.booted_at.is_none());
    assert!(alpha.loaded_at.is_none());
}

#[test]
fn test_snapshot_after_boot_records_boot_and_load_timestamps() {
    let kernel = KernelBuilder::new("test")
        .with_subsystem(Plain { name: "alpha" })
        .build()
        .unwrap();
    kernel.boot().unwrap();
    let snap = kernel.snapshot();
    assert_eq!(snap.lifecycle.state, KernelState::Running);
    let alpha = snap.subsystems.iter().find(|s| s.name == "alpha").unwrap();
    assert!(alpha.booted_at.is_some());
    assert!(alpha.loaded_at.is_some());
}

#[test]
fn test_snapshot_during_run_reflects_running_state() {
    let kernel = KernelBuilder::new("test").build().unwrap();
    let other = kernel.clone();

    let join = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        let snap = other.snapshot();
        assert_eq!(snap.lifecycle.state, KernelState::Running);
        other.shutdown();
    });

    kernel.run().unwrap();
    join.join().unwrap();
}

#[test]
fn test_snapshot_after_run_reports_stopped_state() {
    let kernel = KernelBuilder::new("test").build().unwrap();
    kernel.shutdown();
    kernel.run().unwrap();
    let snap = kernel.snapshot();
    assert_eq!(snap.lifecycle.state, KernelState::Stopped);
}

#[test]
fn test_snapshot_includes_kernel_name() {
    let kernel = KernelBuilder::new("hivedb").build().unwrap();
    let snap = kernel.snapshot();
    assert_eq!(snap.name, "hivedb");
}
