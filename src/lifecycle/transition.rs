//! Legal kernel-state transitions.
//!
//! The legal-transition rules for [`KernelState`] live here as a
//! single constant 8×8 matrix. Code and documentation cannot drift:
//! both [`is_legal`] and [`assert_legal`] read the same table, and
//! the table is exhaustively verified by unit tests.
//!
//! Legal transitions:
//!
//! ```text
//! Created  -> Booting
//! Booting  -> Loading | Failed
//! Loading  -> Running | Failed
//! Running  -> Degraded | Stopping | Failed
//! Degraded -> Running | Stopping | Failed
//! Stopping -> Stopped | Failed
//! Stopped  -> (terminal)
//! Failed   -> (terminal)
//! ```
//!
//! Self-transitions (e.g. `Running -> Running`) are illegal.

use std::error::Error;
use std::fmt;

use lang_lib::t;

use super::KernelState;

/// Number of `KernelState` variants. Must match the discriminant
/// space `KernelState as u8` covers.
const N: usize = 8;

/// Returns the row index for a given [`KernelState`] in the
/// legal-transition matrix. The values match the variant's
/// declaration order (and its `#[repr(u8)]` discriminant).
#[inline]
const fn idx(state: KernelState) -> usize {
    state as usize
}

/// Constant legal-transition matrix.
///
/// `LEGAL_TRANSITIONS[from][to]` is `true` iff the move is allowed.
/// The matrix is 8×8 with one row per source state, declared in
/// declaration order to make the table readable next to the
/// [`KernelState`] enum.
const LEGAL_TRANSITIONS: [[bool; N]; N] = {
    let mut t = [[false; N]; N];

    // Created -> Booting
    t[idx(KernelState::Created)][idx(KernelState::Booting)] = true;

    // Booting -> Loading | Failed
    t[idx(KernelState::Booting)][idx(KernelState::Loading)] = true;
    t[idx(KernelState::Booting)][idx(KernelState::Failed)] = true;

    // Loading -> Running | Failed
    t[idx(KernelState::Loading)][idx(KernelState::Running)] = true;
    t[idx(KernelState::Loading)][idx(KernelState::Failed)] = true;

    // Running -> Degraded | Stopping | Failed
    t[idx(KernelState::Running)][idx(KernelState::Degraded)] = true;
    t[idx(KernelState::Running)][idx(KernelState::Stopping)] = true;
    t[idx(KernelState::Running)][idx(KernelState::Failed)] = true;

    // Degraded -> Running | Stopping | Failed
    t[idx(KernelState::Degraded)][idx(KernelState::Running)] = true;
    t[idx(KernelState::Degraded)][idx(KernelState::Stopping)] = true;
    t[idx(KernelState::Degraded)][idx(KernelState::Failed)] = true;

    // Stopping -> Stopped | Failed
    t[idx(KernelState::Stopping)][idx(KernelState::Stopped)] = true;
    t[idx(KernelState::Stopping)][idx(KernelState::Failed)] = true;

    // Stopped -> (terminal)
    // Failed  -> (terminal)

    t
};

/// Returns `true` if `from -> to` is a legal kernel-state transition.
///
/// Self-transitions are illegal. Terminal states ([`KernelState::Stopped`],
/// [`KernelState::Failed`]) accept no outgoing transition.
///
/// # Examples
///
/// ```
/// use service_kernel::lifecycle::{is_legal, KernelState};
///
/// assert!(is_legal(KernelState::Created, KernelState::Booting));
/// assert!(!is_legal(KernelState::Created, KernelState::Running));
/// assert!(!is_legal(KernelState::Stopped, KernelState::Running));
/// ```
#[inline]
#[must_use]
pub fn is_legal(from: KernelState, to: KernelState) -> bool {
    LEGAL_TRANSITIONS[idx(from)][idx(to)]
}

/// Returns `Ok(())` if the transition is legal, otherwise a
/// [`TransitionError`] describing the rejected move.
///
/// # Errors
///
/// Returns [`TransitionError`] when `is_legal(from, to)` is `false`.
///
/// # Examples
///
/// ```
/// use service_kernel::lifecycle::{assert_legal, KernelState};
///
/// assert!(assert_legal(KernelState::Created, KernelState::Booting).is_ok());
/// assert!(assert_legal(KernelState::Created, KernelState::Running).is_err());
/// ```
#[inline]
pub fn assert_legal(from: KernelState, to: KernelState) -> Result<(), TransitionError> {
    if is_legal(from, to) {
        Ok(())
    } else {
        Err(TransitionError { from, to })
    }
}

/// An attempted state transition that the legal-transition table
/// rejected.
///
/// Returned by [`assert_legal`] and by
/// [`LifecycleController::transition`](super::LifecycleController::transition).
///
/// `Display` routes its message through [`lang_lib::t!`] under the
/// key `kernel.lifecycle.transition.illegal`. The `from` and `to`
/// values are appended after the translated prefix so operators can
/// see exactly which move was rejected, regardless of locale.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct TransitionError {
    /// The state the kernel was in.
    pub from: KernelState,
    /// The state that was requested.
    pub to: KernelState,
}

impl fmt::Display for TransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = t!(
            "kernel.lifecycle.transition.illegal",
            fallback: "illegal lifecycle transition"
        );
        write!(f, "{}: {} -> {}", prefix, self.from, self.to)
    }
}

impl Error for TransitionError {}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const ALL: [KernelState; N] = [
        KernelState::Created,
        KernelState::Booting,
        KernelState::Loading,
        KernelState::Running,
        KernelState::Degraded,
        KernelState::Stopping,
        KernelState::Stopped,
        KernelState::Failed,
    ];

    fn expected(from: KernelState, to: KernelState) -> bool {
        use KernelState::{
            Booting, Created, Degraded, Failed, Loading, Running, Stopped, Stopping,
        };
        matches!(
            (from, to),
            (Created, Booting)
                | (Booting, Loading)
                | (Booting, Failed)
                | (Loading, Running)
                | (Loading, Failed)
                | (Running, Degraded)
                | (Running, Stopping)
                | (Running, Failed)
                | (Degraded, Running)
                | (Degraded, Stopping)
                | (Degraded, Failed)
                | (Stopping, Stopped)
                | (Stopping, Failed)
        )
    }

    #[test]
    fn test_is_legal_matches_expected_for_every_cell() {
        for &from in ALL.iter() {
            for &to in ALL.iter() {
                assert_eq!(
                    is_legal(from, to),
                    expected(from, to),
                    "is_legal({:?}, {:?})",
                    from,
                    to,
                );
            }
        }
    }

    #[test]
    fn test_assert_legal_returns_ok_for_every_legal_pair() {
        for &from in ALL.iter() {
            for &to in ALL.iter() {
                if expected(from, to) {
                    assert!(assert_legal(from, to).is_ok());
                }
            }
        }
    }

    #[test]
    fn test_assert_legal_returns_err_for_every_illegal_pair() {
        for &from in ALL.iter() {
            for &to in ALL.iter() {
                if !expected(from, to) {
                    let err = assert_legal(from, to).unwrap_err();
                    assert_eq!(err.from, from);
                    assert_eq!(err.to, to);
                }
            }
        }
    }

    #[test]
    fn test_transition_error_display_is_non_empty() {
        let err = TransitionError {
            from: KernelState::Created,
            to: KernelState::Running,
        };
        let rendered = err.to_string();
        assert!(!rendered.is_empty());
        assert!(rendered.contains("CREATED"));
        assert!(rendered.contains("RUNNING"));
    }

    #[test]
    fn test_transition_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TransitionError>();
    }

    #[test]
    fn test_self_transitions_are_illegal() {
        for state in ALL {
            assert!(!is_legal(state, state), "{:?} -> {:?}", state, state);
        }
    }

    #[test]
    fn test_terminal_states_have_no_outgoing_transitions() {
        for terminal in [KernelState::Stopped, KernelState::Failed] {
            for &target in ALL.iter() {
                assert!(
                    !is_legal(terminal, target),
                    "terminal {:?} accepted -> {:?}",
                    terminal,
                    target,
                );
            }
        }
    }

    #[test]
    fn test_transition_error_is_std_error() {
        let err = TransitionError {
            from: KernelState::Created,
            to: KernelState::Running,
        };
        let as_error: &dyn Error = &err;
        assert!(as_error.source().is_none());
    }
}
