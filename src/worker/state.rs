//! Worker state vocabulary.
//!
//! [`WorkerState`] is the supervisor's view of where a worker is in
//! its run cycle. Consumers see this through [`WorkerHandle::state`](super::WorkerHandle::state)
//! and per-worker `KernelSnapshot` entries.

use std::fmt;

/// State of a single worker.
///
/// Variants `Idle` and `Busy` are intended for future watchdog
/// integration (Milestone G); the supervisor in this milestone
/// transitions through `Created → Starting → Running → ... →
/// Stopped`.
///
/// Marked `#[non_exhaustive]` for SemVer-stable additions.
#[non_exhaustive]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
#[repr(u8)]
pub enum WorkerState {
    /// Created but not yet spawned.
    Created = 0,
    /// Spawn requested; setup running.
    Starting = 1,
    /// Active and processing.
    Running = 2,
    /// Active but waiting for input (Milestone G).
    Idle = 3,
    /// Active and processing a unit of work (Milestone G).
    Busy = 4,
    /// Last run returned an error.
    Failed = 5,
    /// Restart in progress per restart policy.
    Restarting = 6,
    /// Cancellation requested; cleanup running.
    Stopping = 7,
    /// Terminal — no more transitions.
    Stopped = 8,
}

impl WorkerState {
    /// Returns the lowercase variant name as a static string.
    #[inline]
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            WorkerState::Created => "created",
            WorkerState::Starting => "starting",
            WorkerState::Running => "running",
            WorkerState::Idle => "idle",
            WorkerState::Busy => "busy",
            WorkerState::Failed => "failed",
            WorkerState::Restarting => "restarting",
            WorkerState::Stopping => "stopping",
            WorkerState::Stopped => "stopped",
        }
    }

    /// Returns `true` when no further state changes can occur.
    ///
    /// Only [`WorkerState::Stopped`] is terminal; `Failed` is
    /// followed by either `Restarting → Running` or `Stopped`.
    #[inline]
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, WorkerState::Stopped)
    }

    /// Returns `true` when the worker is actively progressing.
    #[inline]
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(
            self,
            WorkerState::Running | WorkerState::Idle | WorkerState::Busy
        )
    }

    /// Reconstructs a [`WorkerState`] from its `u8` discriminant.
    ///
    /// Used by [`WorkerHandle`](super::WorkerHandle) to read the
    /// shared atomic state cell. Out-of-range values map to
    /// [`WorkerState::Stopped`] (the safest fallback).
    #[cfg(feature = "tokio")]
    #[inline]
    #[must_use]
    pub(crate) const fn from_u8(byte: u8) -> Self {
        match byte {
            0 => WorkerState::Created,
            1 => WorkerState::Starting,
            2 => WorkerState::Running,
            3 => WorkerState::Idle,
            4 => WorkerState::Busy,
            5 => WorkerState::Failed,
            6 => WorkerState::Restarting,
            7 => WorkerState::Stopping,
            _ => WorkerState::Stopped,
        }
    }
}

impl Default for WorkerState {
    #[inline]
    fn default() -> Self {
        WorkerState::Created
    }
}

impl fmt::Display for WorkerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            WorkerState::Created => "CREATED",
            WorkerState::Starting => "STARTING",
            WorkerState::Running => "RUNNING",
            WorkerState::Idle => "IDLE",
            WorkerState::Busy => "BUSY",
            WorkerState::Failed => "FAILED",
            WorkerState::Restarting => "RESTARTING",
            WorkerState::Stopping => "STOPPING",
            WorkerState::Stopped => "STOPPED",
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const ALL: [WorkerState; 9] = [
        WorkerState::Created,
        WorkerState::Starting,
        WorkerState::Running,
        WorkerState::Idle,
        WorkerState::Busy,
        WorkerState::Failed,
        WorkerState::Restarting,
        WorkerState::Stopping,
        WorkerState::Stopped,
    ];

    #[test]
    fn test_as_str_unique() {
        let mut set = HashSet::new();
        for s in ALL {
            assert!(set.insert(s.as_str()));
        }
    }

    #[test]
    fn test_is_terminal_only_stopped() {
        for s in ALL {
            assert_eq!(s.is_terminal(), matches!(s, WorkerState::Stopped));
        }
    }

    #[test]
    fn test_is_active_for_running_idle_busy() {
        for s in ALL {
            let expected = matches!(
                s,
                WorkerState::Running | WorkerState::Idle | WorkerState::Busy
            );
            assert_eq!(s.is_active(), expected);
        }
    }

    #[cfg(feature = "tokio")]
    #[test]
    fn test_from_u8_round_trips() {
        for s in ALL {
            assert_eq!(WorkerState::from_u8(s as u8), s);
        }
    }

    #[cfg(feature = "tokio")]
    #[test]
    fn test_from_u8_out_of_range_falls_back_to_stopped() {
        assert_eq!(WorkerState::from_u8(255), WorkerState::Stopped);
    }
}
