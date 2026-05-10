//! Holder for the active [`ErrorClassifier`].
//!
//! The registry decouples *who classifies errors* from *who reports
//! them*. Subsystems and workers see a `&ErrorRegistry` and call
//! [`ErrorRegistry::classify`]; the classifier is swapped in the
//! background by the kernel builder or by runtime reconfiguration.

use std::error::Error;
use std::fmt;
use std::sync::{Arc, RwLock};

use super::{Classification, ErrorClassifier, NoopClassifier};

/// Registry holding the active error classifier.
///
/// Implementation: an `RwLock<Arc<dyn ErrorClassifier>>`. Reads (the
/// hot path: every error classification goes through `classify`)
/// take a read lock and clone an `Arc`; the actual classifier
/// invocation happens after the lock is dropped, so a slow classifier
/// never blocks
/// [`set_classifier`](ErrorRegistry::set_classifier).
///
/// `ErrorRegistry` is `Send + Sync` and intended to live behind an
/// `Arc` shared across subsystems and workers.
pub struct ErrorRegistry {
    classifier: RwLock<Arc<dyn ErrorClassifier>>,
}

impl fmt::Debug for ErrorRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ErrorRegistry").finish_non_exhaustive()
    }
}

impl ErrorRegistry {
    /// Constructs a registry with a [`NoopClassifier`].
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            classifier: RwLock::new(Arc::new(NoopClassifier)),
        }
    }

    /// Constructs a registry with an explicit classifier.
    #[inline]
    #[must_use]
    pub fn with_classifier(classifier: Arc<dyn ErrorClassifier>) -> Self {
        Self {
            classifier: RwLock::new(classifier),
        }
    }

    /// Replaces the active classifier.
    ///
    /// In-flight classifications complete against the old classifier;
    /// subsequent calls use the new one.
    pub fn set_classifier(&self, classifier: Arc<dyn ErrorClassifier>) {
        let mut guard = self
            .classifier
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = classifier;
    }

    /// Classifies an error using the active classifier.
    pub fn classify(&self, err: &(dyn Error + 'static)) -> Classification {
        let active = {
            let guard = self
                .classifier
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Arc::clone(&guard)
        };
        active.classify(err)
    }
}

impl Default for ErrorRegistry {
    /// Returns a registry with a [`NoopClassifier`].
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::errors::{ErrorAction, Severity};
    use std::fmt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;

    fn assert_send_sync<T: Send + Sync>() {}

    #[derive(Debug)]
    struct DummyError;

    impl fmt::Display for DummyError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("dummy")
        }
    }

    impl Error for DummyError {}

    struct Counting {
        count: AtomicUsize,
    }

    impl ErrorClassifier for Counting {
        fn classify(&self, _err: &(dyn Error + 'static)) -> Classification {
            let _ = self.count.fetch_add(1, Ordering::Relaxed);
            Classification {
                severity: Severity::Critical,
                action: ErrorAction::EmitEvent,
                event_topic: Some("test.counting"),
            }
        }
    }

    #[test]
    fn test_registry_is_send_sync() {
        assert_send_sync::<ErrorRegistry>();
    }

    #[test]
    fn test_new_registry_uses_noop_classifier() {
        let r = ErrorRegistry::new();
        let result = r.classify(&DummyError);
        assert_eq!(result, Classification::default());
    }

    #[test]
    fn test_with_classifier_uses_supplied_classifier() {
        let counting = Arc::new(Counting {
            count: AtomicUsize::new(0),
        });
        let r = ErrorRegistry::with_classifier(Arc::clone(&counting) as Arc<dyn ErrorClassifier>);
        let result = r.classify(&DummyError);
        assert_eq!(result.severity, Severity::Critical);
        assert_eq!(counting.count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_set_classifier_replaces_active() {
        let r = ErrorRegistry::new();
        assert_eq!(r.classify(&DummyError), Classification::default());

        let counting = Arc::new(Counting {
            count: AtomicUsize::new(0),
        });
        r.set_classifier(Arc::clone(&counting) as Arc<dyn ErrorClassifier>);

        let result = r.classify(&DummyError);
        assert_eq!(result.severity, Severity::Critical);
        assert_eq!(counting.count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_default_registry_uses_noop() {
        let r = ErrorRegistry::default();
        assert_eq!(r.classify(&DummyError), Classification::default());
    }

    #[test]
    fn test_concurrent_classify_and_set() {
        let r = Arc::new(ErrorRegistry::new());
        let counting = Arc::new(Counting {
            count: AtomicUsize::new(0),
        });

        let mut handles = Vec::new();

        for _ in 0..4 {
            let r = Arc::clone(&r);
            handles.push(thread::spawn(move || {
                for _ in 0..1_000 {
                    let _ = r.classify(&DummyError);
                }
            }));
        }

        for _ in 0..4 {
            let r = Arc::clone(&r);
            let counting: Arc<dyn ErrorClassifier> = Arc::clone(&counting) as _;
            handles.push(thread::spawn(move || {
                for _ in 0..200 {
                    r.set_classifier(Arc::clone(&counting));
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }
    }
}
