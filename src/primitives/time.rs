//! Kernel-owned time primitives.
//!
//! The rest of the kernel routes monotonic time, deadlines, and
//! periodic intervals through these types instead of touching
//! `std::time::*` directly. The wrapper layer exists so the kernel
//! can evolve its time model — adding mocking hooks, switching to a
//! different backing clock, or layering tracing — without breaking
//! consumer signatures.
//!
//! This module is std-only by design. The Tokio interval primitive
//! lives in `worker::watchdog`, where it belongs alongside the
//! supervisor that drives it.

use std::fmt;
use std::time::Duration;

/// A monotonic point in time.
///
/// Wraps [`std::time::Instant`]. The wrapper preserves the
/// underlying total ordering and hash invariants and adds nothing
/// of its own beyond a stable name.
///
/// # Examples
///
/// ```
/// use service_kernel::primitives::Instant;
/// use std::time::Duration;
///
/// let start = Instant::now();
/// let later = start.checked_add(Duration::from_millis(5)).unwrap();
/// assert!(later > start);
/// ```
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Instant(std::time::Instant);

impl Instant {
    /// Returns the current monotonic time.
    #[inline]
    #[must_use]
    pub fn now() -> Self {
        Self(std::time::Instant::now())
    }

    /// Returns the time elapsed since `self`.
    ///
    /// This is `Instant::now().duration_since(self)`.
    #[inline]
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }

    /// Returns the duration from `earlier` to `self`.
    ///
    /// Saturates at zero if `earlier` is later than `self`, matching
    /// [`std::time::Instant::duration_since`].
    #[inline]
    #[must_use]
    pub fn duration_since(&self, earlier: Instant) -> Duration {
        self.0.duration_since(earlier.0)
    }

    /// Returns `Some(self + duration)`, or `None` on overflow.
    #[inline]
    #[must_use]
    pub fn checked_add(&self, duration: Duration) -> Option<Instant> {
        self.0.checked_add(duration).map(Instant)
    }

    /// Returns `Some(self - duration)`, or `None` if the result
    /// would precede the underlying clock's epoch.
    #[inline]
    #[must_use]
    pub fn checked_sub(&self, duration: Duration) -> Option<Instant> {
        self.0.checked_sub(duration).map(Instant)
    }
}

impl fmt::Debug for Instant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Instant").field(&self.0).finish()
    }
}

/// A monotonic point in time with expiry semantics.
///
/// A `Deadline` is "an [`Instant`], plus the question 'has it
/// passed yet?'". Constructors accept either an absolute instant
/// ([`Deadline::new`]) or a duration from the current moment
/// ([`Deadline::from_now`]).
///
/// # Examples
///
/// ```
/// use service_kernel::primitives::Deadline;
/// use std::time::Duration;
///
/// let d = Deadline::from_now(Duration::from_secs(60));
/// assert!(!d.is_expired());
/// assert!(d.remaining().is_some());
/// ```
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Deadline {
    instant: Instant,
}

impl Deadline {
    /// Creates a deadline at the given instant.
    #[inline]
    #[must_use]
    pub const fn new(at: Instant) -> Self {
        Self { instant: at }
    }

    /// Creates a deadline `duration` from now.
    ///
    /// If `duration` overflows the underlying clock the deadline is
    /// pinned to its source instant; subsequent `is_expired` calls
    /// will simply return `false` until the clock catches up. This
    /// matches `std`'s saturating semantics for unrepresentable
    /// instants and avoids an `Option` return on the constructor.
    #[inline]
    #[must_use]
    pub fn from_now(duration: Duration) -> Self {
        let now = Instant::now();
        let instant = now.checked_add(duration).unwrap_or(now);
        Self { instant }
    }

    /// Returns the underlying instant at which the deadline fires.
    #[inline]
    #[must_use]
    pub const fn instant(&self) -> Instant {
        self.instant
    }

    /// Returns `true` once the deadline has been reached or passed.
    #[inline]
    #[must_use]
    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.instant
    }

    /// Returns the time left before the deadline, or `None` if it
    /// has already expired.
    #[inline]
    #[must_use]
    pub fn remaining(&self) -> Option<Duration> {
        let now = Instant::now();
        if now >= self.instant {
            None
        } else {
            Some(self.instant.duration_since(now))
        }
    }
}

/// A configured periodic interval.
///
/// `Interval` is a value type — it carries the period and nothing
/// else. The actual ticking happens elsewhere (the watchdog owns a
/// Tokio interval driven by this period). Keeping the period and
/// the runtime separate keeps `primitives::time` runtime-agnostic.
///
/// # Examples
///
/// ```
/// use service_kernel::primitives::Interval;
/// use std::time::Duration;
///
/// let watchdog_period = Interval::new(Duration::from_secs(1));
/// assert_eq!(watchdog_period.period(), Duration::from_secs(1));
/// ```
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Interval {
    period: Duration,
}

impl Interval {
    /// Creates an interval with the given period.
    #[inline]
    #[must_use]
    pub const fn new(period: Duration) -> Self {
        Self { period }
    }

    /// Returns the configured period.
    #[inline]
    #[must_use]
    pub const fn period(&self) -> Duration {
        self.period
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_instant_now_increases_across_calls() {
        let a = Instant::now();
        thread::sleep(Duration::from_millis(2));
        let b = Instant::now();
        assert!(b > a);
    }

    #[test]
    fn test_instant_elapsed_is_non_negative() {
        let a = Instant::now();
        let _ = a.elapsed();
    }

    #[test]
    fn test_instant_duration_since_is_close_to_sleep() {
        let a = Instant::now();
        thread::sleep(Duration::from_millis(10));
        let b = Instant::now();
        let gap = b.duration_since(a);
        assert!(gap >= Duration::from_millis(8));
    }

    #[test]
    fn test_instant_checked_add_returns_some_for_normal_durations() {
        let a = Instant::now();
        assert!(a.checked_add(Duration::from_secs(1)).is_some());
    }

    #[test]
    fn test_instant_checked_add_returns_none_on_overflow() {
        let a = Instant::now();
        assert!(a.checked_add(Duration::MAX).is_none());
    }

    #[test]
    fn test_instant_checked_sub_returns_some_for_normal_durations() {
        let a = Instant::now();
        assert!(a.checked_sub(Duration::from_millis(1)).is_some());
    }

    #[test]
    fn test_instant_checked_sub_returns_none_when_before_epoch() {
        let a = Instant::now();
        assert!(a.checked_sub(Duration::MAX).is_none());
    }

    #[test]
    fn test_instant_ord_orders_chronologically() {
        let a = Instant::now();
        thread::sleep(Duration::from_millis(2));
        let b = Instant::now();
        assert!(a < b);
        assert!(b > a);
    }

    #[test]
    fn test_deadline_from_zero_is_immediately_expired() {
        let d = Deadline::from_now(Duration::ZERO);
        assert!(d.is_expired());
        assert!(d.remaining().is_none());
    }

    #[test]
    fn test_deadline_from_huge_is_not_expired() {
        let d = Deadline::from_now(Duration::from_secs(3600));
        assert!(!d.is_expired());
        let remaining = d.remaining().unwrap();
        assert!(remaining > Duration::from_secs(3590));
        assert!(remaining <= Duration::from_secs(3600));
    }

    #[test]
    fn test_deadline_is_expired_consistency() {
        let d = Deadline::from_now(Duration::from_millis(1));
        thread::sleep(Duration::from_millis(5));
        assert!(d.is_expired());
        assert!(d.remaining().is_none());
    }

    #[test]
    fn test_deadline_new_from_explicit_instant() {
        let now = Instant::now();
        let d = Deadline::new(now.checked_add(Duration::from_secs(30)).unwrap());
        assert!(!d.is_expired());
        assert_eq!(
            d.instant(),
            now.checked_add(Duration::from_secs(30)).unwrap()
        );
    }

    #[test]
    fn test_interval_period_round_trips() {
        let i = Interval::new(Duration::from_millis(250));
        assert_eq!(i.period(), Duration::from_millis(250));
    }
}
