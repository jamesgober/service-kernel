//! Hung-worker detection.
//!
//! [`Watchdog`] is the supervisor's single periodic timer. The
//! supervisor's `tokio::select!` loop awaits [`Watchdog::tick`] on
//! each iteration; on tick, it builds a [`WatchdogTarget`] list
//! from registered workers and asks [`Watchdog::check`] to identify
//! workers that have stopped heartbeating.
//!
//! The watchdog applies a **2× grace factor**: a worker is reported
//! as a timeout only after it has been silent for longer than
//! `2 × heartbeat_interval`. Workers that are slightly slow do not
//! count; the threshold catches workers that are fully unresponsive.

use std::time::Duration;

use tokio::time::{interval, Interval, MissedTickBehavior};

use crate::primitives::WorkerId;

/// Default tick period when the spec does not override.
pub const DEFAULT_TICK: Duration = Duration::from_secs(1);

/// Single periodic interval driving liveness checks.
pub struct Watchdog {
    interval: Interval,
}

impl Watchdog {
    /// Constructs a watchdog with the given tick period.
    ///
    /// Missed ticks are skipped (the watchdog never tries to "catch
    /// up" by firing rapidly after a stall).
    #[must_use]
    pub fn new(period: Duration) -> Self {
        let mut iv = interval(period);
        iv.set_missed_tick_behavior(MissedTickBehavior::Delay);
        Self { interval: iv }
    }

    /// Awaits the next tick. Returns the tick instant.
    pub async fn tick(&mut self) -> tokio::time::Instant {
        self.interval.tick().await
    }

    /// Inspects a list of targets at `now_nanos` (current Unix nanos)
    /// and returns the workers silent for more than 2× their
    /// configured heartbeat interval.
    ///
    /// Pure function — does not touch the OS clock; the caller
    /// supplies `now_nanos`. This makes the function trivially
    /// testable without time mocking.
    ///
    /// Workers that have never heartbeated (`last_heartbeat_nanos == 0`)
    /// are skipped: they have not had a chance to start yet.
    #[must_use]
    pub fn check(targets: &[WatchdogTarget], now_nanos: i64) -> Vec<WatchdogTimeout> {
        let mut out = Vec::new();
        for target in targets {
            if target.last_heartbeat_nanos <= 0 {
                continue;
            }
            let interval_nanos = i128::from(target.heartbeat_interval.as_nanos() as i64);
            let threshold = interval_nanos.saturating_mul(2);
            let silent_nanos =
                i128::from(now_nanos).saturating_sub(i128::from(target.last_heartbeat_nanos));
            if silent_nanos > threshold {
                let silent_for = u64_from_nanos(silent_nanos);
                out.push(WatchdogTimeout {
                    id: target.id,
                    name: target.name,
                    silent_for: Duration::from_nanos(silent_for),
                });
            }
        }
        out
    }
}

fn u64_from_nanos(n: i128) -> u64 {
    if n < 0 {
        0
    } else if n > u64::MAX as i128 {
        u64::MAX
    } else {
        n as u64
    }
}

/// One worker the watchdog should inspect on each tick.
///
/// Built from the supervisor's `RegisteredWorker` state. Only
/// workers with a configured heartbeat interval should be included
/// — others are ignored entirely (no heartbeat means no expectation).
#[derive(Debug, Clone, Copy)]
pub struct WatchdogTarget {
    /// Worker identifier.
    pub id: WorkerId,
    /// Worker name (for events and metrics labels).
    pub name: &'static str,
    /// Last observed heartbeat in Unix nanos. Zero means never
    /// heartbeated.
    pub last_heartbeat_nanos: i64,
    /// Configured heartbeat interval; the worker is expected to call
    /// `ctx.heartbeat()` at least this often.
    pub heartbeat_interval: Duration,
}

/// One timeout reported by the watchdog.
#[derive(Debug, Clone, Copy)]
pub struct WatchdogTimeout {
    /// Identifier of the worker that timed out.
    pub id: WorkerId,
    /// Worker name.
    pub name: &'static str,
    /// How long the worker has been silent.
    pub silent_for: Duration,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::primitives::IdGenerator;

    fn target(id: WorkerId, last_nanos: i64, interval: Duration) -> WatchdogTarget {
        WatchdogTarget {
            id,
            name: "w",
            last_heartbeat_nanos: last_nanos,
            heartbeat_interval: interval,
        }
    }

    #[test]
    fn test_check_with_no_targets_returns_empty() {
        assert!(Watchdog::check(&[], 1_000_000_000).is_empty());
    }

    #[test]
    fn test_check_within_grace_returns_empty() {
        let id = IdGenerator::new().next_worker_id();
        // last heartbeat at 0ns, now at 150ms (1.5x of 100ms interval -> within grace)
        let t = target(id, 1, Duration::from_millis(100));
        let now = 150_000_000;
        let timeouts = Watchdog::check(&[t], now);
        assert!(timeouts.is_empty());
    }

    #[test]
    fn test_check_past_grace_returns_timeout() {
        let id = IdGenerator::new().next_worker_id();
        let t = target(id, 1, Duration::from_millis(100));
        // 250ms silent > 2 × 100ms = 200ms
        let now = 250_000_000;
        let timeouts = Watchdog::check(&[t], now);
        assert_eq!(timeouts.len(), 1);
        assert_eq!(timeouts[0].id, id);
    }

    #[test]
    fn test_check_skips_never_heartbeated() {
        let id = IdGenerator::new().next_worker_id();
        let t = target(id, 0, Duration::from_millis(100));
        let now = 1_000_000_000;
        assert!(Watchdog::check(&[t], now).is_empty());
    }

    #[test]
    fn test_check_handles_many_targets() {
        let id_gen = IdGenerator::new();
        let now = 1_000_000_000_i64;
        let mut targets = Vec::with_capacity(1000);
        for i in 0..1000 {
            let last = if i % 2 == 0 { 1 } else { now - 100 };
            targets.push(target(
                id_gen.next_worker_id(),
                last,
                Duration::from_millis(100),
            ));
        }
        let timeouts = Watchdog::check(&targets, now);
        // Half the workers timed out (those with last=1, 1B nanos behind).
        assert_eq!(timeouts.len(), 500);
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn test_tick_advances_with_period() {
        let mut w = Watchdog::new(Duration::from_millis(50));
        let _ = w.tick().await;
        let _ = w.tick().await;
    }
}
