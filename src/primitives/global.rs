//! A lazily-initialized, once-set, thread-safe global container.
//!
//! [`Global<T>`] wraps a [`std::sync::OnceLock`] with the small ergonomic
//! surface the kernel needs:
//!
//! - `const fn` constructor so it can live in `static` scope.
//! - Dual `set` / `try_set` API for callsite clarity.
//! - `get_unchecked` fast path for callers that own the kernel's
//!   initialization contract.
//! - `Debug` impl that distinguishes initialized from uninitialized
//!   state without leaking `T`'s `Debug` bound when the cell is empty.
//!
//! `Global<T>` is the foundation other primitives (id generators, kernel
//! handles, registries) build on. It is intentionally small and free of
//! kernel-specific assumptions.

use std::fmt;
use std::sync::OnceLock;

use lang_lib::t;

/// A lazily-initialized, thread-safe global container.
///
/// At most one value is ever stored; subsequent attempts to write
/// return the rejected value via `Err(value)`. Reads are zero-cost
/// after initialization (a single relaxed atomic load through
/// `OnceLock`).
///
/// The container is `Send + Sync` whenever `T: Send + Sync`. Those
/// bounds come transitively from `OnceLock<T>` — the wrapper adds
/// no `unsafe` impls.
///
/// # Examples
///
/// ```
/// use service_kernel::primitives::Global;
///
/// static CONFIG: Global<u32> = Global::new();
///
/// assert!(!CONFIG.is_initialized());
/// assert!(CONFIG.set(42).is_ok());
/// assert_eq!(CONFIG.get().copied(), Some(42));
/// assert!(CONFIG.set(99).is_err());
/// ```
pub struct Global<T> {
    inner: OnceLock<T>,
}

impl<T> Global<T> {
    /// Creates a new uninitialized container.
    ///
    /// `const` so the container can live in a `static`.
    ///
    /// # Examples
    ///
    /// ```
    /// use service_kernel::primitives::Global;
    ///
    /// static G: Global<&'static str> = Global::new();
    /// assert!(!G.is_initialized());
    /// ```
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: OnceLock::new(),
        }
    }

    /// Sets the contained value. Returns `Err(value)` if already set.
    ///
    /// Equivalent to [`try_set`](Self::try_set); the dual API exists
    /// only for callsite clarity. Use `set` when initialization is
    /// the expected outcome and `try_set` when the call is best-effort.
    ///
    /// # Examples
    ///
    /// ```
    /// use service_kernel::primitives::Global;
    ///
    /// let g: Global<u32> = Global::new();
    /// assert!(g.set(1).is_ok());
    /// assert_eq!(g.set(2), Err(2));
    /// ```
    #[inline]
    pub fn set(&self, value: T) -> Result<(), T> {
        self.inner.set(value)
    }

    /// Sets the contained value if uninitialized.
    ///
    /// Equivalent to [`set`](Self::set). Provided as a separate name so
    /// best-effort callsites read clearly.
    ///
    /// # Examples
    ///
    /// ```
    /// use service_kernel::primitives::Global;
    ///
    /// let g: Global<&'static str> = Global::new();
    /// let _ = g.try_set("hello");
    /// assert_eq!(g.get().copied(), Some("hello"));
    /// ```
    #[inline]
    pub fn try_set(&self, value: T) -> Result<(), T> {
        self.inner.set(value)
    }

    /// Returns a reference to the contained value, or `None` if
    /// uninitialized.
    ///
    /// # Examples
    ///
    /// ```
    /// use service_kernel::primitives::Global;
    ///
    /// let g: Global<u32> = Global::new();
    /// assert!(g.get().is_none());
    /// let _ = g.set(7);
    /// assert_eq!(g.get().copied(), Some(7));
    /// ```
    #[inline]
    pub fn get(&self) -> Option<&T> {
        self.inner.get()
    }

    /// Returns a reference to the contained value.
    ///
    /// Use only when initialization order is part of the kernel
    /// contract — that is, when the caller knows the global has
    /// been set by an earlier boot step.
    ///
    /// # Panics
    ///
    /// Panics if the container is uninitialized. The panic message
    /// is routed through [`lang_lib::t!`] under the key
    /// `kernel.primitives.global.uninitialized`.
    ///
    /// # Examples
    ///
    /// ```should_panic
    /// use service_kernel::primitives::Global;
    ///
    /// let g: Global<u32> = Global::new();
    /// let _ = g.get_unchecked();
    /// ```
    #[inline]
    pub fn get_unchecked(&self) -> &T {
        match self.inner.get() {
            Some(value) => value,
            None => panic!(
                "{}",
                t!(
                    "kernel.primitives.global.uninitialized",
                    fallback: "service-kernel: Global<T> read before initialization"
                )
            ),
        }
    }

    /// Returns `true` once the container holds a value.
    ///
    /// # Examples
    ///
    /// ```
    /// use service_kernel::primitives::Global;
    ///
    /// let g: Global<u32> = Global::new();
    /// assert!(!g.is_initialized());
    /// let _ = g.set(0);
    /// assert!(g.is_initialized());
    /// ```
    #[inline]
    pub fn is_initialized(&self) -> bool {
        self.inner.get().is_some()
    }
}

impl<T> Default for Global<T> {
    /// Returns an uninitialized container, identical to [`Global::new`].
    ///
    /// # Examples
    ///
    /// ```
    /// use service_kernel::primitives::Global;
    ///
    /// let g: Global<u32> = Global::default();
    /// assert!(!g.is_initialized());
    /// ```
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<T: fmt::Debug> fmt::Debug for Global<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.inner.get() {
            Some(value) => f.debug_struct("Global").field("value", value).finish(),
            None => f
                .debug_struct("Global")
                .field("initialized", &false)
                .finish(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::panic::{self, AssertUnwindSafe};
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_set_first_call_succeeds() {
        let g: Global<u32> = Global::new();
        assert!(g.set(7).is_ok());
        assert_eq!(g.get().copied(), Some(7));
    }

    #[test]
    fn test_set_second_call_returns_err() {
        let g: Global<u32> = Global::new();
        assert!(g.set(7).is_ok());
        assert_eq!(g.set(8), Err(8));
        assert_eq!(g.get().copied(), Some(7));
    }

    #[test]
    fn test_try_set_matches_set_semantics() {
        let g: Global<String> = Global::new();
        assert!(g.try_set("first".to_owned()).is_ok());
        assert_eq!(g.try_set("second".to_owned()), Err("second".to_owned()));
        assert_eq!(g.get().map(String::as_str), Some("first"));
    }

    #[test]
    fn test_get_returns_none_before_set() {
        let g: Global<u32> = Global::new();
        assert!(g.get().is_none());
    }

    #[test]
    fn test_is_initialized_flips_on_set() {
        let g: Global<u32> = Global::new();
        assert!(!g.is_initialized());
        let _ = g.set(0);
        assert!(g.is_initialized());
    }

    #[test]
    fn test_get_unchecked_returns_value_after_set() {
        let g: Global<u32> = Global::new();
        let _ = g.set(42);
        assert_eq!(*g.get_unchecked(), 42);
    }

    #[test]
    fn test_get_unchecked_panics_when_uninitialized() {
        let g: Global<u32> = Global::new();
        let result = panic::catch_unwind(AssertUnwindSafe(|| g.get_unchecked()));
        assert!(result.is_err());
    }

    #[test]
    fn test_concurrent_set_exactly_one_winner() {
        let g: Arc<Global<u32>> = Arc::new(Global::new());
        let mut handles = Vec::with_capacity(8);
        for n in 0u32..8 {
            let g = Arc::clone(&g);
            handles.push(thread::spawn(move || g.set(n).is_ok()));
        }
        let wins: usize = handles
            .into_iter()
            .map(|h| usize::from(h.join().unwrap()))
            .sum();
        assert_eq!(wins, 1);
        assert!(g.is_initialized());
    }

    #[test]
    fn test_default_constructs_uninitialized() {
        let g: Global<u32> = Global::default();
        assert!(!g.is_initialized());
    }

    #[test]
    fn test_debug_renders_uninitialized_state() {
        let g: Global<u32> = Global::new();
        let rendered = format!("{:?}", g);
        assert!(rendered.contains("Global"));
        assert!(rendered.contains("initialized"));
        assert!(rendered.contains("false"));
    }

    #[test]
    fn test_debug_renders_initialized_state() {
        let g: Global<u32> = Global::new();
        let _ = g.set(99);
        let rendered = format!("{:?}", g);
        assert!(rendered.contains("Global"));
        assert!(rendered.contains("99"));
    }

    #[test]
    fn test_send_sync_bounds() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<Global<u32>>();
        assert_sync::<Global<u32>>();
    }
}
