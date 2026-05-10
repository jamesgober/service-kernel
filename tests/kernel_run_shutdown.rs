//! Integration tests for the full run + shutdown cycle.

#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use service_kernel::errors::KernelError;
use service_kernel::kernel::{KernelBuilder, KernelContext, Subsystem};
use service_kernel::lifecycle::KernelState;

struct ShutdownCounter {
    name: &'static str,
    count: Arc<AtomicUsize>,
}

impl Subsystem for ShutdownCounter {
    fn name(&self) -> &'static str {
        self.name
    }
    fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
        Ok(())
    }
    fn shutdown(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
        let _ = self.count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[test]
fn test_run_blocks_until_shutdown_signal_then_terminates() {
    let kernel = KernelBuilder::new("test").build().unwrap();
    let other = kernel.clone();

    let join = thread::spawn(move || {
        thread::sleep(Duration::from_millis(15));
        other.shutdown();
    });

    kernel.run().unwrap();
    join.join().unwrap();
    assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Stopped);
}

#[test]
fn test_run_invokes_consumer_subsystem_shutdown_in_reverse() {
    let count = Arc::new(AtomicUsize::new(0));

    let kernel = KernelBuilder::new("test")
        .with_subsystem(ShutdownCounter {
            name: "first",
            count: Arc::clone(&count),
        })
        .with_subsystem(ShutdownCounter {
            name: "second",
            count: Arc::clone(&count),
        })
        .build()
        .unwrap();

    let other = kernel.clone();
    let join = thread::spawn(move || {
        thread::sleep(Duration::from_millis(10));
        other.shutdown();
    });

    kernel.run().unwrap();
    join.join().unwrap();
    assert_eq!(count.load(Ordering::Relaxed), 2);
    assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Stopped);
}

#[test]
fn test_shutdown_via_context_handle_works() {
    let kernel = KernelBuilder::new("test").build().unwrap();
    let shutdown = kernel.context().shutdown;

    let join = thread::spawn(move || {
        thread::sleep(Duration::from_millis(10));
        shutdown.signal();
    });

    kernel.run().unwrap();
    join.join().unwrap();
    assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Stopped);
}

#[test]
fn test_shutdown_signalled_before_run_returns_immediately_after_boot() {
    let kernel = KernelBuilder::new("test").build().unwrap();
    kernel.shutdown(); // signal before run
    kernel.run().unwrap();
    assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Stopped);
}
