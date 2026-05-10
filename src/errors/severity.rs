//! Severity vocabulary for kernel error classification.
//!
//! The six-level scale (`Debug` through `Fatal`) covers the range of
//! states the kernel needs to react to — quiet diagnostic output,
//! routine warnings, recoverable errors, and unrecoverable failures.
//! `Severity` is `Ord`, so callers can compare two severities
//! directly: `Severity::Critical > Severity::Warning` is `true`.

use std::fmt;

/// Severity of a classified error.
///
/// Variants are declared in increasing severity order; the derived
/// [`PartialOrd`]/[`Ord`] implementations therefore order the
/// vocabulary numerically. The numeric ordinal is exposed via
/// [`Severity::ordinal`] and is part of this enum's contract.
///
/// `Display` writes the variant name in uppercase and is intended for
/// internal use (logs, metrics labels, event topics). User-visible
/// status text routes through [`lang_lib::t!`] in the error types
/// that own it.
///
/// Marked `#[non_exhaustive]` so future severities can be added
/// without breaking SemVer.
///
/// # Examples
///
/// ```
/// use service_kernel::errors::Severity;
///
/// assert!(Severity::Debug < Severity::Fatal);
/// assert_eq!(Severity::default(), Severity::Error);
/// assert_eq!(Severity::Warning.as_str(), "warning");
/// ```
#[non_exhaustive]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, PartialOrd, Ord)]
#[repr(u8)]
pub enum Severity {
    /// Diagnostic output. Below the operator's attention threshold.
    Debug,
    /// Informational. Steady-state lifecycle progress, periodic notes.
    Info,
    /// Unexpected condition that did not fail the operation.
    Warning,
    /// Recoverable failure. Default severity.
    Error,
    /// Failure that threatens stability of a subsystem or the service.
    Critical,
    /// Failure that has already cost the service its ability to serve.
    Fatal,
}

impl Severity {
    /// Returns the lowercase variant name as a static string.
    ///
    /// Used as event-topic suffix and metrics label.
    #[inline]
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Severity::Debug => "debug",
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
            Severity::Critical => "critical",
            Severity::Fatal => "fatal",
        }
    }

    /// Returns the numeric ordinal in declaration order.
    ///
    /// `Severity::Debug` is `0`; `Severity::Fatal` is `5`.
    #[inline]
    #[must_use]
    pub const fn ordinal(&self) -> u8 {
        match self {
            Severity::Debug => 0,
            Severity::Info => 1,
            Severity::Warning => 2,
            Severity::Error => 3,
            Severity::Critical => 4,
            Severity::Fatal => 5,
        }
    }
}

impl Default for Severity {
    /// Returns [`Severity::Error`].
    #[inline]
    fn default() -> Self {
        Severity::Error
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Debug => f.write_str("DEBUG"),
            Severity::Info => f.write_str("INFO"),
            Severity::Warning => f.write_str("WARNING"),
            Severity::Error => f.write_str("ERROR"),
            Severity::Critical => f.write_str("CRITICAL"),
            Severity::Fatal => f.write_str("FATAL"),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const ALL: [Severity; 6] = [
        Severity::Debug,
        Severity::Info,
        Severity::Warning,
        Severity::Error,
        Severity::Critical,
        Severity::Fatal,
    ];

    #[test]
    fn test_as_str_values_are_unique() {
        let mut set = HashSet::new();
        for s in ALL {
            assert!(set.insert(s.as_str()));
        }
    }

    #[test]
    fn test_ordering_is_strictly_increasing() {
        for window in ALL.windows(2) {
            assert!(window[0] < window[1], "{:?} < {:?}", window[0], window[1]);
        }
    }

    #[test]
    fn test_default_is_error() {
        assert_eq!(Severity::default(), Severity::Error);
    }

    #[test]
    fn test_display_is_uppercase_variant_name() {
        assert_eq!(Severity::Debug.to_string(), "DEBUG");
        assert_eq!(Severity::Fatal.to_string(), "FATAL");
    }

    #[test]
    fn test_ordinals_match_declaration_order() {
        for (i, s) in ALL.iter().enumerate() {
            assert_eq!(usize::from(s.ordinal()), i);
        }
    }
}
