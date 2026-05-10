//! Topic naming convention for kernel events.
//!
//! Topics are strings, namespaced by the kernel module that owns the
//! event. The convention is:
//!
//! ```text
//! kernel.lifecycle.<state>     // e.g. kernel.lifecycle.running
//! kernel.worker.<event>        // e.g. kernel.worker.started
//! kernel.error.<severity>      // e.g. kernel.error.critical
//! kernel.health.<subsystem>    // e.g. kernel.health.storage
//! kernel.metric.<name>         // e.g. kernel.metric.workers_running
//! kernel.custom                // catch-all root for CustomEvent topics
//! <consumer>.<...>             // consumer-defined topics
//! ```
//!
//! Topics are constructed via the builders in this module rather than
//! `format!`-ed inline. Each builder returns a `&'static str`, so the
//! topic strings are interned at compile time and free to clone, hash,
//! and compare. Suffixes that are not part of the kernel's stable
//! vocabulary fall through to a generic `kernel.<category>.unknown`
//! topic — the routing layer still delivers, but the operator gets
//! a clear "this came from outside our list" signal.
//!
//! New stable suffixes (e.g. when worker events land in Milestone F)
//! get added to the matching builder; the call sites do not change.

/// Returns the topic string for a lifecycle event whose state is
/// described by `state_name`.
///
/// The expected input is the value of [`KernelState::as_str`](crate::lifecycle::KernelState::as_str).
/// Unknown suffixes return the generic
/// `kernel.lifecycle.unknown` topic.
///
/// # Examples
///
/// ```
/// use service_kernel::events::topic::lifecycle_topic;
///
/// assert_eq!(lifecycle_topic("running"), "kernel.lifecycle.running");
/// assert_eq!(lifecycle_topic("nope"), "kernel.lifecycle.unknown");
/// ```
#[must_use]
pub fn lifecycle_topic(state_name: &str) -> &'static str {
    match state_name {
        "created" => "kernel.lifecycle.created",
        "booting" => "kernel.lifecycle.booting",
        "loading" => "kernel.lifecycle.loading",
        "running" => "kernel.lifecycle.running",
        "degraded" => "kernel.lifecycle.degraded",
        "stopping" => "kernel.lifecycle.stopping",
        "stopped" => "kernel.lifecycle.stopped",
        "failed" => "kernel.lifecycle.failed",
        _ => "kernel.lifecycle.unknown",
    }
}

/// Returns the topic string for a worker event named `event_name`.
///
/// `event_name` is expected to be the value of
/// [`WorkerLifecycleEvent::kind`](crate::worker::WorkerLifecycleEvent::kind).
/// Unknown suffixes return `kernel.worker.unknown`.
#[must_use]
pub fn worker_topic(event_name: &str) -> &'static str {
    match event_name {
        "started" => "kernel.worker.started",
        "heartbeat" => "kernel.worker.heartbeat",
        "idle" => "kernel.worker.idle",
        "busy" => "kernel.worker.busy",
        "failed" => "kernel.worker.failed",
        "panicked" => "kernel.worker.panicked",
        "restarted" => "kernel.worker.restarted",
        "stopping" => "kernel.worker.stopping",
        "stopped" => "kernel.worker.stopped",
        "circuit_opened" => "kernel.worker.circuit_opened",
        "circuit_half_opened" => "kernel.worker.circuit_half_opened",
        "circuit_closed" => "kernel.worker.circuit_closed",
        "timeout" => "kernel.worker.timeout",
        "" | "placeholder" => "kernel.worker.placeholder",
        _ => "kernel.worker.unknown",
    }
}

/// Returns the topic string for an error event of the given severity.
///
/// `severity_name` is expected to be the value of
/// [`Severity::as_str`](crate::errors::Severity::as_str). Unknown
/// suffixes return `kernel.error.unknown`.
#[must_use]
pub fn error_topic(severity_name: &str) -> &'static str {
    match severity_name {
        "debug" => "kernel.error.debug",
        "info" => "kernel.error.info",
        "warning" => "kernel.error.warning",
        "error" => "kernel.error.error",
        "critical" => "kernel.error.critical",
        "fatal" => "kernel.error.fatal",
        _ => "kernel.error.unknown",
    }
}

/// Returns the topic string for a health event from the given
/// subsystem.
///
/// `subsystem_name` is either a registered kernel-side name
/// (`"aggregate"`, `"kernel"`, `"lifecycle"`) or the consumer-supplied
/// subsystem name from a `HealthEvent::SubsystemChanged`. Each
/// recognized name maps to a stable topic; the kernel's built-in
/// subsystems and the per-subsystem topics for the consumer's
/// modules all share the `kernel.health.*` namespace.
///
/// Unknown subsystems fall through to `kernel.health.unknown`. New
/// kernel-side subsystem names land here; consumers register their
/// subsystems through the kernel's API at runtime, and the routing
/// table here recognizes them once their names are known.
#[must_use]
pub fn health_topic(subsystem_name: &str) -> &'static str {
    match subsystem_name {
        "aggregate" => "kernel.health.aggregate",
        "kernel" => "kernel.health.kernel",
        "lifecycle" => "kernel.health.lifecycle",
        "storage" => "kernel.health.storage",
        "events" => "kernel.health.events",
        "errors" => "kernel.health.errors",
        "metrics" => "kernel.health.metrics",
        "workers" => "kernel.health.workers",
        "shutdown" => "kernel.health.shutdown",
        _ => "kernel.health.unknown",
    }
}

/// Returns the topic string for a metric event with the given name.
///
/// `metric_name` is the kernel-side metric identifier (e.g.
/// `"kernel.lifecycle.phase"`). The returned topic prepends
/// `kernel.metric.` and strips the leading `kernel.` from the metric
/// name so the topic and metric namespaces stay distinguishable.
/// Unknown names fall through to `kernel.metric.unknown`.
#[must_use]
pub fn metric_topic(metric_name: &str) -> &'static str {
    match metric_name {
        "kernel.lifecycle.phase" => "kernel.metric.lifecycle.phase",
        "kernel.lifecycle.transitions" => "kernel.metric.lifecycle.transitions",
        "kernel.errors" => "kernel.metric.errors",
        "kernel.health.aggregate" => "kernel.metric.health.aggregate",
        "kernel.workers.running" => "kernel.metric.workers.running",
        "kernel.workers.failed" => "kernel.metric.workers.failed",
        "kernel.workers.restarted" => "kernel.metric.workers.restarted",
        "kernel.shutdown.duration" => "kernel.metric.shutdown.duration",
        _ => "kernel.metric.unknown",
    }
}

/// Generic root topic for `CustomEvent`s without a more specific
/// kernel-side topic.
pub const CUSTOM_EVENT_ROOT_TOPIC: &str = "kernel.custom";

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::errors::Severity;
    use crate::lifecycle::KernelState;

    const ALL_STATES: [KernelState; 8] = [
        KernelState::Created,
        KernelState::Booting,
        KernelState::Loading,
        KernelState::Running,
        KernelState::Degraded,
        KernelState::Stopping,
        KernelState::Stopped,
        KernelState::Failed,
    ];

    const ALL_SEVERITIES: [Severity; 6] = [
        Severity::Debug,
        Severity::Info,
        Severity::Warning,
        Severity::Error,
        Severity::Critical,
        Severity::Fatal,
    ];

    #[test]
    fn test_every_kernel_state_has_a_lifecycle_topic() {
        for state in ALL_STATES {
            let topic = lifecycle_topic(state.as_str());
            assert!(topic.starts_with("kernel.lifecycle."));
            assert!(!topic.ends_with("unknown"), "{:?}", state);
        }
    }

    #[test]
    fn test_every_severity_has_an_error_topic() {
        for sev in ALL_SEVERITIES {
            let topic = error_topic(sev.as_str());
            assert!(topic.starts_with("kernel.error."));
            assert!(!topic.ends_with("unknown"), "{:?}", sev);
        }
    }

    #[test]
    fn test_unknown_suffix_returns_unknown_topic() {
        assert_eq!(lifecycle_topic("nope"), "kernel.lifecycle.unknown");
        assert_eq!(worker_topic("nope"), "kernel.worker.unknown");
        assert_eq!(error_topic("nope"), "kernel.error.unknown");
        assert_eq!(health_topic("nope"), "kernel.health.unknown");
        assert_eq!(metric_topic("nope"), "kernel.metric.unknown");
    }

    #[test]
    fn test_topics_are_namespaced() {
        assert!(lifecycle_topic("running").starts_with("kernel."));
        assert!(worker_topic("placeholder").starts_with("kernel."));
        assert!(error_topic("info").starts_with("kernel."));
        assert!(health_topic("kernel").starts_with("kernel."));
        assert!(metric_topic("placeholder").starts_with("kernel."));
    }

    #[test]
    fn test_custom_event_root_topic_is_namespaced() {
        assert_eq!(CUSTOM_EVENT_ROOT_TOPIC, "kernel.custom");
    }
}
