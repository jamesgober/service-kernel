//! Fan-out stress test for [`EventDispatcher`].
//!
//! 1 publisher × 100 subscribers × 100_000 events on the same topic.
//! Each subscriber must observe every event exactly once. Wall-clock
//! cap: 30 seconds.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use service_kernel::events::{EventDispatcher, KernelEvent, LifecycleEvent};
use service_kernel::lifecycle::KernelState;
use service_kernel::primitives::Instant as KernelInstant;

const SUBSCRIBERS: usize = 100;
const EVENTS: usize = 100_000;
const WALL_CLOCK_CAP: Duration = Duration::from_secs(30);

#[test]
fn test_fanout_every_subscriber_receives_every_event() {
    let dispatcher = EventDispatcher::new();

    let counters: Vec<Arc<AtomicUsize>> = (0..SUBSCRIBERS)
        .map(|_| Arc::new(AtomicUsize::new(0)))
        .collect();

    for counter in &counters {
        let counter = Arc::clone(counter);
        let _ = dispatcher.subscribe("kernel.lifecycle.running", move |_| {
            let _ = counter.fetch_add(1, Ordering::Relaxed);
        });
    }

    assert_eq!(
        dispatcher.subscriber_count("kernel.lifecycle.running"),
        SUBSCRIBERS
    );

    let start = Instant::now();
    for _ in 0..EVENTS {
        dispatcher.emit(KernelEvent::Lifecycle(LifecycleEvent::Transition {
            from: KernelState::Loading,
            to: KernelState::Running,
            at: KernelInstant::now(),
        }));
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed < WALL_CLOCK_CAP,
        "fanout exceeded wall-clock cap: took {:?}",
        elapsed,
    );

    for (i, counter) in counters.iter().enumerate() {
        assert_eq!(
            counter.load(Ordering::Relaxed),
            EVENTS,
            "subscriber {} received {} events, expected {}",
            i,
            counter.load(Ordering::Relaxed),
            EVENTS,
        );
    }
}
