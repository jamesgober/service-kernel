//! The kernel's own typed error.
//!
//! [`KernelError`] is the error type returned by kernel APIs. It is
//! a category-keyed enum where each variant carries a stable
//! [`KernelErrorCode`] in the form `KER-NNNNN`. Callers can match on
//! the variant for type-specific recovery, on the code for stable
//! string-pivots in logs, or on the [`KernelError::category`] for a
//! coarse classification.
//!
//! # Translation key convention
//!
//! `KernelError::Display` routes its message through [`lang_lib::t!`]
//! under the key:
//!
//! ```text
//! kernel.error.<category>.<numeric_code>
//! ```
//!
//! For example: `kernel.error.lifecycle.10001`. The fallback string
//! is `"<category>: KER-<padded_code>"` (e.g.
//! `"lifecycle: KER-10001"`), so logs remain readable when no locale
//! has been loaded. New variants in later milestones MUST follow this
//! same key convention.

use std::error::Error;
use std::fmt;

use lang_lib::Lang;

use crate::lifecycle::TransitionError;
use crate::primitives::WorkerId;

/// Boxed source-error type used by [`KernelError`] variants whose
/// underlying cause is consumer-defined.
type BoxError = Box<dyn Error + Send + Sync + 'static>;

/// Stable error codes for the kernel's own errors.
///
/// Codes are grouped by category in the form `KER-NNNNN`:
///
/// - `1xxxx` — lifecycle
/// - `2xxxx` — subsystem
/// - `3xxxx` — worker
/// - `4xxxx` — config
/// - `5xxxx` — shutdown
/// - `9xxxx` — generic / internal
///
/// New variants land alongside the modules that produce them. The
/// numeric values are part of the stable contract — never reuse a
/// retired value.
#[non_exhaustive]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
#[repr(u32)]
pub enum KernelErrorCode {
    /// Illegal lifecycle transition request.
    LifecycleIllegalTransition = 10_001,

    /// Subsystem boot failed (placeholder; expanded in Milestone E).
    SubsystemBootFailed = 20_001,
    /// Subsystem dependency was not registered.
    SubsystemDependencyMissing = 20_002,

    /// Worker spawn failed (placeholder; expanded in Milestone F).
    WorkerSpawnFailed = 30_001,

    /// Configuration was invalid.
    ConfigInvalid = 40_001,

    /// Shutdown timed out (placeholder; expanded in Milestone H).
    ShutdownTimeout = 50_001,

    /// Generic internal error.
    Internal = 90_001,
}

impl KernelErrorCode {
    /// Returns the numeric value of this error code.
    #[inline]
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self as u32
    }
}

/// The kernel's own error type.
///
/// `KernelError` is `#[non_exhaustive]` only by way of the discriminated
/// variants — additions land in later milestones. Variants whose
/// underlying cause comes from the consumer (subsystems, workers,
/// shutdown hooks) carry a boxed [`std::error::Error`] source.
#[non_exhaustive]
#[derive(Debug)]
pub enum KernelError {
    /// A lifecycle transition was rejected by the legal-transition
    /// table.
    Lifecycle {
        /// Stable error code.
        code: KernelErrorCode,
        /// Underlying transition rejection.
        source: TransitionError,
    },
    /// A subsystem failed to boot or load.
    Subsystem {
        /// Stable error code.
        code: KernelErrorCode,
        /// Subsystem name.
        name: &'static str,
        /// Underlying cause from the subsystem implementation.
        source: BoxError,
    },
    /// A worker failed to spawn or run.
    Worker {
        /// Stable error code.
        code: KernelErrorCode,
        /// Identifier of the worker that produced the error.
        worker_id: WorkerId,
        /// Underlying cause from the worker implementation.
        source: BoxError,
    },
    /// Configuration provided to the kernel was invalid.
    Config {
        /// Stable error code.
        code: KernelErrorCode,
        /// Underlying cause from the config layer.
        source: BoxError,
    },
    /// Shutdown coordination failed.
    Shutdown {
        /// Stable error code.
        code: KernelErrorCode,
        /// Underlying cause from the shutdown layer.
        source: BoxError,
    },
    /// Generic / internal kernel error with no stable cause.
    Internal {
        /// Stable error code.
        code: KernelErrorCode,
        /// Operator-readable description of the failure.
        message: String,
    },
}

impl KernelError {
    /// Returns the stable error code for this error.
    #[inline]
    #[must_use]
    pub fn code(&self) -> KernelErrorCode {
        match self {
            KernelError::Lifecycle { code, .. }
            | KernelError::Subsystem { code, .. }
            | KernelError::Worker { code, .. }
            | KernelError::Config { code, .. }
            | KernelError::Shutdown { code, .. }
            | KernelError::Internal { code, .. } => *code,
        }
    }

    /// Returns the lowercase variant category.
    ///
    /// The category is the namespace component used in translation
    /// keys: `kernel.error.<category>.<numeric_code>`.
    #[inline]
    #[must_use]
    pub fn category(&self) -> &'static str {
        match self {
            KernelError::Lifecycle { .. } => "lifecycle",
            KernelError::Subsystem { .. } => "subsystem",
            KernelError::Worker { .. } => "worker",
            KernelError::Config { .. } => "config",
            KernelError::Shutdown { .. } => "shutdown",
            KernelError::Internal { .. } => "internal",
        }
    }
}

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let category = self.category();
        let code = self.code().as_u32();
        let key = format!("kernel.error.{}.{}", category, code);
        let fallback = format!("{}: KER-{:05}", category, code);
        let translated = Lang::translate(&key, None, Some(&fallback));
        if let KernelError::Internal { message, .. } = self {
            if !message.is_empty() {
                return write!(f, "{}: {}", translated, message);
            }
        }
        f.write_str(&translated)
    }
}

impl Error for KernelError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            KernelError::Lifecycle { source, .. } => Some(source),
            KernelError::Subsystem { source, .. }
            | KernelError::Worker { source, .. }
            | KernelError::Config { source, .. }
            | KernelError::Shutdown { source, .. } => Some(&**source),
            KernelError::Internal { .. } => None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::lifecycle::KernelState;
    use std::collections::HashSet;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn test_error_codes_are_unique() {
        let codes = [
            KernelErrorCode::LifecycleIllegalTransition,
            KernelErrorCode::SubsystemBootFailed,
            KernelErrorCode::SubsystemDependencyMissing,
            KernelErrorCode::WorkerSpawnFailed,
            KernelErrorCode::ConfigInvalid,
            KernelErrorCode::ShutdownTimeout,
            KernelErrorCode::Internal,
        ];
        let mut set = HashSet::new();
        for c in codes {
            assert!(set.insert(c.as_u32()));
            assert!(c.as_u32() > 0);
        }
    }

    #[test]
    fn test_kernel_error_is_send_sync() {
        assert_send_sync::<KernelError>();
    }

    #[test]
    fn test_lifecycle_variant_carries_source() {
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
    }

    #[test]
    fn test_internal_variant_has_no_source() {
        let err = KernelError::Internal {
            code: KernelErrorCode::Internal,
            message: "boom".to_owned(),
        };
        assert!(err.source().is_none());
    }

    #[test]
    fn test_display_contains_code_in_fallback() {
        let err = KernelError::Internal {
            code: KernelErrorCode::Internal,
            message: String::new(),
        };
        let rendered = err.to_string();
        assert!(rendered.contains("internal"));
        assert!(rendered.contains("KER-90001"));
    }

    #[test]
    fn test_display_appends_internal_message() {
        let err = KernelError::Internal {
            code: KernelErrorCode::Internal,
            message: "boom".to_owned(),
        };
        let rendered = err.to_string();
        assert!(rendered.contains("boom"));
    }

    #[test]
    fn test_subsystem_variant_carries_source() {
        let cause: BoxError = "missing dep".into();
        let err = KernelError::Subsystem {
            code: KernelErrorCode::SubsystemDependencyMissing,
            name: "storage",
            source: cause,
        };
        assert_eq!(err.category(), "subsystem");
        assert!(err.source().is_some());
    }

    #[test]
    fn test_category_matches_lowercase_variant() {
        assert_eq!(
            KernelError::Internal {
                code: KernelErrorCode::Internal,
                message: String::new(),
            }
            .category(),
            "internal"
        );
    }
}
