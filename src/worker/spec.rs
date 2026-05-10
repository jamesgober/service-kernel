//! Per-worker configuration.
//!
//! [`WorkerSpec`] is built fluently. Reasonable defaults — `Optional`
//! criticality, `OnFailure` restart, exponential backoff — make the
//! shortest possible call site valid:
//!
//! ```
//! use service_kernel::worker::WorkerSpec;
//!
//! let spec = WorkerSpec::new("hive-distro");
//! assert_eq!(spec.name, "hive-distro");
//! ```

use std::time::Duration;

use super::{BackoffPolicy, CircuitPolicy, Criticality, RestartPolicy};

/// Configuration for a single worker.
///
/// Cheap to clone — fields are small value types. The supervisor
/// keeps a `Clone` per registered worker so it can re-spawn after a
/// failure without holding back the consumer's original spec.
#[derive(Debug, Clone)]
pub struct WorkerSpec {
    /// Stable worker name. Used in events, metrics labels, log
    /// lines, and the `KernelSnapshot`.
    pub name: &'static str,
    /// How important this worker is to the kernel.
    pub criticality: Criticality,
    /// Whether and when to restart on failure.
    pub restart: RestartPolicy,
    /// How long to wait between restarts.
    pub backoff: BackoffPolicy,
    /// Optional per-iteration timeout. Reserved for Milestone G.
    pub timeout: Option<Duration>,
    /// Optional heartbeat interval. The watchdog (Milestone G) flags
    /// workers silent for more than `2 × heartbeat_interval`.
    pub heartbeat_interval: Option<Duration>,
    /// Optional circuit-breaker policy. When set, repeated failures
    /// open the breaker and suppress further restarts until the
    /// open duration elapses.
    pub circuit: Option<CircuitPolicy>,
}

impl WorkerSpec {
    /// Constructs a spec with the given name and default settings.
    #[inline]
    #[must_use]
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            criticality: Criticality::Optional,
            restart: RestartPolicy::OnFailure,
            backoff: BackoffPolicy::default(),
            timeout: None,
            heartbeat_interval: None,
            circuit: None,
        }
    }

    /// Sets criticality to [`Criticality::Critical`].
    #[inline]
    #[must_use]
    pub fn critical(mut self) -> Self {
        self.criticality = Criticality::Critical;
        self
    }

    /// Sets criticality to [`Criticality::Essential`].
    #[inline]
    #[must_use]
    pub fn essential(mut self) -> Self {
        self.criticality = Criticality::Essential;
        self
    }

    /// Sets criticality to [`Criticality::Optional`] (the default).
    #[inline]
    #[must_use]
    pub fn optional(mut self) -> Self {
        self.criticality = Criticality::Optional;
        self
    }

    /// Sets criticality to [`Criticality::Background`].
    #[inline]
    #[must_use]
    pub fn background(mut self) -> Self {
        self.criticality = Criticality::Background;
        self
    }

    /// Sets restart policy to [`RestartPolicy::Never`].
    #[inline]
    #[must_use]
    pub fn restart_never(mut self) -> Self {
        self.restart = RestartPolicy::Never;
        self
    }

    /// Sets restart policy to [`RestartPolicy::OnFailure`] (default).
    #[inline]
    #[must_use]
    pub fn restart_on_failure(mut self) -> Self {
        self.restart = RestartPolicy::OnFailure;
        self
    }

    /// Sets restart policy to [`RestartPolicy::Always`].
    #[inline]
    #[must_use]
    pub fn restart_always(mut self) -> Self {
        self.restart = RestartPolicy::Always;
        self
    }

    /// Sets restart policy to
    /// [`RestartPolicy::MaxRetries`] with the given parameters.
    #[inline]
    #[must_use]
    pub fn restart_max_retries(mut self, retries: u32, window: Duration) -> Self {
        self.restart = RestartPolicy::MaxRetries { retries, window };
        self
    }

    /// Sets backoff policy to [`BackoffPolicy::None`].
    #[inline]
    #[must_use]
    pub fn backoff_none(mut self) -> Self {
        self.backoff = BackoffPolicy::None;
        self
    }

    /// Sets backoff policy to [`BackoffPolicy::Fixed`].
    #[inline]
    #[must_use]
    pub fn backoff_fixed(mut self, duration: Duration) -> Self {
        self.backoff = BackoffPolicy::Fixed(duration);
        self
    }

    /// Sets backoff policy to [`BackoffPolicy::Exponential`].
    #[inline]
    #[must_use]
    pub fn backoff_exponential(mut self, base: Duration, max: Duration) -> Self {
        self.backoff = BackoffPolicy::Exponential { base, max };
        self
    }

    /// Sets the heartbeat interval.
    #[inline]
    #[must_use]
    pub fn heartbeat(mut self, interval: Duration) -> Self {
        self.heartbeat_interval = Some(interval);
        self
    }

    /// Sets the per-iteration timeout (Milestone G).
    #[inline]
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Attaches a [`CircuitPolicy`] to this worker.
    ///
    /// The supervisor will open the circuit after the policy's
    /// `failure_threshold` is reached and suppress restarts until
    /// the open duration elapses.
    ///
    /// # Examples
    ///
    /// ```
    /// use service_kernel::worker::{CircuitPolicy, WorkerSpec};
    /// use std::time::Duration;
    ///
    /// let spec = WorkerSpec::new("indexer").circuit(CircuitPolicy::default());
    /// assert!(spec.circuit.is_some());
    /// ```
    #[inline]
    #[must_use]
    pub fn circuit(mut self, policy: CircuitPolicy) -> Self {
        self.circuit = Some(policy);
        self
    }

    /// Removes any configured circuit-breaker policy.
    #[inline]
    #[must_use]
    pub fn no_circuit(mut self) -> Self {
        self.circuit = None;
        self
    }
}

impl Default for WorkerSpec {
    /// Returns a spec with name `""` and default settings.
    ///
    /// Prefer [`WorkerSpec::new`] over `Default::default` so the
    /// worker has a name.
    #[inline]
    fn default() -> Self {
        Self::new("")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_new_uses_documented_defaults() {
        let s = WorkerSpec::new("alpha");
        assert_eq!(s.name, "alpha");
        assert_eq!(s.criticality, Criticality::Optional);
        assert_eq!(s.restart, RestartPolicy::OnFailure);
        assert_eq!(s.backoff, BackoffPolicy::default());
        assert!(s.timeout.is_none());
        assert!(s.heartbeat_interval.is_none());
    }

    #[test]
    fn test_critical_builder_sets_criticality() {
        assert_eq!(
            WorkerSpec::new("a").critical().criticality,
            Criticality::Critical
        );
        assert_eq!(
            WorkerSpec::new("a").essential().criticality,
            Criticality::Essential
        );
        assert_eq!(
            WorkerSpec::new("a").optional().criticality,
            Criticality::Optional
        );
        assert_eq!(
            WorkerSpec::new("a").background().criticality,
            Criticality::Background
        );
    }

    #[test]
    fn test_restart_builders_set_restart_policy() {
        assert_eq!(
            WorkerSpec::new("a").restart_never().restart,
            RestartPolicy::Never
        );
        assert_eq!(
            WorkerSpec::new("a").restart_always().restart,
            RestartPolicy::Always
        );
        let s = WorkerSpec::new("a").restart_max_retries(5, Duration::from_secs(60));
        assert_eq!(
            s.restart,
            RestartPolicy::MaxRetries {
                retries: 5,
                window: Duration::from_secs(60),
            }
        );
    }

    #[test]
    fn test_backoff_builders_set_backoff_policy() {
        assert_eq!(WorkerSpec::new("a").backoff_none().backoff, BackoffPolicy::None);
        assert_eq!(
            WorkerSpec::new("a")
                .backoff_fixed(Duration::from_millis(50))
                .backoff,
            BackoffPolicy::Fixed(Duration::from_millis(50)),
        );
        assert_eq!(
            WorkerSpec::new("a")
                .backoff_exponential(Duration::from_millis(10), Duration::from_secs(1))
                .backoff,
            BackoffPolicy::Exponential {
                base: Duration::from_millis(10),
                max: Duration::from_secs(1),
            }
        );
    }

    #[test]
    fn test_heartbeat_builder_records_interval() {
        let s = WorkerSpec::new("a").heartbeat(Duration::from_secs(5));
        assert_eq!(s.heartbeat_interval, Some(Duration::from_secs(5)));
    }

    #[test]
    fn test_timeout_builder_records_duration() {
        let s = WorkerSpec::new("a").timeout(Duration::from_secs(2));
        assert_eq!(s.timeout, Some(Duration::from_secs(2)));
    }
}
