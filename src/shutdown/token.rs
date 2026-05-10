//! Cooperative-cancel token for the shutdown coordinator.
//!
//! [`ShutdownToken`] is a thin wrapper around
//! [`tokio_util::sync::CancellationToken`]. The wrapper exists so
//! the kernel's API does not leak the Tokio-specific name; the
//! mechanics are identical.

use std::fmt;

use tokio_util::sync::CancellationToken;

/// Cooperative cancellation token used by the shutdown coordinator.
///
/// `signal()` cancels the token — a one-way operation. Subscribers
/// observe via [`is_signalled`](Self::is_signalled) (sync check) or
/// [`signalled`](Self::signalled) (async wait).
///
/// Tokens form a parent/child tree via [`child`](Self::child):
/// cancelling the parent cancels every child, but cancelling a
/// child is local — it does not propagate upward.
#[derive(Clone)]
pub struct ShutdownToken {
    inner: CancellationToken,
}

impl ShutdownToken {
    /// Constructs a fresh, un-signalled token.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: CancellationToken::new(),
        }
    }

    /// Constructs a child of this token.
    ///
    /// The child cancels when the parent does; the parent does not
    /// cancel when the child does.
    #[inline]
    #[must_use]
    pub fn child(&self) -> Self {
        Self {
            inner: self.inner.child_token(),
        }
    }

    /// Signals the token. Idempotent.
    #[inline]
    pub fn signal(&self) {
        self.inner.cancel();
    }

    /// Returns `true` once the token has been signalled (sync check).
    #[inline]
    #[must_use]
    pub fn is_signalled(&self) -> bool {
        self.inner.is_cancelled()
    }

    /// Awaits the signal asynchronously.
    #[inline]
    pub async fn signalled(&self) {
        self.inner.cancelled().await;
    }
}

impl Default for ShutdownToken {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ShutdownToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShutdownToken")
            .field("signalled", &self.is_signalled())
            .finish()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_new_token_not_signalled() {
        assert!(!ShutdownToken::new().is_signalled());
    }

    #[test]
    fn test_signal_flips_is_signalled() {
        let t = ShutdownToken::new();
        t.signal();
        assert!(t.is_signalled());
    }

    #[test]
    fn test_signal_is_idempotent() {
        let t = ShutdownToken::new();
        t.signal();
        t.signal();
        assert!(t.is_signalled());
    }

    #[tokio::test]
    async fn test_signalled_resolves_after_signal() {
        let t = ShutdownToken::new();
        let other = t.clone();
        let join = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            other.signal();
        });
        t.signalled().await;
        join.await.unwrap();
    }

    #[test]
    fn test_child_cancels_when_parent_signalled() {
        let parent = ShutdownToken::new();
        let child = parent.child();
        parent.signal();
        assert!(child.is_signalled());
    }

    #[test]
    fn test_child_signal_does_not_propagate_to_parent() {
        let parent = ShutdownToken::new();
        let child = parent.child();
        child.signal();
        assert!(child.is_signalled());
        assert!(!parent.is_signalled());
    }
}
