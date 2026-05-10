//! Smoke test: install_signal_handler does not block kernel shutdown.
//!
//! We can't trigger SIGTERM/Ctrl+C cleanly from a unit test, so the
//! smoke test just verifies the kernel still works when a handler is
//! installed and shutdown is signalled programmatically.

#![cfg(feature = "tokio")]
#![allow(clippy::unwrap_used)]

use std::thread;
use std::time::Duration;

use service_kernel::kernel::KernelBuilder;
use service_kernel::lifecycle::KernelState;

#[test]
fn test_install_signal_handler_does_not_block_programmatic_shutdown() {
    let kernel = KernelBuilder::new("test").build().unwrap();
    kernel.install_signal_handler();

    let other = kernel.clone();
    let join = thread::spawn(move || {
        thread::sleep(Duration::from_millis(30));
        other.shutdown();
    });
    kernel.run().unwrap();
    join.join().unwrap();

    assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Stopped);
}

#[test]
fn test_install_signal_handler_is_idempotent() {
    let kernel = KernelBuilder::new("test").build().unwrap();
    kernel.install_signal_handler();
    kernel.install_signal_handler();
    kernel.install_signal_handler();

    let other = kernel.clone();
    let join = thread::spawn(move || {
        thread::sleep(Duration::from_millis(30));
        other.shutdown();
    });
    kernel.run().unwrap();
    join.join().unwrap();
    assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Stopped);
}
