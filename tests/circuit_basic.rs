//! Integration tests for the circuit breaker (no supervisor).

#![allow(clippy::unwrap_used)]

use std::thread;
use std::time::Duration;

use service_kernel::worker::{CircuitBreaker, CircuitPolicy, CircuitState};

#[test]
fn test_default_policy_values() {
    let p = CircuitPolicy::default();
    assert_eq!(p.failure_threshold, 3);
    assert_eq!(p.failure_window, Duration::from_secs(60));
    assert_eq!(p.open_duration, Duration::from_secs(30));
}

#[test]
fn test_breaker_round_trip_closed_open_halfopen_closed() {
    let breaker = CircuitBreaker::new(CircuitPolicy::new(
        2,
        Duration::from_secs(60),
        Duration::from_millis(20),
    ));

    assert_eq!(breaker.state(), CircuitState::Closed);
    assert!(breaker.allow());
    assert_eq!(breaker.record_failure(), CircuitState::Closed);
    assert_eq!(breaker.record_failure(), CircuitState::Open);
    assert!(!breaker.allow());

    thread::sleep(Duration::from_millis(30));
    assert_eq!(breaker.tick(), CircuitState::HalfOpen);
    assert!(breaker.allow());

    assert_eq!(breaker.record_success(), CircuitState::Closed);
    assert!(breaker.allow());
}

#[test]
fn test_halfopen_failure_returns_to_open() {
    let breaker = CircuitBreaker::new(CircuitPolicy::new(
        1,
        Duration::from_secs(60),
        Duration::from_millis(10),
    ));
    let _ = breaker.record_failure();
    thread::sleep(Duration::from_millis(20));
    assert_eq!(breaker.tick(), CircuitState::HalfOpen);
    assert_eq!(breaker.record_failure(), CircuitState::Open);
}
