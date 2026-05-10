//! Restart-backoff policy.
//!
//! [`BackoffPolicy::delay`] returns the wait duration before the
//! next restart attempt. The exponential variant uses saturating
//! arithmetic so a runaway `attempt` counter cannot overflow into a
//! short delay.

use std::time::Duration;

/// Backoff-policy variants.
///
/// Marked `#[non_exhaustive]` for SemVer-stable additions.
#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum BackoffPolicy {
    /// No delay between attempts.
    None,
    /// Constant delay.
    Fixed(Duration),
    /// Exponential growth, saturating at `max`.
    Exponential {
        /// First-attempt delay (`attempt = 0`).
        base: Duration,
        /// Upper cap; the returned delay never exceeds this.
        max: Duration,
    },
}

impl BackoffPolicy {
    /// Returns the delay before attempt number `attempt` (0-based).
    ///
    /// Saturating arithmetic — a huge `attempt` cannot wrap.
    #[inline]
    #[must_use]
    pub fn delay(&self, attempt: u32) -> Duration {
        match self {
            BackoffPolicy::None => Duration::ZERO,
            BackoffPolicy::Fixed(d) => *d,
            BackoffPolicy::Exponential { base, max } => {
                let base_nanos = base.as_nanos();
                let cap = max.as_nanos();
                if base_nanos == 0 || cap == 0 {
                    return Duration::ZERO;
                }
                // Cap shift exponent at 63 — any larger and the shift
                // is well past saturation anyway.
                let shift = attempt.min(63);
                let scaled = base_nanos.saturating_mul(1_u128 << shift);
                let chosen = scaled.min(cap);
                let secs = (chosen / 1_000_000_000) as u64;
                let nanos = (chosen % 1_000_000_000) as u32;
                Duration::new(secs, nanos)
            }
        }
    }
}

impl Default for BackoffPolicy {
    /// Exponential backoff: 100 ms base, 30 s cap.
    #[inline]
    fn default() -> Self {
        BackoffPolicy::Exponential {
            base: Duration::from_millis(100),
            max: Duration::from_secs(30),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_none_returns_zero_duration() {
        assert_eq!(BackoffPolicy::None.delay(0), Duration::ZERO);
        assert_eq!(BackoffPolicy::None.delay(99), Duration::ZERO);
    }

    #[test]
    fn test_fixed_returns_constant_duration() {
        let p = BackoffPolicy::Fixed(Duration::from_millis(250));
        assert_eq!(p.delay(0), Duration::from_millis(250));
        assert_eq!(p.delay(99), Duration::from_millis(250));
    }

    #[test]
    fn test_exponential_at_attempt_zero_is_base() {
        let p = BackoffPolicy::Exponential {
            base: Duration::from_millis(100),
            max: Duration::from_secs(30),
        };
        assert_eq!(p.delay(0), Duration::from_millis(100));
    }

    #[test]
    fn test_exponential_doubles_each_attempt() {
        let p = BackoffPolicy::Exponential {
            base: Duration::from_millis(100),
            max: Duration::from_secs(60),
        };
        assert_eq!(p.delay(1), Duration::from_millis(200));
        assert_eq!(p.delay(2), Duration::from_millis(400));
        assert_eq!(p.delay(5), Duration::from_millis(3200));
    }

    #[test]
    fn test_exponential_saturates_at_max() {
        let p = BackoffPolicy::Exponential {
            base: Duration::from_millis(100),
            max: Duration::from_secs(1),
        };
        // 100ms * 2^4 = 1600ms > 1s cap, so we get 1s.
        assert_eq!(p.delay(4), Duration::from_secs(1));
        // Far past saturation point.
        assert_eq!(p.delay(64), Duration::from_secs(1));
        assert_eq!(p.delay(u32::MAX), Duration::from_secs(1));
    }

    #[test]
    fn test_default_is_exponential_100ms_30s() {
        assert_eq!(
            BackoffPolicy::default(),
            BackoffPolicy::Exponential {
                base: Duration::from_millis(100),
                max: Duration::from_secs(30),
            }
        );
    }
}
