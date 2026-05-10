//! Circuit-breaker policy and state machine.
//!
//! [`CircuitBreaker`] is per-worker repeated-failure containment. The
//! breaker counts failures inside a sliding window and, on crossing
//! the threshold, transitions to [`CircuitState::Open`] — refusing
//! further restart attempts. A periodic [`CircuitBreaker::tick`] (driven
//! by the Tokio-gated `crate::worker::Watchdog`) advances
//! `Open → HalfOpen` after the open duration elapses; a single
//! trial run from `HalfOpen` either closes the breaker on success or
//! re-opens it on failure.
//!
//! State transitions:
//!
//! ```text
//!         record_failure (over threshold)
//!           ┌─────────────────────────────┐
//!           ▼                             │
//!  ┌──────────────┐                       │
//!  │   Closed     │                       │
//!  └──────┬───────┘                       │
//!         │ record_success                │
//!         │ (resets counter)              │
//!         ▲                               │
//!         │                               │
//!  ┌──────┴───────┐    tick (after open   │
//!  │  HalfOpen    │◄──── duration)─────── Open
//!  └──────┬───────┘                       │
//!         │ record_failure                │
//!         └───────────────────────────────┘
//! ```

use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Circuit-breaker state.
#[non_exhaustive]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
#[repr(u8)]
pub enum CircuitState {
    /// Normal operation; failures count toward the threshold.
    Closed = 0,
    /// Threshold exceeded; restarts are suppressed.
    Open = 1,
    /// Recovery trial; one attempt is permitted.
    HalfOpen = 2,
}

impl CircuitState {
    #[inline]
    fn from_u8(byte: u8) -> Self {
        match byte {
            1 => CircuitState::Open,
            2 => CircuitState::HalfOpen,
            _ => CircuitState::Closed,
        }
    }
}

impl Default for CircuitState {
    #[inline]
    fn default() -> Self {
        CircuitState::Closed
    }
}

/// Circuit-breaker configuration.
///
/// # Examples
///
/// ```
/// use service_kernel::worker::CircuitPolicy;
/// use std::time::Duration;
///
/// let p = CircuitPolicy::default();
/// assert_eq!(p.failure_threshold, 3);
/// assert_eq!(p.failure_window, Duration::from_secs(60));
/// assert_eq!(p.open_duration, Duration::from_secs(30));
/// ```
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct CircuitPolicy {
    /// Number of failures within `failure_window` that trips the breaker.
    pub failure_threshold: u32,
    /// Sliding window over which failures accumulate.
    pub failure_window: Duration,
    /// How long the breaker stays Open before promoting to HalfOpen.
    pub open_duration: Duration,
}

impl CircuitPolicy {
    /// Constructs a policy with explicit values.
    #[inline]
    #[must_use]
    pub fn new(failure_threshold: u32, failure_window: Duration, open_duration: Duration) -> Self {
        Self {
            failure_threshold,
            failure_window,
            open_duration,
        }
    }
}

impl Default for CircuitPolicy {
    /// Returns the kernel's default policy: threshold 3, 60-second
    /// window, 30-second open duration.
    #[inline]
    fn default() -> Self {
        Self {
            failure_threshold: 3,
            failure_window: Duration::from_secs(60),
            open_duration: Duration::from_secs(30),
        }
    }
}

/// Per-worker circuit breaker.
///
/// `record_failure` and `record_success` advance the state machine.
/// `tick` is called periodically (by the watchdog) to advance Open
/// → HalfOpen. `allow` reports whether work should be attempted.
///
/// # Examples
///
/// ```
/// use service_kernel::worker::{CircuitBreaker, CircuitPolicy, CircuitState};
/// use std::time::Duration;
///
/// let breaker = CircuitBreaker::new(CircuitPolicy {
///     failure_threshold: 2,
///     failure_window: Duration::from_secs(60),
///     open_duration: Duration::from_millis(50),
/// });
/// assert_eq!(breaker.state(), CircuitState::Closed);
/// assert_eq!(breaker.record_failure(), CircuitState::Closed);
/// assert_eq!(breaker.record_failure(), CircuitState::Open);
/// assert!(!breaker.allow());
/// ```
#[derive(Debug)]
pub struct CircuitBreaker {
    state: AtomicU8,
    failures: AtomicU32,
    last_failure_at: Mutex<Option<Instant>>,
    opened_at: Mutex<Option<Instant>>,
    policy: CircuitPolicy,
}

impl CircuitBreaker {
    /// Constructs a closed breaker with the given policy.
    #[inline]
    #[must_use]
    pub fn new(policy: CircuitPolicy) -> Self {
        Self {
            state: AtomicU8::new(CircuitState::Closed as u8),
            failures: AtomicU32::new(0),
            last_failure_at: Mutex::new(None),
            opened_at: Mutex::new(None),
            policy,
        }
    }

    /// Returns the breaker's policy.
    #[inline]
    #[must_use]
    pub fn policy(&self) -> &CircuitPolicy {
        &self.policy
    }

    /// Returns the current state.
    #[inline]
    #[must_use]
    pub fn state(&self) -> CircuitState {
        CircuitState::from_u8(self.state.load(Ordering::Acquire))
    }

    /// Records a failure. Returns the state after recording.
    pub fn record_failure(&self) -> CircuitState {
        let now = Instant::now();
        let prior_state = self.state();

        match prior_state {
            CircuitState::HalfOpen => {
                // Trial failed — back to Open.
                self.state
                    .store(CircuitState::Open as u8, Ordering::Release);
                let mut opened = self
                    .opened_at
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                *opened = Some(now);
                CircuitState::Open
            }
            CircuitState::Open => CircuitState::Open,
            CircuitState::Closed => {
                // Reset counter if outside window, then increment.
                let in_window = {
                    let mut last = self
                        .last_failure_at
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let in_window = last
                        .map(|t| now.duration_since(t) <= self.policy.failure_window)
                        .unwrap_or(false);
                    *last = Some(now);
                    in_window
                };

                if !in_window {
                    self.failures.store(0, Ordering::Release);
                }
                let count = self.failures.fetch_add(1, Ordering::AcqRel) + 1;

                if count >= self.policy.failure_threshold {
                    self.state
                        .store(CircuitState::Open as u8, Ordering::Release);
                    let mut opened = self
                        .opened_at
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    *opened = Some(now);
                    CircuitState::Open
                } else {
                    CircuitState::Closed
                }
            }
        }
    }

    /// Records a success. Returns the state after recording.
    pub fn record_success(&self) -> CircuitState {
        self.failures.store(0, Ordering::Release);
        self.state
            .store(CircuitState::Closed as u8, Ordering::Release);
        let mut opened = self
            .opened_at
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *opened = None;
        let mut last = self
            .last_failure_at
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *last = None;
        CircuitState::Closed
    }

    /// Periodic tick — advances Open → HalfOpen if `open_duration`
    /// has elapsed. Returns the current state after the tick.
    pub fn tick(&self) -> CircuitState {
        if self.state() != CircuitState::Open {
            return self.state();
        }
        let now = Instant::now();
        let opened = {
            let guard = self
                .opened_at
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *guard
        };
        match opened {
            Some(t) if now.duration_since(t) >= self.policy.open_duration => {
                self.state
                    .store(CircuitState::HalfOpen as u8, Ordering::Release);
                CircuitState::HalfOpen
            }
            _ => CircuitState::Open,
        }
    }

    /// Returns `true` when work should be attempted.
    ///
    /// `false` only in the Open state.
    #[inline]
    #[must_use]
    pub fn allow(&self) -> bool {
        !matches!(self.state(), CircuitState::Open)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    fn breaker_with(threshold: u32, window: Duration, open: Duration) -> CircuitBreaker {
        CircuitBreaker::new(CircuitPolicy::new(threshold, window, open))
    }

    #[test]
    fn test_default_policy_values() {
        let p = CircuitPolicy::default();
        assert_eq!(p.failure_threshold, 3);
        assert_eq!(p.failure_window, Duration::from_secs(60));
        assert_eq!(p.open_duration, Duration::from_secs(30));
    }

    #[test]
    fn test_new_breaker_is_closed() {
        let b = breaker_with(3, Duration::from_secs(60), Duration::from_secs(30));
        assert_eq!(b.state(), CircuitState::Closed);
        assert!(b.allow());
    }

    #[test]
    fn test_failures_under_threshold_stay_closed() {
        let b = breaker_with(3, Duration::from_secs(60), Duration::from_secs(30));
        assert_eq!(b.record_failure(), CircuitState::Closed);
        assert_eq!(b.record_failure(), CircuitState::Closed);
        assert_eq!(b.state(), CircuitState::Closed);
    }

    #[test]
    fn test_failures_at_threshold_open_breaker() {
        let b = breaker_with(2, Duration::from_secs(60), Duration::from_secs(30));
        assert_eq!(b.record_failure(), CircuitState::Closed);
        assert_eq!(b.record_failure(), CircuitState::Open);
        assert!(!b.allow());
    }

    #[test]
    fn test_threshold_zero_opens_on_first_failure() {
        let b = breaker_with(0, Duration::from_secs(60), Duration::from_secs(30));
        assert_eq!(b.record_failure(), CircuitState::Open);
    }

    #[test]
    fn test_record_success_resets_to_closed() {
        let b = breaker_with(2, Duration::from_secs(60), Duration::from_secs(30));
        let _ = b.record_failure();
        let _ = b.record_failure();
        assert_eq!(b.state(), CircuitState::Open);
        assert_eq!(b.record_success(), CircuitState::Closed);
        assert!(b.allow());
    }

    #[test]
    fn test_tick_advances_open_to_halfopen_after_duration() {
        let b = breaker_with(1, Duration::from_secs(60), Duration::from_millis(20));
        assert_eq!(b.record_failure(), CircuitState::Open);
        thread::sleep(Duration::from_millis(30));
        assert_eq!(b.tick(), CircuitState::HalfOpen);
        assert!(b.allow());
    }

    #[test]
    fn test_tick_holds_open_before_duration() {
        let b = breaker_with(1, Duration::from_secs(60), Duration::from_secs(30));
        assert_eq!(b.record_failure(), CircuitState::Open);
        assert_eq!(b.tick(), CircuitState::Open);
    }

    #[test]
    fn test_halfopen_failure_returns_to_open() {
        let b = breaker_with(1, Duration::from_secs(60), Duration::from_millis(10));
        let _ = b.record_failure();
        thread::sleep(Duration::from_millis(20));
        assert_eq!(b.tick(), CircuitState::HalfOpen);
        assert_eq!(b.record_failure(), CircuitState::Open);
    }

    #[test]
    fn test_halfopen_success_returns_to_closed() {
        let b = breaker_with(1, Duration::from_secs(60), Duration::from_millis(10));
        let _ = b.record_failure();
        thread::sleep(Duration::from_millis(20));
        let _ = b.tick();
        assert_eq!(b.record_success(), CircuitState::Closed);
    }

    #[test]
    fn test_allow_per_state() {
        let b = breaker_with(1, Duration::from_secs(60), Duration::from_millis(10));
        assert!(b.allow()); // Closed
        let _ = b.record_failure();
        assert!(!b.allow()); // Open
        thread::sleep(Duration::from_millis(20));
        let _ = b.tick();
        assert!(b.allow()); // HalfOpen
    }

    #[test]
    fn test_concurrent_failures_count_correctly() {
        let b = Arc::new(breaker_with(
            50,
            Duration::from_secs(60),
            Duration::from_secs(30),
        ));
        let mut joins = Vec::new();
        for _ in 0..10 {
            let b = Arc::clone(&b);
            joins.push(thread::spawn(move || {
                for _ in 0..5 {
                    let _ = b.record_failure();
                }
            }));
        }
        for j in joins {
            j.join().unwrap();
        }
        // 50 failures total -> exactly threshold -> Open.
        assert_eq!(b.state(), CircuitState::Open);
    }

    #[test]
    fn test_stale_failures_outside_window_reset_counter() {
        let b = breaker_with(3, Duration::from_millis(10), Duration::from_secs(30));
        let _ = b.record_failure();
        let _ = b.record_failure();
        thread::sleep(Duration::from_millis(20));
        // Window elapsed; counter resets on next failure.
        assert_eq!(b.record_failure(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_state_default_is_closed() {
        assert_eq!(CircuitState::default(), CircuitState::Closed);
    }
}
