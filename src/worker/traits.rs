//! [`Worker`] and [`AsyncWorker`] traits — the consumer extension
//! point for supervised work.
//!
//! Sync workers run on Tokio's blocking thread pool via
//! `spawn_blocking`. Async workers run as ordinary Tokio tasks. The
//! supervisor accepts both behind a single dispatch path.

use std::error::Error;
use std::fmt;

use lang_lib::Lang;

use super::WorkerContext;

/// Error returned by a worker's `run` method.
///
/// Carries an operator-readable `message` plus an optional
/// `source` for chained-error inspection. `Display` translates the
/// prefix through [`lang_lib::t!`].
#[derive(Debug)]
pub struct WorkerError {
    /// Operator-readable description of the failure.
    pub message: String,
    /// Optional underlying cause.
    pub source: Option<Box<dyn Error + Send + Sync + 'static>>,
}

impl WorkerError {
    /// Constructs a `WorkerError` with the given message and no
    /// source.
    #[inline]
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source: None,
        }
    }

    /// Constructs a `WorkerError` with both a message and a source.
    #[inline]
    #[must_use]
    pub fn with_source(
        message: impl Into<String>,
        source: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }
}

impl fmt::Display for WorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = Lang::translate("kernel.worker.error.prefix", None, Some("worker error"));
        write!(f, "{}: {}", prefix, self.message)
    }
}

impl Error for WorkerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|s| &**s as &(dyn Error + 'static))
    }
}

/// Synchronous, supervised, long-running work.
///
/// The supervisor calls [`Worker::run`] on a dedicated context; the
/// implementation runs until either the context's cancellation token
/// fires or the work completes. Return `Ok(())` for clean
/// completion; `Err(WorkerError)` triggers the configured restart
/// policy.
///
/// The trait is `Send + Sync + 'static` and held behind
/// `Arc<dyn Worker>` by the supervisor.
///
/// # Cancellation
///
/// Cooperative. Workers MUST check
/// [`WorkerContext::is_cancelled`] periodically and return when it
/// flips. The supervisor will not interrupt a worker that ignores
/// cancellation.
///
/// # Examples
///
/// ```
/// use service_kernel::worker::{Worker, WorkerContext, WorkerError};
///
/// struct CounterWorker;
///
/// impl Worker for CounterWorker {
///     fn name(&self) -> &'static str {
///         "counter"
///     }
///
///     fn run(&self, ctx: WorkerContext) -> Result<(), WorkerError> {
///         let mut n = 0_u64;
///         while !ctx.is_cancelled() {
///             n += 1;
///             ctx.heartbeat();
///             if n > 1_000 {
///                 break;
///             }
///         }
///         Ok(())
///     }
/// }
/// ```
pub trait Worker: Send + Sync + 'static {
    /// Stable worker name. Used in events, metrics, and logs.
    fn name(&self) -> &'static str;

    /// Synchronous entry point for the worker.
    fn run(&self, ctx: WorkerContext) -> Result<(), WorkerError>;
}

/// Asynchronous variant of [`Worker`].
///
/// The supervisor spawns `run` as a Tokio task. Cancellation is
/// awaited via [`WorkerContext::cancelled`].
///
/// `async-trait` is used to keep the trait object-safe so the
/// supervisor can hold heterogeneous async workers behind
/// `Arc<dyn AsyncWorker>`.
///
/// # Examples
///
/// ```
/// use service_kernel::worker::{AsyncWorker, WorkerContext, WorkerError};
///
/// struct AsyncCounterWorker;
///
/// #[async_trait::async_trait]
/// impl AsyncWorker for AsyncCounterWorker {
///     fn name(&self) -> &'static str {
///         "async-counter"
///     }
///
///     async fn run(&self, ctx: WorkerContext) -> Result<(), WorkerError> {
///         loop {
///             tokio::select! {
///                 _ = ctx.cancelled() => return Ok(()),
///                 _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
///                     ctx.heartbeat();
///                 }
///             }
///         }
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait AsyncWorker: Send + Sync + 'static {
    /// Stable worker name. Used in events, metrics, and logs.
    fn name(&self) -> &'static str;

    /// Asynchronous entry point for the worker.
    async fn run(&self, ctx: WorkerContext) -> Result<(), WorkerError>;
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn assert_send_sync<T: Send + Sync + ?Sized>() {}

    struct DummySync;

    impl Worker for DummySync {
        fn name(&self) -> &'static str {
            "dummy-sync"
        }
        fn run(&self, _ctx: WorkerContext) -> Result<(), WorkerError> {
            Ok(())
        }
    }

    struct DummyAsync;

    #[async_trait::async_trait]
    impl AsyncWorker for DummyAsync {
        fn name(&self) -> &'static str {
            "dummy-async"
        }
        async fn run(&self, _ctx: WorkerContext) -> Result<(), WorkerError> {
            Ok(())
        }
    }

    #[test]
    fn test_traits_are_object_safe() {
        let _: Arc<dyn Worker> = Arc::new(DummySync);
        let _: Arc<dyn AsyncWorker> = Arc::new(DummyAsync);
    }

    #[test]
    fn test_worker_error_send_sync() {
        assert_send_sync::<WorkerError>();
    }

    #[test]
    fn test_worker_error_chains_source() {
        #[derive(Debug)]
        struct InnerErr;

        impl fmt::Display for InnerErr {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("inner")
            }
        }

        impl Error for InnerErr {}

        let err = WorkerError::with_source("outer", InnerErr);
        assert_eq!(err.message, "outer");
        assert!(err.source().is_some());
    }

    #[test]
    fn test_worker_error_display_contains_message() {
        let err = WorkerError::new("boom");
        assert!(err.to_string().contains("boom"));
    }
}
