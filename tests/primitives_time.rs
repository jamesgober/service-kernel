//! Integration tests for `service_kernel::primitives::time`.

#![allow(clippy::unwrap_used)]

use std::thread::sleep;
use std::time::Duration;

use service_kernel::primitives::{Deadline, Instant, Interval};

#[test]
fn test_instant_now_advances_after_sleep() {
    let a = Instant::now();
    sleep(Duration::from_millis(5));
    let b = Instant::now();
    assert!(b > a);
}

#[test]
fn test_instant_checked_add_overflow_returns_none() {
    let a = Instant::now();
    assert!(a.checked_add(Duration::MAX).is_none());
}

#[test]
fn test_deadline_zero_is_immediately_expired() {
    let d = Deadline::from_now(Duration::ZERO);
    assert!(d.is_expired());
    assert!(d.remaining().is_none());
}

#[test]
fn test_deadline_long_returns_remaining() {
    let d = Deadline::from_now(Duration::from_secs(10));
    let remaining = d.remaining().unwrap();
    assert!(remaining > Duration::from_secs(9));
    assert!(remaining <= Duration::from_secs(10));
}

#[test]
fn test_deadline_explicit_instant_round_trips() {
    let now = Instant::now();
    let target = now.checked_add(Duration::from_millis(50)).unwrap();
    let d = Deadline::new(target);
    assert_eq!(d.instant(), target);
}

#[test]
fn test_interval_value_round_trip() {
    let i = Interval::new(Duration::from_millis(250));
    assert_eq!(i.period(), Duration::from_millis(250));
}
