//! Concurrent stress test for [`HealthRegistry`].
//!
//! 32 threads × 100_000 reports each (3.2M total) into 50
//! subsystems, plus a separate reader thread polling `aggregate()`
//! every 100µs. Wall-clock cap: 30 seconds.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use service_kernel::health::{HealthRegistry, HealthStatus};

const THREADS: usize = 32;
const PER_THREAD: usize = 100_000;
const SUBSYSTEMS: usize = 50;
const WALL_CLOCK_CAP: Duration = Duration::from_secs(30);

const STATUSES: [HealthStatus; 5] = [
    HealthStatus::Healthy,
    HealthStatus::Degraded,
    HealthStatus::Unhealthy,
    HealthStatus::Critical,
    HealthStatus::Unknown,
];

const NAMES: [&str; SUBSYSTEMS] = [
    "s00", "s01", "s02", "s03", "s04", "s05", "s06", "s07", "s08", "s09", "s10", "s11", "s12",
    "s13", "s14", "s15", "s16", "s17", "s18", "s19", "s20", "s21", "s22", "s23", "s24", "s25",
    "s26", "s27", "s28", "s29", "s30", "s31", "s32", "s33", "s34", "s35", "s36", "s37", "s38",
    "s39", "s40", "s41", "s42", "s43", "s44", "s45", "s46", "s47", "s48", "s49",
];

fn next_xorshift(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

#[test]
fn test_health_registry_under_concurrent_pressure() {
    let registry = Arc::new(HealthRegistry::new());
    let stop = Arc::new(AtomicBool::new(false));
    let start = Instant::now();

    // Reader thread
    let reader_registry = Arc::clone(&registry);
    let reader_stop = Arc::clone(&stop);
    let reader_handle = thread::spawn(move || {
        while !reader_stop.load(Ordering::Relaxed) {
            let agg = reader_registry.aggregate();
            assert!(STATUSES.contains(&agg));
            thread::sleep(Duration::from_micros(100));
        }
    });

    // Writer threads
    let mut writers = Vec::with_capacity(THREADS);
    for thread_idx in 0..THREADS {
        let r = Arc::clone(&registry);
        let mut seed = (thread_idx as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        if seed == 0 {
            seed = 0x1234_5678_9ABC_DEF0;
        }
        writers.push(thread::spawn(move || {
            for _ in 0..PER_THREAD {
                let r1 = next_xorshift(&mut seed);
                let r2 = next_xorshift(&mut seed);
                let name = NAMES[(r1 as usize) % NAMES.len()];
                let status = STATUSES[(r2 as usize) % STATUSES.len()];
                r.report(name, status);
            }
        }));
    }

    for w in writers {
        w.join().expect("writer panicked");
    }

    stop.store(true, Ordering::Relaxed);
    reader_handle.join().expect("reader panicked");

    let elapsed = start.elapsed();
    assert!(
        elapsed < WALL_CLOCK_CAP,
        "stress took {:?}, exceeded {:?}",
        elapsed,
        WALL_CLOCK_CAP,
    );

    let snap = registry.snapshot();
    let max_in_map = snap.subsystems.values().copied().max();
    let aggregate = snap.aggregate;

    if let Some(max) = max_in_map {
        assert_eq!(
            aggregate, max,
            "aggregate {:?} differs from max-of-subsystems {:?}",
            aggregate, max,
        );
    } else {
        assert_eq!(aggregate, HealthStatus::Healthy);
    }

    assert!(snap.subsystems.len() <= NAMES.len());
}
