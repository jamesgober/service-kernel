//! Integration tests for `service_kernel::lifecycle`.
//!
//! Walks the controller through realistic transition sequences to
//! verify the public API surface from outside the crate.

#![allow(clippy::unwrap_used)]

use service_kernel::lifecycle::{assert_legal, is_legal, KernelState, LifecycleController, Phase};

#[test]
fn test_full_happy_path() {
    let c = LifecycleController::new();
    assert_eq!(c.state(), KernelState::Created);

    for next in [
        KernelState::Booting,
        KernelState::Loading,
        KernelState::Running,
        KernelState::Stopping,
        KernelState::Stopped,
    ] {
        c.transition(next).unwrap();
    }

    let snap = c.snapshot();
    assert_eq!(snap.state, KernelState::Stopped);
    assert_eq!(snap.phase, Phase::Shutdown);
}

#[test]
fn test_failure_path() {
    let c = LifecycleController::new();
    c.transition(KernelState::Booting).unwrap();
    c.transition(KernelState::Failed).unwrap();
    assert!(c.state().is_terminal());
}

#[test]
fn test_degraded_recovery_path() {
    let c = LifecycleController::new();
    for next in [
        KernelState::Booting,
        KernelState::Loading,
        KernelState::Running,
        KernelState::Degraded,
        KernelState::Running,
    ] {
        c.transition(next).unwrap();
    }
    assert_eq!(c.state(), KernelState::Running);
}

#[test]
fn test_illegal_transition_preserves_state_and_returns_error() {
    let c = LifecycleController::new();
    let err = c.transition(KernelState::Running).unwrap_err();
    assert_eq!(err.from, KernelState::Created);
    assert_eq!(err.to, KernelState::Running);
    assert_eq!(c.state(), KernelState::Created);
}

#[test]
fn test_terminal_state_rejects_all_outgoing_transitions() {
    let c = LifecycleController::new();
    c.transition(KernelState::Booting).unwrap();
    c.transition(KernelState::Loading).unwrap();
    c.transition(KernelState::Running).unwrap();
    c.transition(KernelState::Stopping).unwrap();
    c.transition(KernelState::Stopped).unwrap();

    for state in [
        KernelState::Created,
        KernelState::Booting,
        KernelState::Loading,
        KernelState::Running,
        KernelState::Degraded,
        KernelState::Stopping,
        KernelState::Stopped,
        KernelState::Failed,
    ] {
        assert!(c.transition(state).is_err());
    }
}

#[test]
fn test_pure_validators_match_controller_behavior() {
    let c = LifecycleController::new();
    let from = c.state();
    for to in [
        KernelState::Booting,
        KernelState::Running,
        KernelState::Stopped,
    ] {
        let pure = is_legal(from, to);
        let controller = LifecycleController::new().transition(to).is_ok();
        assert_eq!(pure, controller, "{:?} -> {:?}", from, to);

        let assert = assert_legal(from, to);
        assert_eq!(assert.is_ok(), pure);
    }
}

#[test]
fn test_snapshot_carries_phase_and_state() {
    let c = LifecycleController::new();
    let s0 = c.snapshot();
    assert_eq!(s0.state, KernelState::Created);
    assert_eq!(s0.phase, Phase::Idle);

    c.transition(KernelState::Booting).unwrap();
    let s1 = c.snapshot();
    assert_eq!(s1.state, KernelState::Booting);
    assert_eq!(s1.phase, Phase::Boot);
    assert!(s1.last_transition >= s0.last_transition);
}
