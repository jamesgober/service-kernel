//! Generic [`tokio::task::JoinSet`] drain with grace + abort.
//!
//! Used by the shutdown coordinator to wait for in-flight tasks
//! to finish, with a deadline. Pattern lifted from the
//! `hive-system` production runtime's `drain_connections` helper
//! and generalized to any `JoinSet<T>`.

use std::time::{Duration, Instant};

use tokio::task::JoinSet;

/// Outcome of a single [`drain`] call.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct DrainOutcome {
    /// Tasks that completed before the grace period expired.
    pub drained: usize,
    /// Tasks aborted because the grace period expired.
    pub aborted: usize,
    /// Wall-clock time spent draining.
    pub elapsed: Duration,
}

impl DrainOutcome {
    /// Total tasks accounted for.
    #[inline]
    #[must_use]
    pub fn total(&self) -> usize {
        self.drained + self.aborted
    }

    /// Returns `true` when every task drained cleanly.
    #[inline]
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.aborted == 0
    }
}

/// Drains a [`JoinSet`] up to the grace deadline; aborts the rest.
///
/// The function awaits task completions in a `tokio::select!` loop
/// against a single `tokio::time::sleep` pinned to the deadline. On
/// expiry it calls [`JoinSet::shutdown`] to abort outstanding tasks
/// and counts the cancellation errors that follow.
///
/// Returns a [`DrainOutcome`] with cleanly-drained, aborted, and
/// elapsed counts.
///
/// # Examples
///
/// ```
/// # async fn example() {
/// use std::time::Duration;
/// use service_kernel::shutdown::drain;
/// use tokio::task::JoinSet;
///
/// let mut set: JoinSet<()> = JoinSet::new();
/// set.spawn(async {
///     tokio::time::sleep(Duration::from_millis(5)).await;
/// });
/// let outcome = drain(&mut set, Duration::from_millis(50)).await;
/// assert_eq!(outcome.drained, 1);
/// assert_eq!(outcome.aborted, 0);
/// # }
/// ```
pub async fn drain<T>(set: &mut JoinSet<T>, grace: Duration) -> DrainOutcome
where
    T: Send + 'static,
{
    let started = Instant::now();
    let mut drained = 0_usize;
    let mut aborted = 0_usize;

    if set.is_empty() {
        return DrainOutcome {
            drained,
            aborted,
            elapsed: started.elapsed(),
        };
    }

    let deadline = started + grace;
    let timeout_fut = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline));
    tokio::pin!(timeout_fut);

    // Phase 1: drain until the deadline or the JoinSet empties.
    loop {
        tokio::select! {
            biased;
            () = &mut timeout_fut => break,
            result = set.join_next() => {
                match result {
                    Some(Ok(_)) => drained += 1,
                    Some(Err(err)) if err.is_cancelled() => aborted += 1,
                    Some(Err(_)) => drained += 1,
                    None => {
                        return DrainOutcome {
                            drained,
                            aborted,
                            elapsed: started.elapsed(),
                        };
                    }
                }
            }
        }
    }

    // Phase 2: deadline expired and the set still holds tasks.
    if !set.is_empty() {
        set.abort_all();
        while let Some(result) = set.join_next().await {
            match result {
                Ok(_) => drained += 1,
                Err(err) if err.is_cancelled() => aborted += 1,
                Err(_) => aborted += 1,
            }
        }
    }

    DrainOutcome {
        drained,
        aborted,
        elapsed: started.elapsed(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_empty_join_set_returns_zero() {
        let mut set: JoinSet<()> = JoinSet::new();
        let outcome = drain(&mut set, Duration::from_millis(50)).await;
        assert_eq!(outcome.drained, 0);
        assert_eq!(outcome.aborted, 0);
        assert!(outcome.is_clean());
    }

    #[tokio::test]
    async fn test_quick_tasks_all_drained() {
        let mut set: JoinSet<()> = JoinSet::new();
        for _ in 0..5 {
            let _ = set.spawn(async { tokio::time::sleep(Duration::from_millis(2)).await });
        }
        let outcome = drain(&mut set, Duration::from_millis(200)).await;
        assert_eq!(outcome.drained, 5);
        assert_eq!(outcome.aborted, 0);
        assert!(outcome.is_clean());
    }

    #[tokio::test]
    async fn test_slow_tasks_all_aborted() {
        let mut set: JoinSet<()> = JoinSet::new();
        for _ in 0..3 {
            let _ = set.spawn(async { tokio::time::sleep(Duration::from_secs(60)).await });
        }
        let outcome = drain(&mut set, Duration::from_millis(20)).await;
        assert_eq!(outcome.drained, 0);
        assert_eq!(outcome.aborted, 3);
        assert!(!outcome.is_clean());
    }

    #[tokio::test]
    async fn test_mixed_tasks_split_correctly() {
        let mut set: JoinSet<()> = JoinSet::new();
        for _ in 0..3 {
            let _ = set.spawn(async { tokio::time::sleep(Duration::from_millis(2)).await });
        }
        for _ in 0..2 {
            let _ = set.spawn(async { tokio::time::sleep(Duration::from_secs(60)).await });
        }
        let outcome = drain(&mut set, Duration::from_millis(50)).await;
        assert_eq!(outcome.drained, 3);
        assert_eq!(outcome.aborted, 2);
        assert_eq!(outcome.total(), 5);
    }

    #[tokio::test]
    async fn test_zero_grace_aborts_immediately() {
        let mut set: JoinSet<()> = JoinSet::new();
        for _ in 0..3 {
            let _ = set.spawn(async { tokio::time::sleep(Duration::from_secs(60)).await });
        }
        let outcome = drain(&mut set, Duration::ZERO).await;
        assert_eq!(outcome.drained, 0);
        assert_eq!(outcome.aborted, 3);
    }
}
