//! Error classification trait and the kernel's default no-op classifier.
//!
//! The kernel does not assume which errors a consumer cares about.
//! Consumers implement [`ErrorClassifier`] to map their domain errors
//! to a [`Classification`] (severity + recommended action + optional
//! event topic), and register the classifier with the kernel's
//! [`ErrorRegistry`](super::ErrorRegistry).
//!
//! Until a consumer registers a classifier, the kernel runs with
//! [`NoopClassifier`], which classifies every error as
//! `Classification::default()` — `Severity::Error`, `ErrorAction::LogOnly`,
//! no event topic. Boring, safe, predictable.

use std::error::Error;

use super::{ErrorAction, Severity};

/// Result of classifying an error.
///
/// `Classification` is `Copy` because it is a small value type — three
/// fields totalling a handful of bytes. Pass it by value freely.
///
/// # Examples
///
/// ```
/// use service_kernel::errors::{Classification, ErrorAction, Severity};
///
/// let c = Classification::default();
/// assert_eq!(c.severity, Severity::Error);
/// assert_eq!(c.action, ErrorAction::LogOnly);
/// assert!(c.event_topic.is_none());
/// ```
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Classification {
    /// Severity of the classified error.
    pub severity: Severity,
    /// Recommended action — advisory only.
    pub action: ErrorAction,
    /// Optional event topic for event emission.
    pub event_topic: Option<&'static str>,
}

impl Default for Classification {
    /// Returns the kernel's safe default: `Severity::Error`,
    /// `ErrorAction::LogOnly`, no event topic.
    #[inline]
    fn default() -> Self {
        Self {
            severity: Severity::Error,
            action: ErrorAction::LogOnly,
            event_topic: None,
        }
    }
}

/// Maps any [`std::error::Error`] to a [`Classification`].
///
/// Implementations typically downcast the error reference to the
/// consumer's own error type and pattern-match on the variant. If the
/// error is unknown, return [`Classification::default`] — the kernel
/// will treat it as a routine error.
///
/// The trait is object-safe and is held in the [`ErrorRegistry`](super::ErrorRegistry)
/// behind an [`std::sync::Arc<dyn ErrorClassifier>`].
///
/// # Examples
///
/// ```
/// use std::error::Error;
/// use std::fmt;
/// use service_kernel::errors::{
///     Classification, ErrorAction, ErrorClassifier, Severity,
/// };
///
/// #[derive(Debug)]
/// enum MyError {
///     StorageCorrupt,
///     AuthFailed,
/// }
///
/// impl fmt::Display for MyError {
///     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
///         match self {
///             MyError::StorageCorrupt => f.write_str("storage corrupt"),
///             MyError::AuthFailed => f.write_str("auth failed"),
///         }
///     }
/// }
///
/// impl Error for MyError {}
///
/// struct MyClassifier;
///
/// impl ErrorClassifier for MyClassifier {
///     fn classify(&self, err: &(dyn Error + 'static)) -> Classification {
///         if let Some(my_err) = err.downcast_ref::<MyError>() {
///             return match my_err {
///                 MyError::StorageCorrupt => Classification {
///                     severity: Severity::Critical,
///                     action: ErrorAction::EnterReadOnlyMode,
///                     event_topic: Some("myapp.storage.corrupt"),
///                 },
///                 MyError::AuthFailed => Classification {
///                     severity: Severity::Warning,
///                     action: ErrorAction::LogOnly,
///                     event_topic: None,
///                 },
///             };
///         }
///         Classification::default()
///     }
/// }
///
/// let classifier = MyClassifier;
/// let result = classifier.classify(&MyError::StorageCorrupt);
/// assert_eq!(result.severity, Severity::Critical);
/// ```
pub trait ErrorClassifier: Send + Sync + 'static {
    /// Classifies an error.
    fn classify(&self, err: &(dyn Error + 'static)) -> Classification;
}

/// Default classifier: returns [`Classification::default`] for every
/// error.
///
/// Registered automatically by [`ErrorRegistry::new`](super::ErrorRegistry::new).
/// Replace it via
/// [`ErrorRegistry::set_classifier`](super::ErrorRegistry::set_classifier)
/// when the consumer is ready to wire up its own error policy.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopClassifier;

impl ErrorClassifier for NoopClassifier {
    #[inline]
    fn classify(&self, _err: &(dyn Error + 'static)) -> Classification {
        Classification::default()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fmt;
    use std::sync::Arc;

    #[derive(Debug)]
    struct DummyError;

    impl fmt::Display for DummyError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("dummy")
        }
    }

    impl Error for DummyError {}

    #[derive(Debug)]
    struct CustomError(&'static str);

    impl fmt::Display for CustomError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0)
        }
    }

    impl Error for CustomError {}

    struct CustomClassifier;

    impl ErrorClassifier for CustomClassifier {
        fn classify(&self, err: &(dyn Error + 'static)) -> Classification {
            if let Some(c) = err.downcast_ref::<CustomError>() {
                if c.0 == "high" {
                    return Classification {
                        severity: Severity::Critical,
                        action: ErrorAction::BeginShutdown,
                        event_topic: Some("custom.high"),
                    };
                }
            }
            Classification::default()
        }
    }

    #[test]
    fn test_classification_default() {
        let c = Classification::default();
        assert_eq!(c.severity, Severity::Error);
        assert_eq!(c.action, ErrorAction::LogOnly);
        assert!(c.event_topic.is_none());
    }

    #[test]
    fn test_noop_classifier_returns_default() {
        let c = NoopClassifier;
        let result = c.classify(&DummyError);
        assert_eq!(result, Classification::default());
    }

    #[test]
    fn test_custom_classifier_downcasts() {
        let c = CustomClassifier;
        let high = c.classify(&CustomError("high"));
        assert_eq!(high.severity, Severity::Critical);
        assert_eq!(high.action, ErrorAction::BeginShutdown);
        assert_eq!(high.event_topic, Some("custom.high"));

        let low = c.classify(&CustomError("low"));
        assert_eq!(low, Classification::default());

        let other = c.classify(&DummyError);
        assert_eq!(other, Classification::default());
    }

    #[test]
    fn test_trait_is_object_safe() {
        let boxed: Arc<dyn ErrorClassifier> = Arc::new(NoopClassifier);
        let _ = boxed.classify(&DummyError);
    }
}
