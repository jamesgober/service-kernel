//! Per-worker handle bundle.
//!
//! [`WorkerContext`] is constructed by the supervisor and handed to
//! the worker's `run` method. It exposes cancellation (sync + async),
//! heartbeat tracking, and `Clone`-able views into the kernel's
//! event, metrics, and health handles.

use std::fmt;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::events::EventHandle;
use crate::health::HealthHandle;
use crate::metrics::MetricsHandle;
use crate::primitives::WorkerId;

/// Cooperative-cancel + heartbeat context handed to every worker.
///
/// Cheap to clone — internal state lives behind `Arc`s.
#[derive(Clone)]
pub struct WorkerContext {
    cancel_token: CancellationToken,
    last_heartbeat: Arc<AtomicI64>,
    metrics: MetricsHandle,
    events: EventHandle,
    health: HealthHandle,
    worker_id: WorkerId,
    worker_name: &'static str,
}

impl WorkerContext {
    /// Constructs a context. Used internally by the supervisor.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        worker_id: WorkerId,
        worker_name: &'static str,
        cancel_token: CancellationToken,
        last_heartbeat: Arc<AtomicI64>,
        metrics: MetricsHandle,
        events: EventHandle,
        health: HealthHandle,
    ) -> Self {
        Self {
            cancel_token,
            last_heartbeat,
            metrics,
            events,
            health,
            worker_id,
            worker_name,
        }
    }

    /// Returns the worker's stable identifier.
    #[inline]
    #[must_use]
    pub fn id(&self) -> WorkerId {
        self.worker_id
    }

    /// Returns the worker's stable name.
    #[inline]
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.worker_name
    }

    /// Returns `true` once the worker has been asked to stop.
    ///
    /// Sync check — call freely from inside synchronous loops.
    #[inline]
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    /// Awaits the cancellation signal.
    ///
    /// Use this in a `tokio::select!` so the worker can interleave
    /// cancellation with normal work.
    #[inline]
    pub async fn cancelled(&self) {
        self.cancel_token.cancelled().await;
    }

    /// Records a heartbeat: stores the current timestamp in nanos
    /// since the Unix epoch.
    ///
    /// Lock-free atomic store. Cheap enough to call inside tight
    /// loops; the watchdog (Milestone G) reads the same atomic.
    #[inline]
    pub fn heartbeat(&self) {
        let now = unix_nanos();
        self.last_heartbeat.store(now, Ordering::Release);
    }

    /// Returns the wall-clock instant of the last heartbeat as
    /// nanos-since-epoch, or `0` if the worker has not heartbeated yet.
    #[inline]
    #[must_use]
    pub fn last_heartbeat_nanos(&self) -> i64 {
        self.last_heartbeat.load(Ordering::Acquire)
    }

    /// Returns the metrics handle.
    #[inline]
    #[must_use]
    pub fn metrics(&self) -> &MetricsHandle {
        &self.metrics
    }

    /// Returns the event handle.
    #[inline]
    #[must_use]
    pub fn events(&self) -> &EventHandle {
        &self.events
    }

    /// Returns the health handle.
    #[inline]
    #[must_use]
    pub fn health(&self) -> &HealthHandle {
        &self.health
    }
}

impl fmt::Debug for WorkerContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkerContext")
            .field("id", &self.worker_id)
            .field("name", &self.worker_name)
            .field("cancelled", &self.is_cancelled())
            .finish_non_exhaustive()
    }
}

/// Returns the current time in nanoseconds since the Unix epoch.
/// Saturates at 0 if the system clock is before the epoch.
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
    use crate::events::EventDispatcher;
    use crate::health::HealthRegistry;
    use crate::metrics::NoopMetricsBackend;
    use crate::primitives::IdGenerator;

    fn make_ctx() -> (WorkerContext, CancellationToken) {
        let dispatcher = EventDispatcher::new();
        let health = HealthRegistry::new();
        let metrics: MetricsHandle = Arc::new(NoopMetricsBackend);
        let token = CancellationToken::new();
        let id_gen = IdGenerator::new();
        let ctx = WorkerContext::new(
            id_gen.next_worker_id(),
            "test-worker",
            token.clone(),
            Arc::new(AtomicI64::new(0)),
            metrics,
            dispatcher.handle(),
            health.handle(),
        );
        (ctx, token)
    }

    #[test]
    fn test_initial_state() {
        let (ctx, _token) = make_ctx();
        assert_eq!(ctx.name(), "test-worker");
        assert!(!ctx.is_cancelled());
        assert_eq!(ctx.last_heartbeat_nanos(), 0);
    }

    #[test]
    fn test_cancel_flips_is_cancelled() {
        let (ctx, token) = make_ctx();
        token.cancel();
        assert!(ctx.is_cancelled());
    }

    #[test]
    fn test_heartbeat_updates_atomic() {
        let (ctx, _token) = make_ctx();
        ctx.heartbeat();
        assert!(ctx.last_heartbeat_nanos() > 0);
    }

    #[tokio::test]
    async fn test_cancelled_resolves_after_cancel() {
        let (ctx, token) = make_ctx();
        token.cancel();
        ctx.cancelled().await;
    }

    #[tokio::test]
    async fn test_clone_shares_cancellation_state() {
        let (ctx, token) = make_ctx();
        let clone = ctx.clone();
        token.cancel();
        assert!(clone.is_cancelled());
    }
}
