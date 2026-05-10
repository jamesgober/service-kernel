//! Error classification and runtime policy.
//!
//! The kernel does not invent a single error hierarchy. Instead, it
//! offers a classification system: any [`std::error::Error`] can be
//! classified into a [`Severity`] and an [`ErrorAction`] by a
//! consumer-supplied [`ErrorClassifier`]. The kernel routes the
//! result through events, health, and metrics in subsequent
//! milestones.
//!
//! The kernel's own errors are typed as [`KernelError`] with stable
//! codes in the form `KER-NNNNN`. Localized message text routes
//! through [`lang_lib::t!`] under keys of the form
//! `kernel.error.<category>.<numeric_code>`. The fallback string
//! always names the category and code, so logs remain readable
//! when no locale has been loaded.

pub mod action;
pub mod classifier;
pub mod kernel_error;
pub mod registry;
pub mod severity;

pub use action::ErrorAction;
pub use classifier::{Classification, ErrorClassifier, NoopClassifier};
pub use kernel_error::{KernelError, KernelErrorCode};
pub use registry::ErrorRegistry;
pub use severity::Severity;
