//! Minimal kernel example.
//!
//! Boots a kernel with no consumer subsystems, prints the lifecycle
//! transitions as they happen, and shuts down after one second.
//!
//! Run with: `cargo run --example minimal`

use std::thread;
use std::time::Duration;

use service_kernel::events::{KernelEvent, LifecycleEvent};
use service_kernel::prelude::*;

fn main() -> Result<(), KernelError> {
    let kernel = KernelBuilder::new("minimal").build().map_err(|e| {
        // The builder's error is its own type; wrap it in a generic
        // KernelError so the example's return type stays simple.
        KernelError::Internal {
            code: service_kernel::errors::KernelErrorCode::Internal,
            message: format!("build failed: {e}"),
        }
    })?;

    // Subscribe to every lifecycle topic so the example prints the
    // full state progression as it happens.
    for topic in [
        "kernel.lifecycle.booting",
        "kernel.lifecycle.loading",
        "kernel.lifecycle.running",
        "kernel.lifecycle.stopping",
        "kernel.lifecycle.stopped",
        "kernel.lifecycle.shutdown_started",
        "kernel.lifecycle.shutdown_completed",
    ] {
        let _ = kernel
            .context()
            .events
            .subscribe(topic, move |event| match event {
                KernelEvent::Lifecycle(LifecycleEvent::Transition { from, to, .. }) => {
                    println!("transition {} -> {}", from, to);
                }
                KernelEvent::Lifecycle(LifecycleEvent::ShutdownStarted { .. }) => {
                    println!("shutdown: started");
                }
                KernelEvent::Lifecycle(LifecycleEvent::ShutdownCompleted { duration, .. }) => {
                    println!("shutdown: completed in {:?}", duration);
                }
                _ => {}
            });
    }

    // Spawn a thread that signals shutdown after one second.
    let shutdown = kernel.clone();
    let join = thread::spawn(move || {
        thread::sleep(Duration::from_secs(1));
        shutdown.shutdown();
    });

    let result = kernel.run();
    let _ = join.join();
    result
}
