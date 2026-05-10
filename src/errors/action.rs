//! Advisory policy actions returned alongside a [`Severity`](super::Severity).
//!
//! `ErrorAction` is a recommendation, not a mandate. The supervisor,
//! lifecycle controller, or shutdown coordinator decides whether to
//! honor the recommendation when it sees the [`Classification`](super::Classification).
//! A classifier saying [`ErrorAction::AbortProcess`] does not actually
//! abort the process; it sets the flag, and the controller decides.

use std::fmt;

/// Recommended response to a classified error.
///
/// Variants are arranged from least to most disruptive — this is a
/// soft ordering, not enforced. Marked `#[non_exhaustive]` so future
/// actions can land without breaking SemVer.
///
/// # Examples
///
/// ```
/// use service_kernel::errors::ErrorAction;
///
/// assert_eq!(ErrorAction::default(), ErrorAction::LogOnly);
/// assert!(ErrorAction::AbortProcess.is_terminal());
/// assert!(!ErrorAction::RestartWorker.is_terminal());
/// ```
#[non_exhaustive]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
#[repr(u8)]
pub enum ErrorAction {
    /// Log the error and move on. Default.
    LogOnly,
    /// Emit an error event so subscribers can react.
    EmitEvent,
    /// Mark the worker that produced the error as degraded.
    MarkWorkerDegraded,
    /// Mark the kernel itself as degraded (still serving, partial loss).
    MarkServiceDegraded,
    /// Restart the worker that produced the error.
    RestartWorker,
    /// Open the circuit breaker on the failing dependency.
    OpenCircuit,
    /// Mark a subsystem (commonly storage) as read-only.
    EnterReadOnlyMode,
    /// Begin graceful shutdown.
    BeginShutdown,
    /// Abort the process — last-resort response to a fatal error.
    AbortProcess,
}

impl ErrorAction {
    /// Returns the lowercase variant name as a static string.
    #[inline]
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            ErrorAction::LogOnly => "log_only",
            ErrorAction::EmitEvent => "emit_event",
            ErrorAction::MarkWorkerDegraded => "mark_worker_degraded",
            ErrorAction::MarkServiceDegraded => "mark_service_degraded",
            ErrorAction::RestartWorker => "restart_worker",
            ErrorAction::OpenCircuit => "open_circuit",
            ErrorAction::EnterReadOnlyMode => "enter_read_only_mode",
            ErrorAction::BeginShutdown => "begin_shutdown",
            ErrorAction::AbortProcess => "abort_process",
        }
    }

    /// Returns `true` for actions that move the kernel toward
    /// termination ([`ErrorAction::BeginShutdown`] and
    /// [`ErrorAction::AbortProcess`]).
    #[inline]
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, ErrorAction::BeginShutdown | ErrorAction::AbortProcess)
    }
}

impl Default for ErrorAction {
    /// Returns [`ErrorAction::LogOnly`].
    #[inline]
    fn default() -> Self {
        ErrorAction::LogOnly
    }
}

impl fmt::Display for ErrorAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ErrorAction::LogOnly => "LOG_ONLY",
            ErrorAction::EmitEvent => "EMIT_EVENT",
            ErrorAction::MarkWorkerDegraded => "MARK_WORKER_DEGRADED",
            ErrorAction::MarkServiceDegraded => "MARK_SERVICE_DEGRADED",
            ErrorAction::RestartWorker => "RESTART_WORKER",
            ErrorAction::OpenCircuit => "OPEN_CIRCUIT",
            ErrorAction::EnterReadOnlyMode => "ENTER_READ_ONLY_MODE",
            ErrorAction::BeginShutdown => "BEGIN_SHUTDOWN",
            ErrorAction::AbortProcess => "ABORT_PROCESS",
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const ALL: [ErrorAction; 9] = [
        ErrorAction::LogOnly,
        ErrorAction::EmitEvent,
        ErrorAction::MarkWorkerDegraded,
        ErrorAction::MarkServiceDegraded,
        ErrorAction::RestartWorker,
        ErrorAction::OpenCircuit,
        ErrorAction::EnterReadOnlyMode,
        ErrorAction::BeginShutdown,
        ErrorAction::AbortProcess,
    ];

    #[test]
    fn test_as_str_values_are_unique() {
        let mut set = HashSet::new();
        for a in ALL {
            assert!(set.insert(a.as_str()));
        }
    }

    #[test]
    fn test_is_terminal_only_for_shutdown_and_abort() {
        for a in ALL {
            let expected = matches!(a, ErrorAction::BeginShutdown | ErrorAction::AbortProcess);
            assert_eq!(a.is_terminal(), expected, "{:?}", a);
        }
    }

    #[test]
    fn test_default_is_log_only() {
        assert_eq!(ErrorAction::default(), ErrorAction::LogOnly);
    }

    #[test]
    fn test_display_is_screaming_snake_case() {
        assert_eq!(ErrorAction::LogOnly.to_string(), "LOG_ONLY");
        assert_eq!(ErrorAction::AbortProcess.to_string(), "ABORT_PROCESS");
    }
}
