//! Panic-payload normalization.
//!
//! [`catch_panic`] wraps [`std::panic::catch_unwind`] and turns the
//! `Box<dyn Any>` payload that `panic!` leaves behind into a typed
//! [`PanicReason`]. The reason is then surfaced through events,
//! errors, and logs without leaking dynamic-typed details to
//! consumers.

use std::any::Any;
use std::fmt;
use std::panic::{self, UnwindSafe};

use lang_lib::Lang;

/// Typed normalization of a `catch_unwind` panic payload.
///
/// `panic!("static message")` produces `StaticStr`;
/// `panic!(String::from("dynamic"))` produces `String`. Anything
/// else (including `panic_any(custom_value)`) becomes `Unknown` —
/// the kernel does not attempt to debug-format arbitrary payloads.
///
/// `Display` translates through [`lang_lib::t!`] so panic reasons
/// surfaced in user-visible output (event payloads, error logs)
/// follow the kernel's localization rules.
#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum PanicReason {
    /// Panic payload was a `&'static str`.
    StaticStr(&'static str),
    /// Panic payload was a `String`.
    String(String),
    /// Panic payload was something else; details discarded.
    Unknown,
}

impl PanicReason {
    /// Returns the message, or a localized "unknown" placeholder.
    #[inline]
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            PanicReason::StaticStr(s) => (*s).to_owned(),
            PanicReason::String(s) => s.clone(),
            PanicReason::Unknown => {
                Lang::translate("kernel.worker.panic.unknown", None, Some("<unknown panic>"))
            }
        }
    }

    /// Best-effort conversion of a raw `catch_unwind` payload into a
    /// [`PanicReason`].
    #[must_use]
    pub fn from_payload(payload: Box<dyn Any + Send + 'static>) -> Self {
        let leftover = match payload.downcast::<&'static str>() {
            Ok(s) => return PanicReason::StaticStr(*s),
            Err(p) => p,
        };
        if let Ok(s) = leftover.downcast::<String>() {
            return PanicReason::String(*s);
        }
        PanicReason::Unknown
    }
}

impl fmt::Display for PanicReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = Lang::translate("kernel.worker.panic.prefix", None, Some("worker panicked"));
        write!(f, "{}: {}", prefix, self.message())
    }
}

/// Runs `f`, catching any panic that escapes it.
///
/// Returns `Ok(value)` on a normal return or `Err(reason)` on a
/// panic. `UnwindSafe` is required by [`std::panic::catch_unwind`].
/// Wrap arguments in [`std::panic::AssertUnwindSafe`] when they do
/// not implement `UnwindSafe`.
///
/// # Examples
///
/// ```
/// use service_kernel::worker::panic::{catch_panic, PanicReason};
///
/// let ok = catch_panic(|| 7);
/// assert_eq!(ok.unwrap(), 7);
///
/// let err = catch_panic(|| panic!("oh no"));
/// assert!(matches!(err, Err(PanicReason::StaticStr("oh no"))));
/// ```
pub fn catch_panic<F, T>(f: F) -> Result<T, PanicReason>
where
    F: FnOnce() -> T + UnwindSafe,
{
    panic::catch_unwind(f).map_err(PanicReason::from_payload)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::panic::AssertUnwindSafe;

    #[test]
    fn test_catch_panic_passes_through_normal_value() {
        let result = catch_panic(|| 42);
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_catch_panic_classifies_static_str() {
        let result = catch_panic(|| panic!("boom"));
        match result {
            Err(PanicReason::StaticStr(msg)) => assert_eq!(msg, "boom"),
            other => panic!("expected StaticStr, got {:?}", other),
        }
    }

    #[test]
    fn test_catch_panic_classifies_string_payload() {
        let result = catch_panic(|| panic!("{}", String::from("dynamic")));
        match result {
            Err(PanicReason::String(s)) => assert_eq!(s, "dynamic"),
            other => panic!("expected String, got {:?}", other),
        }
    }

    #[test]
    fn test_catch_panic_classifies_unknown_payload() {
        let result = catch_panic(AssertUnwindSafe(|| std::panic::panic_any(42_u32)));
        assert!(matches!(result, Err(PanicReason::Unknown)));
    }

    #[test]
    fn test_message_returns_useful_string() {
        assert_eq!(PanicReason::StaticStr("hi").message(), "hi");
        assert_eq!(PanicReason::String("dyn".to_owned()).message(), "dyn");
        assert!(!PanicReason::Unknown.message().is_empty());
    }

    #[test]
    fn test_display_includes_prefix_and_message() {
        let rendered = PanicReason::StaticStr("boom").to_string();
        assert!(rendered.contains("boom"));
    }
}
