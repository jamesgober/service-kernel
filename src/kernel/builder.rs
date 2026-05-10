//! Fluent kernel builder.
//!
//! [`KernelBuilder::new`] returns an empty builder. The consumer
//! chains `with_subsystem`, `with_error_classifier`,
//! `with_metrics_backend`, and `with_shutdown_grace`, then calls
//! [`KernelBuilder::build`] to produce a [`Kernel`]. `build()`
//! validates the configuration, topologically sorts subsystems via
//! Kahn's algorithm, and rejects duplicates, missing dependencies,
//! and cycles with a typed [`BuildError`].

use std::collections::{HashMap, HashSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use lang_lib::Lang;

use crate::errors::{ErrorClassifier, ErrorRegistry, NoopClassifier};
use crate::events::EventDispatcher;
use crate::health::HealthRegistry;
use crate::lifecycle::LifecycleController;
use crate::metrics::{MetricsBackend, MetricsHandle, NoopMetricsBackend};

use super::builtins::{boxed_builtins, BUILTIN_NAMES};
use super::core::{Kernel, KernelInner};
use super::handles::ShutdownInner;
use super::Subsystem;

#[cfg(feature = "tokio")]
use crate::worker::{AsyncWorker, Worker, WorkerSpec};

/// Default shutdown grace period.
const DEFAULT_SHUTDOWN_GRACE: Duration = Duration::from_secs(10);

/// Reasons a [`KernelBuilder::build`] call may fail.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum BuildError {
    /// Two subsystems registered the same name.
    DuplicateSubsystem {
        /// The conflicting name.
        name: &'static str,
    },
    /// A subsystem declared a dependency that no other subsystem
    /// supplies.
    MissingDependency {
        /// Subsystem with the unsatisfiable dependency.
        subsystem: &'static str,
        /// Name of the missing dependency.
        dependency: &'static str,
    },
    /// A dependency cycle was detected.
    DependencyCycle {
        /// Subsystem names participating in the cycle.
        cycle: Vec<&'static str>,
    },
    /// The kernel was built with an empty `name`.
    EmptyName,
    /// A consumer subsystem took one of the reserved built-in names.
    ReservedName {
        /// The reserved name the consumer attempted to register.
        name: &'static str,
    },
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::DuplicateSubsystem { name } => {
                let prefix = Lang::translate(
                    "kernel.builder.duplicate_subsystem",
                    None,
                    Some("duplicate subsystem"),
                );
                write!(f, "{}: {}", prefix, name)
            }
            BuildError::MissingDependency {
                subsystem,
                dependency,
            } => {
                let prefix = Lang::translate(
                    "kernel.builder.missing_dependency",
                    None,
                    Some("missing dependency"),
                );
                write!(f, "{}: {} requires {}", prefix, subsystem, dependency)
            }
            BuildError::DependencyCycle { cycle } => {
                let prefix = Lang::translate(
                    "kernel.builder.dependency_cycle",
                    None,
                    Some("dependency cycle"),
                );
                write!(f, "{}: {}", prefix, cycle.join(" -> "))
            }
            BuildError::EmptyName => {
                let prefix = Lang::translate(
                    "kernel.builder.empty_name",
                    None,
                    Some("kernel name must not be empty"),
                );
                f.write_str(&prefix)
            }
            BuildError::ReservedName { name } => {
                let prefix = Lang::translate(
                    "kernel.builder.reserved_name",
                    None,
                    Some("subsystem name is reserved by the kernel"),
                );
                write!(f, "{}: {}", prefix, name)
            }
        }
    }
}

impl Error for BuildError {}

/// Tokio-only worker registration entry.
#[cfg(feature = "tokio")]
pub(crate) enum PendingWorker {
    /// Synchronous worker.
    Sync(WorkerSpec, Arc<dyn Worker>),
    /// Asynchronous worker.
    Async(WorkerSpec, Arc<dyn AsyncWorker>),
}

/// Fluent builder for [`Kernel`].
pub struct KernelBuilder {
    name: &'static str,
    subsystems: Vec<Box<dyn Subsystem>>,
    error_classifier: Option<Arc<dyn ErrorClassifier>>,
    metrics_backend: Option<MetricsHandle>,
    shutdown_grace: Duration,
    #[cfg(feature = "tokio")]
    workers: Vec<PendingWorker>,
}

impl KernelBuilder {
    /// Constructs a builder with the given kernel name.
    ///
    /// `name` is used as a metrics label and event-topic prefix.
    /// Pass a stable identifier (`"hivedb"`, `"my-service"`), not a
    /// dynamic string.
    #[inline]
    #[must_use]
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            subsystems: Vec::new(),
            error_classifier: None,
            metrics_backend: None,
            shutdown_grace: DEFAULT_SHUTDOWN_GRACE,
            #[cfg(feature = "tokio")]
            workers: Vec::new(),
        }
    }

    /// Registers a synchronous [`Worker`].
    ///
    /// Available when the `tokio` feature is enabled. The supervisor
    /// runs sync workers on Tokio's blocking pool.
    #[cfg(feature = "tokio")]
    #[must_use]
    pub fn with_worker<W: Worker>(mut self, spec: WorkerSpec, worker: W) -> Self {
        self.workers
            .push(PendingWorker::Sync(spec, Arc::new(worker)));
        self
    }

    /// Registers an asynchronous [`AsyncWorker`].
    ///
    /// Available when the `tokio` feature is enabled. The supervisor
    /// runs async workers as ordinary Tokio tasks.
    #[cfg(feature = "tokio")]
    #[must_use]
    pub fn with_async_worker<W: AsyncWorker>(mut self, spec: WorkerSpec, worker: W) -> Self {
        self.workers
            .push(PendingWorker::Async(spec, Arc::new(worker)));
        self
    }

    /// Registers a consumer subsystem.
    #[must_use]
    pub fn with_subsystem<S: Subsystem>(mut self, subsystem: S) -> Self {
        self.subsystems.push(Box::new(subsystem));
        self
    }

    /// Registers an error classifier.
    ///
    /// Without this call the kernel uses [`NoopClassifier`].
    #[must_use]
    pub fn with_error_classifier<C: ErrorClassifier>(mut self, classifier: C) -> Self {
        self.error_classifier = Some(Arc::new(classifier));
        self
    }

    /// Registers a metrics backend.
    ///
    /// Without this call the kernel uses [`NoopMetricsBackend`].
    #[must_use]
    pub fn with_metrics_backend<B: MetricsBackend>(mut self, backend: B) -> Self {
        self.metrics_backend = Some(Arc::new(backend));
        self
    }

    /// Sets the shutdown grace period.
    ///
    /// Currently informational — the supervisor and shutdown drain
    /// land in Milestones F and H. The kernel stores the value so
    /// later milestones don't need to plumb it back in.
    #[must_use]
    pub fn with_shutdown_grace(mut self, grace: Duration) -> Self {
        self.shutdown_grace = grace;
        self
    }

    /// Validates the configuration and constructs the kernel.
    ///
    /// # Errors
    ///
    /// Returns [`BuildError`] on duplicate names, missing
    /// dependencies, dependency cycles, an empty kernel name, or a
    /// consumer using a reserved built-in name.
    pub fn build(self) -> Result<Kernel, BuildError> {
        if self.name.is_empty() {
            return Err(BuildError::EmptyName);
        }

        #[cfg(feature = "tokio")]
        let KernelBuilder {
            name,
            mut subsystems,
            error_classifier,
            metrics_backend,
            shutdown_grace,
            workers,
        } = self;

        #[cfg(not(feature = "tokio"))]
        let KernelBuilder {
            name,
            mut subsystems,
            error_classifier,
            metrics_backend,
            shutdown_grace,
        } = self;

        // Reject consumer use of any built-in name.
        let builtins: HashSet<&'static str> = BUILTIN_NAMES.iter().copied().collect();
        for s in subsystems.iter() {
            if builtins.contains(s.name()) {
                return Err(BuildError::ReservedName { name: s.name() });
            }
        }

        // Prepend built-ins so they sit at the start of the input vector.
        let mut combined: Vec<Box<dyn Subsystem>> = boxed_builtins();
        combined.append(&mut subsystems);

        // Reject duplicates across the combined set.
        let mut seen: HashSet<&'static str> = HashSet::with_capacity(combined.len());
        for s in combined.iter() {
            if !seen.insert(s.name()) {
                return Err(BuildError::DuplicateSubsystem { name: s.name() });
            }
        }

        // Verify each declared dependency exists.
        for s in combined.iter() {
            for dep in s.dependencies() {
                if !seen.contains(dep) {
                    return Err(BuildError::MissingDependency {
                        subsystem: s.name(),
                        dependency: dep,
                    });
                }
            }
        }

        let sorted = topo_sort(&combined)?;
        let ordered: Vec<Box<dyn Subsystem>> = reorder(combined, &sorted);

        // Construct the registries.
        let metrics: MetricsHandle = metrics_backend.unwrap_or_else(|| Arc::new(NoopMetricsBackend));
        let dispatcher = Arc::new(EventDispatcher::new());

        let mut lifecycle_controller = LifecycleController::with_events_and_metrics(
            dispatcher.handle(),
            Arc::clone(&metrics),
        );
        // (no further mutation needed; controller created in one shot.)
        let _ = &mut lifecycle_controller;
        let lifecycle = Arc::new(lifecycle_controller);

        let health = Arc::new(HealthRegistry::with_events(dispatcher.handle()));

        let errors = Arc::new(match error_classifier {
            Some(classifier) => ErrorRegistry::with_classifier(classifier),
            None => ErrorRegistry::with_classifier(Arc::new(NoopClassifier)),
        });

        let shutdown = Arc::new(ShutdownInner::new());

        #[cfg(feature = "tokio")]
        let shutdown_coordinator = Arc::new(
            crate::shutdown::ShutdownCoordinator::new(dispatcher.handle(), shutdown_grace),
        );

        let inner = KernelInner {
            name,
            lifecycle,
            events: dispatcher,
            errors,
            health,
            metrics,
            shutdown,
            subsystems: std::sync::Mutex::new(ordered),
            shutdown_grace,
            #[cfg(feature = "tokio")]
            workers: std::sync::Mutex::new(workers),
            #[cfg(feature = "tokio")]
            shutdown_coordinator,
            #[cfg(feature = "tokio")]
            signal_handler_requested: std::sync::atomic::AtomicBool::new(false),
        };

        Ok(Kernel::from_inner(Arc::new(inner)))
    }
}

/// Returns the order in which subsystems should boot.
///
/// Implements Kahn's algorithm against the directed graph
/// `subsystem -> dependency`. The traversal follows edges in
/// "boot-this-first" direction: a node with no remaining incoming
/// edges (no unsatisfied dependencies) becomes ready.
fn topo_sort(subsystems: &[Box<dyn Subsystem>]) -> Result<Vec<&'static str>, BuildError> {
    let mut indegree: HashMap<&'static str, usize> = HashMap::with_capacity(subsystems.len());
    let mut dependents: HashMap<&'static str, Vec<&'static str>> =
        HashMap::with_capacity(subsystems.len());

    for s in subsystems {
        let _ = indegree.entry(s.name()).or_insert(0);
        let _ = dependents.entry(s.name()).or_default();
    }

    for s in subsystems {
        for dep in s.dependencies() {
            *indegree.entry(s.name()).or_insert(0) += 1;
            dependents.entry(dep).or_default().push(s.name());
        }
    }

    // Seed the queue with nodes that have no dependencies, preserving
    // declaration order so the resulting boot sequence is
    // deterministic for a given input vector.
    let mut queue: VecDeque<&'static str> = subsystems
        .iter()
        .filter(|s| indegree.get(s.name()).copied().unwrap_or(0) == 0)
        .map(|s| s.name())
        .collect();

    let mut sorted: Vec<&'static str> = Vec::with_capacity(subsystems.len());

    while let Some(name) = queue.pop_front() {
        sorted.push(name);
        if let Some(children) = dependents.get(name) {
            for child in children.clone() {
                let entry = indegree.entry(child).or_insert(0);
                if *entry > 0 {
                    *entry -= 1;
                }
                if *entry == 0 {
                    queue.push_back(child);
                }
            }
        }
    }

    if sorted.len() != subsystems.len() {
        // Anything still with non-zero indegree participates in a cycle.
        let cycle: Vec<&'static str> = subsystems
            .iter()
            .filter(|s| indegree.get(s.name()).copied().unwrap_or(0) > 0)
            .map(|s| s.name())
            .collect();
        return Err(BuildError::DependencyCycle { cycle });
    }

    Ok(sorted)
}

/// Reorders `subsystems` to follow `order` (a slice of names).
fn reorder(
    mut subsystems: Vec<Box<dyn Subsystem>>,
    order: &[&'static str],
) -> Vec<Box<dyn Subsystem>> {
    let mut indexed: HashMap<&'static str, Box<dyn Subsystem>> =
        HashMap::with_capacity(subsystems.len());
    while let Some(s) = subsystems.pop() {
        let _ = indexed.insert(s.name(), s);
    }
    order
        .iter()
        .filter_map(|name| indexed.remove(name))
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::errors::KernelError;

    fn assert_send<T: Send>() {}

    struct TestSubsystem {
        name: &'static str,
        deps: &'static [&'static str],
    }

    impl Subsystem for TestSubsystem {
        fn name(&self) -> &'static str {
            self.name
        }
        fn dependencies(&self) -> &'static [&'static str] {
            self.deps
        }
        fn boot(&self, _ctx: &super::super::KernelContext) -> Result<(), KernelError> {
            Ok(())
        }
    }

    #[test]
    fn test_builder_is_send() {
        assert_send::<KernelBuilder>();
    }

    #[test]
    fn test_empty_builder_builds_with_only_builtins() {
        let kernel = KernelBuilder::new("test").build().unwrap();
        let snap = kernel.snapshot();
        assert_eq!(snap.subsystems.len(), BUILTIN_NAMES.len());
    }

    #[test]
    fn test_empty_name_is_rejected() {
        let err = KernelBuilder::new("").build().unwrap_err();
        assert!(matches!(err, BuildError::EmptyName));
    }

    #[test]
    fn test_duplicate_consumer_subsystem_is_rejected() {
        let err = KernelBuilder::new("test")
            .with_subsystem(TestSubsystem {
                name: "store",
                deps: &[],
            })
            .with_subsystem(TestSubsystem {
                name: "store",
                deps: &[],
            })
            .build()
            .unwrap_err();
        assert!(matches!(err, BuildError::DuplicateSubsystem { name: "store" }));
    }

    #[test]
    fn test_consumer_using_reserved_name_is_rejected() {
        let err = KernelBuilder::new("test")
            .with_subsystem(TestSubsystem {
                name: "events",
                deps: &[],
            })
            .build()
            .unwrap_err();
        assert!(matches!(err, BuildError::ReservedName { name: "events" }));
    }

    #[test]
    fn test_missing_dependency_is_rejected() {
        let err = KernelBuilder::new("test")
            .with_subsystem(TestSubsystem {
                name: "store",
                deps: &["missing"],
            })
            .build()
            .unwrap_err();
        assert!(matches!(
            err,
            BuildError::MissingDependency {
                subsystem: "store",
                dependency: "missing",
            }
        ));
    }

    #[test]
    fn test_dependency_cycle_is_detected() {
        let err = KernelBuilder::new("test")
            .with_subsystem(TestSubsystem {
                name: "alpha",
                deps: &["beta"],
            })
            .with_subsystem(TestSubsystem {
                name: "beta",
                deps: &["alpha"],
            })
            .build()
            .unwrap_err();
        match err {
            BuildError::DependencyCycle { cycle } => {
                assert!(cycle.contains(&"alpha"));
                assert!(cycle.contains(&"beta"));
            }
            other => panic!("expected DependencyCycle, got {:?}", other),
        }
    }

    #[test]
    fn test_topo_sort_orders_dependencies_first() {
        let kernel = KernelBuilder::new("test")
            .with_subsystem(TestSubsystem {
                name: "third",
                deps: &["second"],
            })
            .with_subsystem(TestSubsystem {
                name: "second",
                deps: &["first"],
            })
            .with_subsystem(TestSubsystem {
                name: "first",
                deps: &[],
            })
            .build()
            .unwrap();

        let names: Vec<&str> = kernel
            .snapshot()
            .subsystems
            .iter()
            .map(|s| s.name)
            .collect();
        let pos_first = names.iter().position(|n| *n == "first").unwrap();
        let pos_second = names.iter().position(|n| *n == "second").unwrap();
        let pos_third = names.iter().position(|n| *n == "third").unwrap();
        assert!(pos_first < pos_second);
        assert!(pos_second < pos_third);
    }

    #[test]
    fn test_consumer_subsystem_with_builtin_dep_orders_after_builtin() {
        let kernel = KernelBuilder::new("test")
            .with_subsystem(TestSubsystem {
                name: "myapp",
                deps: &["events"],
            })
            .build()
            .unwrap();

        let names: Vec<&str> = kernel
            .snapshot()
            .subsystems
            .iter()
            .map(|s| s.name)
            .collect();
        let pos_events = names.iter().position(|n| *n == "events").unwrap();
        let pos_myapp = names.iter().position(|n| *n == "myapp").unwrap();
        assert!(pos_events < pos_myapp);
    }
}
