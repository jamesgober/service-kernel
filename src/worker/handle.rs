//! Observable view of a single worker's state.

use std::fmt;
use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::primitives::WorkerId;

use super::WorkerState;

/// Cheap, cloneable view into one worker's runtime state.
///
/// All fields are atomic — readers never lock. The supervisor
/// updates the cell as the worker progresses; consumers
/// (e.g. `KernelSnapshot`) read the cell at any time.
#[derive(Clone)]
pub struct WorkerHandle {
    id: WorkerId,
    name: &'static str,
    state: Arc<AtomicU8>,
    cancel_token: CancellationToken,
    failures_in_window: Arc<AtomicU32>,
    last_failure_nanos: Arc<AtomicI64>,
}

impl WorkerHandle {
    /// Constructs a handle. Used internally by the supervisor.
    pub(crate) fn new(
        id: WorkerId,
        name: &'static str,
        cancel_token: CancellationToken,
    ) -> Self {
        Self {
            id,
            name,
            state: Arc::new(AtomicU8::new(WorkerState::Created as u8)),
            cancel_token,
            failures_in_window: Arc::new(AtomicU32::new(0)),
            last_failure_nanos: Arc::new(AtomicI64::new(0)),
        }
    }

    /// Returns the worker's identifier.
    #[inline]
    #[must_use]
    pub fn id(&self) -> WorkerId {
        self.id
    }

    /// Returns the worker's stable name.
    #[inline]
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Returns the worker's current [`WorkerState`].
    #[inline]
    #[must_use]
    pub fn state(&self) -> WorkerState {
        WorkerState::from_u8(self.state.load(Ordering::Acquire))
    }

    /// Returns `true` when the worker is in an active state
    /// ([`WorkerState::is_active`]).
    #[inline]
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.state().is_active()
    }

    /// Asks the worker to cancel.
    ///
    /// Idempotent. Workers detect cancellation via
    /// [`WorkerContext::is_cancelled`](super::WorkerContext::is_cancelled)
    /// or [`WorkerContext::cancelled`](super::WorkerContext::cancelled).
    #[inline]
    pub fn cancel(&self) {
        self.cancel_token.cancel();
    }

    /// Updates the state cell. Used internally by the supervisor.
    #[inline]
    pub(crate) fn set_state(&self, state: WorkerState) {
        self.state.store(state as u8, Ordering::Release);
    }

    /// Records a failure timestamp and increments the counter
    /// (resetting it if the previous failure is outside `window`).
    pub(crate) fn record_failure(&self, window_nanos: i64) -> u32 {
        let now = unix_nanos();
        let last = self.last_failure_nanos.swap(now, Ordering::AcqRel);
        if last == 0 || now - last > window_nanos {
            self.failures_in_window.store(1, Ordering::Release);
            1
        } else {
            self.failures_in_window.fetch_add(1, Ordering::AcqRel) + 1
        }
    }

    /// Returns the supervisor-side cancellation token. Used by the
    /// supervisor when spawning the worker's task.
    #[inline]
    #[must_use]
    pub(crate) fn cancel_token(&self) -> CancellationToken {
        self.cancel_token.clone()
    }
}

impl fmt::Debug for WorkerHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkerHandle")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

fn unix_nanos() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::primitives::IdGenerator;

    #[test]
    fn test_new_handle_in_created_state() {
        let id_gen = IdGenerator::new();
        let h = WorkerHandle::new(id_gen.next_worker_id(), "alpha", CancellationToken::new());
        assert_eq!(h.state(), WorkerState::Created);
        assert!(!h.is_active());
        assert_eq!(h.name(), "alpha");
    }

    #[test]
    fn test_cancel_token_propagates() {
        let id_gen = IdGenerator::new();
        let token = CancellationToken::new();
        let h = WorkerHandle::new(id_gen.next_worker_id(), "alpha", token.clone());
        h.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn test_set_state_round_trips() {
        let id_gen = IdGenerator::new();
        let h = WorkerHandle::new(id_gen.next_worker_id(), "alpha", CancellationToken::new());
        h.set_state(WorkerState::Running);
        assert_eq!(h.state(), WorkerState::Running);
        assert!(h.is_active());
    }

    #[test]
    fn test_record_failure_increments_within_window() {
        let id_gen = IdGenerator::new();
        let h = WorkerHandle::new(id_gen.next_worker_id(), "alpha", CancellationToken::new());
        // Use a very wide window so successive calls all count.
        let window_nanos = i64::MAX / 2;
        let n1 = h.record_failure(window_nanos);
        let n2 = h.record_failure(window_nanos);
        let n3 = h.record_failure(window_nanos);
        assert_eq!(n1, 1);
        assert_eq!(n2, 2);
        assert_eq!(n3, 3);
    }

    #[test]
    fn test_record_failure_resets_outside_window() {
        let id_gen = IdGenerator::new();
        let h = WorkerHandle::new(id_gen.next_worker_id(), "alpha", CancellationToken::new());
        // Window of 0 nanos forces a reset on every call (last == now is fine,
        // but anything > 0 nanos elapsed triggers a reset).
        let _ = h.record_failure(0);
        std::thread::sleep(std::time::Duration::from_millis(2));
        let n2 = h.record_failure(0);
        assert_eq!(n2, 1);
    }
}
