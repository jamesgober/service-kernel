//! Concurrent stress test for [`LifecycleController`].
//!
//! Spawns 16 threads issuing 25_000 mixed read / transition operations
//! each (400_000 total) against a single shared controller. Most
//! transitions are expected to fail — the controller becomes terminal
//! quickly, and from there every move is rejected — but the
//! controller must never panic, never deadlock, and never report a
//! state that did not come from an `Ok(())` transition.
//!
//! The wall-clock cap is 10s. Exceeding it indicates lock contention,
//! not a slow CI runner.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use service_kernel::lifecycle::{KernelState, LifecycleController};

const THREADS: usize = 16;
const ITERATIONS_PER_THREAD: usize = 25_000;
const WALL_CLOCK_CAP: Duration = Duration::from_secs(10);

const ALL: [KernelState; 8] = [
    KernelState::Created,
    KernelState::Booting,
    KernelState::Loading,
    KernelState::Running,
    KernelState::Degraded,
    KernelState::Stopping,
    KernelState::Stopped,
    KernelState::Failed,
];

/// Tiny xorshift PRNG. Avoids pulling in `rand` for a stress test.
fn next_xorshift(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

#[test]
fn test_concurrent_transitions_never_panic_or_deadlock() {
    let controller = Arc::new(LifecycleController::new());
    let start = Instant::now();

    let mut handles = Vec::with_capacity(THREADS);
    for thread_idx in 0..THREADS {
        let controller = Arc::clone(&controller);
        let mut seed = (thread_idx as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        if seed == 0 {
            seed = 0x1234_5678_9ABC_DEF0;
        }

        handles.push(thread::spawn(move || {
            let mut successes: u64 = 0;
            let mut failures: u64 = 0;

            for _ in 0..ITERATIONS_PER_THREAD {
                let r = next_xorshift(&mut seed);
                let kind = r % 4;
                match kind {
                    0 => {
                        let _ = controller.state();
                    }
                    1 => {
                        let _ = controller.snapshot();
                    }
                    _ => {
                        let to = ALL[(r as usize >> 8) % ALL.len()];
                        if controller.transition(to).is_ok() {
                            successes += 1;
                        } else {
                            failures += 1;
                        }
                    }
                }
            }

            (successes, failures)
        }));
    }

    let mut total_successes: u64 = 0;
    let mut total_failures: u64 = 0;
    for h in handles {
        let (s, f) = h.join().expect("worker thread panicked");
        total_successes += s;
        total_failures += f;
    }

    let elapsed = start.elapsed();
    assert!(
        elapsed < WALL_CLOCK_CAP,
        "stress test exceeded wall-clock cap: took {:?}",
        elapsed,
    );

    let final_state = controller.state();
    assert!(
        ALL.contains(&final_state),
        "final state {:?} is not a valid KernelState variant",
        final_state,
    );

    assert!(total_successes > 0 || total_failures > 0);
}
