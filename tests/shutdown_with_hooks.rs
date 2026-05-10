//! Integration test: shutdown hooks run in registration order;
//! failures don't stop the sequence.

#![cfg(feature = "tokio")]
#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use service_kernel::kernel::KernelBuilder;
use service_kernel::shutdown::{HookError, ShutdownContext, ShutdownHook};

struct CountingHook {
    name: &'static str,
    log: Arc<Mutex<Vec<&'static str>>>,
    fail: bool,
}

#[async_trait::async_trait]
impl ShutdownHook for CountingHook {
    fn name(&self) -> &'static str {
        self.name
    }
    async fn run(&self, _ctx: &ShutdownContext) -> Result<(), HookError> {
        self.log.lock().unwrap().push(self.name);
        if self.fail {
            Err(HookError::from_message(self.name, "intentional"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn test_hooks_run_in_registration_order() {
    let kernel = KernelBuilder::new("test")
        .with_shutdown_grace(Duration::from_secs(5))
        .build()
        .unwrap();
    let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
    kernel.register_shutdown_hook(CountingHook {
        name: "first",
        log: Arc::clone(&log),
        fail: false,
    });
    kernel.register_shutdown_hook(CountingHook {
        name: "second",
        log: Arc::clone(&log),
        fail: false,
    });
    kernel.register_shutdown_hook(CountingHook {
        name: "third",
        log: Arc::clone(&log),
        fail: false,
    });

    let other = kernel.clone();
    let join = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        other.shutdown();
    });
    kernel.run().unwrap();
    join.join().unwrap();

    assert_eq!(*log.lock().unwrap(), vec!["first", "second", "third"]);
}

#[test]
fn test_hook_failure_does_not_stop_subsequent_hooks() {
    let kernel = KernelBuilder::new("test")
        .with_shutdown_grace(Duration::from_secs(5))
        .build()
        .unwrap();
    let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
    kernel.register_shutdown_hook(CountingHook {
        name: "fail",
        log: Arc::clone(&log),
        fail: true,
    });
    let after_count = Arc::new(AtomicUsize::new(0));
    {
        let after_count = Arc::clone(&after_count);
        kernel.register_shutdown_hook(CountingHook {
            name: "after-fail",
            log: Arc::clone(&log),
            fail: false,
        });
        let _ = after_count.fetch_add(1, Ordering::Relaxed);
    }

    let other = kernel.clone();
    let join = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        other.shutdown();
    });
    kernel.run().unwrap();
    join.join().unwrap();

    let observed = log.lock().unwrap().clone();
    assert!(observed.contains(&"fail"));
    assert!(observed.contains(&"after-fail"));
}
