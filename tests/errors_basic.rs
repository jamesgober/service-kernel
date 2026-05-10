//! Integration tests for the kernel's error classification spine.

#![allow(clippy::unwrap_used)]

use std::error::Error;
use std::fmt;
use std::sync::Arc;

use service_kernel::errors::{
    Classification, ErrorAction, ErrorClassifier, ErrorRegistry, KernelError, KernelErrorCode,
    NoopClassifier, Severity,
};
use service_kernel::lifecycle::{KernelState, TransitionError};

#[derive(Debug)]
struct AppError {
    kind: AppKind,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum AppKind {
    StorageCorrupt,
    AuthFailed,
    NotMine,
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.kind)
    }
}

impl Error for AppError {}

struct AppClassifier;

impl ErrorClassifier for AppClassifier {
    fn classify(&self, err: &(dyn Error + 'static)) -> Classification {
        if let Some(app) = err.downcast_ref::<AppError>() {
            return match app.kind {
                AppKind::StorageCorrupt => Classification {
                    severity: Severity::Critical,
                    action: ErrorAction::EnterReadOnlyMode,
                    event_topic: Some("myapp.storage.corrupt"),
                },
                AppKind::AuthFailed => Classification {
                    severity: Severity::Warning,
                    action: ErrorAction::LogOnly,
                    event_topic: None,
                },
                AppKind::NotMine => Classification::default(),
            };
        }
        Classification::default()
    }
}

#[test]
fn test_default_registry_uses_noop_classifier() {
    let r = ErrorRegistry::new();
    let err = AppError {
        kind: AppKind::StorageCorrupt,
    };
    assert_eq!(r.classify(&err), Classification::default());
}

#[test]
fn test_with_classifier_routes_to_consumer_logic() {
    let r = ErrorRegistry::with_classifier(Arc::new(AppClassifier));

    let storage = AppError {
        kind: AppKind::StorageCorrupt,
    };
    let result = r.classify(&storage);
    assert_eq!(result.severity, Severity::Critical);
    assert_eq!(result.action, ErrorAction::EnterReadOnlyMode);
    assert_eq!(result.event_topic, Some("myapp.storage.corrupt"));

    let auth = AppError {
        kind: AppKind::AuthFailed,
    };
    let auth_result = r.classify(&auth);
    assert_eq!(auth_result.severity, Severity::Warning);
}

#[test]
fn test_set_classifier_swap_at_runtime() {
    let r = ErrorRegistry::new();
    let err = AppError {
        kind: AppKind::StorageCorrupt,
    };
    assert_eq!(r.classify(&err), Classification::default());

    r.set_classifier(Arc::new(AppClassifier));
    assert_eq!(r.classify(&err).severity, Severity::Critical);

    r.set_classifier(Arc::new(NoopClassifier));
    assert_eq!(r.classify(&err), Classification::default());
}

#[test]
fn test_unknown_error_falls_through_to_default() {
    let r = ErrorRegistry::with_classifier(Arc::new(AppClassifier));
    let err = AppError {
        kind: AppKind::NotMine,
    };
    assert_eq!(r.classify(&err), Classification::default());
}

#[test]
fn test_kernel_error_lifecycle_variant_round_trips() {
    let inner = TransitionError {
        from: KernelState::Created,
        to: KernelState::Running,
    };
    let err = KernelError::Lifecycle {
        code: KernelErrorCode::LifecycleIllegalTransition,
        source: inner,
    };
    assert_eq!(err.code(), KernelErrorCode::LifecycleIllegalTransition);
    assert_eq!(err.category(), "lifecycle");
    assert!(err.source().is_some());
    let rendered = err.to_string();
    assert!(rendered.contains("KER-10001"));
}

#[test]
fn test_severity_order_is_useful() {
    assert!(Severity::Debug < Severity::Warning);
    assert!(Severity::Warning < Severity::Critical);
    assert!(Severity::Critical < Severity::Fatal);
}
