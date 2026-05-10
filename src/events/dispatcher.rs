//! Topic-keyed event dispatcher.
//!
//! `mod-events` dispatches by Rust `TypeId`; the kernel needs
//! topic-string routing. The dispatcher in this module owns the
//! topic-keyed routing table and delivers each [`KernelEvent`] to
//! every subscriber whose registered topic matches the event's
//! [`KernelEvent::topic`].
//!
//! Handlers are isolated against panics with
//! [`std::panic::catch_unwind`], mirroring the panic-safety guarantee
//! `mod-events` provides on its own dispatch path. A panicking
//! handler is silently dropped — it would be unsafe to emit an event
//! describing the panic, since that would create unbounded recursion
//! when the event reaches the same panicking handler.
//!
//! [`EventHandle`] is a cheap, `Clone`able view into the dispatcher
//! that subsystems and workers receive. Handles share the underlying
//! routing table — emitting through one handle reaches every
//! subscriber of every other handle on the same dispatcher.

use std::collections::HashMap;
use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use super::KernelEvent;

/// Boxed dynamic event handler.
type Handler = Box<dyn Fn(&KernelEvent) + Send + Sync + 'static>;

/// Per-topic registration entry.
type Registration = (SubscriptionId, Arc<Handler>);

/// Unique identifier returned by
/// [`EventDispatcher::subscribe`] / [`EventHandle::subscribe`].
///
/// Pass it back to `unsubscribe` to remove the registration.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct SubscriptionId(u64);

/// Internal routing state shared by an [`EventDispatcher`] and its
/// derived [`EventHandle`]s.
struct Inner {
    routes: RwLock<HashMap<&'static str, Vec<Registration>>>,
    by_id: RwLock<HashMap<u64, &'static str>>,
    next_id: AtomicU64,
}

impl Inner {
    fn new() -> Self {
        Self {
            routes: RwLock::new(HashMap::new()),
            by_id: RwLock::new(HashMap::new()),
            next_id: AtomicU64::new(0),
        }
    }

    fn subscribe(&self, topic: &'static str, handler: Handler) -> SubscriptionId {
        let id = SubscriptionId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let arc: Arc<Handler> = Arc::new(handler);
        {
            let mut routes = self
                .routes
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            routes.entry(topic).or_default().push((id, arc));
        }
        {
            let mut by_id = self
                .by_id
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = by_id.insert(id.0, topic);
        }
        id
    }

    fn unsubscribe(&self, id: SubscriptionId) -> bool {
        let topic = {
            let mut by_id = self
                .by_id
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match by_id.remove(&id.0) {
                Some(t) => t,
                None => return false,
            }
        };

        let mut routes = self
            .routes
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(list) = routes.get_mut(topic) {
            if let Some(pos) = list.iter().position(|(sid, _)| *sid == id) {
                let _ = list.remove(pos);
                if list.is_empty() {
                    let _ = routes.remove(topic);
                }
                return true;
            }
        }
        false
    }

    fn subscriber_count(&self, topic: &'static str) -> usize {
        let routes = self
            .routes
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        routes.get(topic).map(Vec::len).unwrap_or(0)
    }

    fn emit(&self, event: KernelEvent) {
        let handlers: Vec<Arc<Handler>> = {
            let routes = self
                .routes
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match routes.get(event.topic()) {
                Some(list) => list.iter().map(|(_, h)| Arc::clone(h)).collect(),
                None => return,
            }
        };
        for handler in handlers {
            let _ = catch_unwind(AssertUnwindSafe(|| handler(&event)));
        }
    }
}

/// Synchronous topic-keyed event dispatcher.
///
/// Construct one per kernel and hand out [`EventHandle`]s to
/// subsystems and workers via [`EventDispatcher::handle`]. Both the
/// dispatcher and the handles share the same routing table.
///
/// `EventDispatcher` is `Send + Sync`. It is intended to live behind
/// an `Arc` (or be moved into the kernel state struct that itself
/// lives behind an `Arc`).
pub struct EventDispatcher {
    inner: Arc<Inner>,
}

impl EventDispatcher {
    /// Constructs a new, empty dispatcher.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner::new()),
        }
    }

    /// Returns a [`Clone`]able handle that shares this dispatcher's
    /// routing table.
    #[inline]
    #[must_use]
    pub fn handle(&self) -> EventHandle {
        EventHandle {
            inner: Arc::clone(&self.inner),
        }
    }

    /// Emits an event. Every subscriber on the event's topic is
    /// invoked synchronously before this call returns.
    #[inline]
    pub fn emit(&self, event: KernelEvent) {
        self.inner.emit(event);
    }

    /// Registers `handler` to receive every event whose topic equals
    /// `topic`.
    pub fn subscribe<F>(&self, topic: &'static str, handler: F) -> SubscriptionId
    where
        F: Fn(&KernelEvent) + Send + Sync + 'static,
    {
        self.inner.subscribe(topic, Box::new(handler))
    }

    /// Removes a subscription. Returns `true` if it existed,
    /// `false` if the id was unknown or already removed.
    pub fn unsubscribe(&self, id: SubscriptionId) -> bool {
        self.inner.unsubscribe(id)
    }

    /// Returns the current number of subscribers on `topic`.
    #[inline]
    #[must_use]
    pub fn subscriber_count(&self, topic: &'static str) -> usize {
        self.inner.subscriber_count(topic)
    }
}

impl Default for EventDispatcher {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for EventDispatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventDispatcher").finish_non_exhaustive()
    }
}

/// Cheap, cloneable handle to an [`EventDispatcher`].
///
/// Subsystems and workers receive an `EventHandle` rather than the
/// dispatcher itself. The handle exposes the same emit / subscribe /
/// unsubscribe surface but does not allow constructing a fresh
/// dispatcher — handles only exist in relation to one.
///
/// All clones of an `EventHandle` share one routing table: a
/// subscription added through any handle is visible to emits through
/// any other handle (or through the originating dispatcher).
#[derive(Clone)]
pub struct EventHandle {
    inner: Arc<Inner>,
}

impl EventHandle {
    /// Emits an event. See [`EventDispatcher::emit`].
    #[inline]
    pub fn emit(&self, event: KernelEvent) {
        self.inner.emit(event);
    }

    /// Subscribes a handler. See [`EventDispatcher::subscribe`].
    pub fn subscribe<F>(&self, topic: &'static str, handler: F) -> SubscriptionId
    where
        F: Fn(&KernelEvent) + Send + Sync + 'static,
    {
        self.inner.subscribe(topic, Box::new(handler))
    }

    /// Removes a subscription. See [`EventDispatcher::unsubscribe`].
    pub fn unsubscribe(&self, id: SubscriptionId) -> bool {
        self.inner.unsubscribe(id)
    }

    /// Returns the current subscriber count for `topic`.
    #[inline]
    #[must_use]
    pub fn subscriber_count(&self, topic: &'static str) -> usize {
        self.inner.subscriber_count(topic)
    }
}

impl fmt::Debug for EventHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventHandle").finish_non_exhaustive()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::events::{ErrorEvent, LifecycleEvent};
    use crate::lifecycle::KernelState;
    use crate::primitives::Instant;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn assert_send_sync<T: Send + Sync>() {}

    fn lifecycle_event(to: KernelState) -> KernelEvent {
        KernelEvent::Lifecycle(LifecycleEvent::Transition {
            from: KernelState::Booting,
            to,
            at: Instant::now(),
        })
    }

    #[test]
    fn test_send_sync() {
        assert_send_sync::<EventDispatcher>();
        assert_send_sync::<EventHandle>();
        assert_send_sync::<SubscriptionId>();
    }

    #[test]
    fn test_emit_to_single_subscriber() {
        let d = EventDispatcher::new();
        let count = Arc::new(AtomicUsize::new(0));
        let cb = Arc::clone(&count);
        let _ = d.subscribe("kernel.lifecycle.running", move |_| {
            let _ = cb.fetch_add(1, Ordering::Relaxed);
        });
        d.emit(lifecycle_event(KernelState::Running));
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_multiple_subscribers_same_topic_all_receive() {
        let d = EventDispatcher::new();
        let total = Arc::new(AtomicUsize::new(0));

        for _ in 0..5 {
            let cb = Arc::clone(&total);
            let _ = d.subscribe("kernel.lifecycle.running", move |_| {
                let _ = cb.fetch_add(1, Ordering::Relaxed);
            });
        }
        d.emit(lifecycle_event(KernelState::Running));
        assert_eq!(total.load(Ordering::Relaxed), 5);
    }

    #[test]
    fn test_subscriber_on_different_topic_is_not_called() {
        let d = EventDispatcher::new();
        let other = Arc::new(AtomicUsize::new(0));
        let cb = Arc::clone(&other);
        let _ = d.subscribe("kernel.lifecycle.failed", move |_| {
            let _ = cb.fetch_add(1, Ordering::Relaxed);
        });
        d.emit(lifecycle_event(KernelState::Running));
        assert_eq!(other.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_unsubscribe_removes_handler() {
        let d = EventDispatcher::new();
        let count = Arc::new(AtomicUsize::new(0));
        let cb = Arc::clone(&count);
        let id = d.subscribe("kernel.lifecycle.running", move |_| {
            let _ = cb.fetch_add(1, Ordering::Relaxed);
        });
        assert!(d.unsubscribe(id));
        assert!(!d.unsubscribe(id));
        d.emit(lifecycle_event(KernelState::Running));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_subscriber_count_round_trips() {
        let d = EventDispatcher::new();
        assert_eq!(d.subscriber_count("kernel.lifecycle.running"), 0);
        let id = d.subscribe("kernel.lifecycle.running", |_| {});
        assert_eq!(d.subscriber_count("kernel.lifecycle.running"), 1);
        assert!(d.unsubscribe(id));
        assert_eq!(d.subscriber_count("kernel.lifecycle.running"), 0);
    }

    #[test]
    fn test_panicking_handler_does_not_crash_dispatcher() {
        let d = EventDispatcher::new();
        let count = Arc::new(AtomicUsize::new(0));
        let cb = Arc::clone(&count);

        let _ = d.subscribe("kernel.lifecycle.running", |_| {
            panic!("intentional");
        });
        let _ = d.subscribe("kernel.lifecycle.running", move |_| {
            let _ = cb.fetch_add(1, Ordering::Relaxed);
        });

        d.emit(lifecycle_event(KernelState::Running));
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_handle_clone_shares_routing() {
        let d = EventDispatcher::new();
        let h1 = d.handle();
        let h2 = h1.clone();
        let count = Arc::new(AtomicUsize::new(0));
        let cb = Arc::clone(&count);
        let _ = h1.subscribe("kernel.lifecycle.running", move |_| {
            let _ = cb.fetch_add(1, Ordering::Relaxed);
        });
        h2.emit(lifecycle_event(KernelState::Running));
        d.emit(lifecycle_event(KernelState::Running));
        assert_eq!(count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_emit_with_no_subscribers_is_noop() {
        let d = EventDispatcher::new();
        d.emit(KernelEvent::Error(ErrorEvent {
            severity: crate::errors::Severity::Info,
            action: crate::errors::ErrorAction::LogOnly,
            message: "nothing here".to_owned(),
        }));
    }

    #[test]
    fn test_default_dispatcher_works() {
        let d = EventDispatcher::default();
        assert_eq!(d.subscriber_count("kernel.lifecycle.running"), 0);
    }

    #[test]
    fn test_subscribe_then_immediately_unsubscribe() {
        let d = EventDispatcher::new();
        let count = Arc::new(AtomicUsize::new(0));
        let cb = Arc::clone(&count);
        let id = d.subscribe("kernel.lifecycle.running", move |_| {
            let _ = cb.fetch_add(1, Ordering::Relaxed);
        });
        assert!(d.unsubscribe(id));
        d.emit(lifecycle_event(KernelState::Running));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }
}
