//! Integration tests for `service_kernel::primitives::Global`.
//!
//! These tests exercise the public API from outside the crate.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::thread;

use service_kernel::primitives::Global;

#[test]
fn test_global_set_then_get() {
    let g: Global<u32> = Global::new();
    assert!(g.get().is_none());
    assert!(!g.is_initialized());
    assert!(g.set(123).is_ok());
    assert!(g.is_initialized());
    assert_eq!(g.get().copied(), Some(123));
}

#[test]
fn test_global_second_set_returns_err_with_value() {
    let g: Global<u32> = Global::new();
    assert!(g.set(1).is_ok());
    assert_eq!(g.set(2), Err(2));
}

#[test]
fn test_global_try_set_matches_set() {
    let g: Global<&'static str> = Global::new();
    assert!(g.try_set("first").is_ok());
    assert_eq!(g.try_set("second"), Err("second"));
}

#[test]
fn test_global_get_unchecked_returns_value_after_set() {
    let g: Global<u32> = Global::new();
    let _ = g.set(7);
    assert_eq!(*g.get_unchecked(), 7);
}

#[test]
fn test_global_concurrent_set_exactly_one_winner() {
    let g: Arc<Global<usize>> = Arc::new(Global::new());
    let mut handles = Vec::new();
    for n in 0..16 {
        let g = Arc::clone(&g);
        handles.push(thread::spawn(move || g.set(n).is_ok()));
    }
    let wins: usize = handles
        .into_iter()
        .map(|h| usize::from(h.join().unwrap()))
        .sum();
    assert_eq!(wins, 1);
    assert!(g.is_initialized());
    assert!(g.get().copied().is_some_and(|v| v < 16));
}

#[test]
fn test_global_default_is_uninitialized() {
    let g: Global<u32> = Global::default();
    assert!(!g.is_initialized());
}
