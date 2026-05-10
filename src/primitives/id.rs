//! Strongly-typed kernel-internal identifiers.
//!
//! [`KernelId`], [`WorkerId`], and [`SubsystemId`] are newtype wrappers
//! around [`u64`]. They are not interchangeable: passing a [`WorkerId`]
//! where a [`SubsystemId`] is expected is a compile error.
//!
//! Identifiers are produced by an [`IdGenerator`], which is a single
//! atomic counter shared by all three id types. Identifier uniqueness
//! is guaranteed within a single generator instance for the lifetime
//! of the process. Two different generators produce independent
//! sequences and may collide — this is intentional, and lets tests
//! and embedded consumers run isolated id spaces.
//!
//! These types are deliberately not UUIDs. UUIDs are a consumer
//! concern (record identity, replication, distributed coordination).
//! Kernel-internal references — "which worker is this?", "which
//! subsystem holds this lock?" — fit a `u64` and benefit from being
//! cheap to copy and hash.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

macro_rules! impl_id {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
        pub struct $name(u64);

        impl $name {
            /// Constructs an identifier from a raw value.
            ///
            /// Reserved for tests and adapters that bridge external
            /// numbering schemes. Production code should obtain ids
            /// through [`IdGenerator`].
            #[inline]
            #[must_use]
            pub const fn from_raw(value: u64) -> Self {
                Self(value)
            }

            /// Returns the underlying numeric value.
            #[inline]
            #[must_use]
            pub const fn as_u64(self) -> u64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }
    };
}

impl_id!(
    KernelId,
    "Identifies a kernel-level object (kernel instance, registry entry).\n\
     \n\
     # Examples\n\
     \n\
     ```\n\
     use service_kernel::primitives::{IdGenerator, KernelId};\n\
     use std::collections::HashMap;\n\
     \n\
     let gen = IdGenerator::new();\n\
     let id: KernelId = gen.next_kernel_id();\n\
     let mut by_id: HashMap<KernelId, &str> = HashMap::new();\n\
     let _ = by_id.insert(id, \"primary\");\n\
     assert_eq!(by_id.get(&id).copied(), Some(\"primary\"));\n\
     ```"
);

impl_id!(
    WorkerId,
    "Identifies a supervised worker.\n\
     \n\
     # Examples\n\
     \n\
     ```\n\
     use service_kernel::primitives::{IdGenerator, WorkerId};\n\
     use std::collections::HashMap;\n\
     \n\
     let gen = IdGenerator::new();\n\
     let id: WorkerId = gen.next_worker_id();\n\
     let mut workers: HashMap<WorkerId, &str> = HashMap::new();\n\
     let _ = workers.insert(id, \"replication-listener\");\n\
     assert!(workers.contains_key(&id));\n\
     ```"
);

impl_id!(
    SubsystemId,
    "Identifies a subsystem registered with the kernel.\n\
     \n\
     # Examples\n\
     \n\
     ```\n\
     use service_kernel::primitives::{IdGenerator, SubsystemId};\n\
     use std::collections::HashMap;\n\
     \n\
     let gen = IdGenerator::new();\n\
     let id: SubsystemId = gen.next_subsystem_id();\n\
     let mut subs: HashMap<SubsystemId, &str> = HashMap::new();\n\
     let _ = subs.insert(id, \"storage\");\n\
     assert!(subs.contains_key(&id));\n\
     ```"
);

/// Lock-free generator of kernel-internal identifiers.
///
/// A single shared counter feeds all three id types. The counter is
/// incremented atomically with [`Ordering::Relaxed`] — strict
/// happens-before is not required for id generation; uniqueness within
/// the generator is sufficient.
///
/// At one billion ids per second the `u64` counter wraps in roughly
/// 584 years; the kernel does not check for overflow. If a process
/// generates ids fast enough to wrap, unique-id assumptions are
/// already the smallest of its problems.
///
/// `IdGenerator::new()` is `const`, so the generator can live in a
/// `static` — the typical placement for the kernel's process-wide
/// id space.
///
/// # Examples
///
/// ```
/// use service_kernel::primitives::IdGenerator;
///
/// let gen = IdGenerator::new();
/// let a = gen.next_worker_id();
/// let b = gen.next_worker_id();
/// assert_ne!(a, b);
/// ```
#[derive(Debug)]
pub struct IdGenerator {
    next: AtomicU64,
}

impl IdGenerator {
    /// Creates a new generator with its counter at zero.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            next: AtomicU64::new(0),
        }
    }

    /// Returns the next [`KernelId`] in the sequence.
    #[inline]
    #[must_use]
    pub fn next_kernel_id(&self) -> KernelId {
        KernelId::from_raw(self.next.fetch_add(1, Ordering::Relaxed))
    }

    /// Returns the next [`WorkerId`] in the sequence.
    #[inline]
    #[must_use]
    pub fn next_worker_id(&self) -> WorkerId {
        WorkerId::from_raw(self.next.fetch_add(1, Ordering::Relaxed))
    }

    /// Returns the next [`SubsystemId`] in the sequence.
    #[inline]
    #[must_use]
    pub fn next_subsystem_id(&self) -> SubsystemId {
        SubsystemId::from_raw(self.next.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for IdGenerator {
    /// Returns a fresh generator, identical to [`IdGenerator::new`].
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_kernel_ids_are_monotonic() {
        let gen = IdGenerator::new();
        let a = gen.next_kernel_id();
        let b = gen.next_kernel_id();
        let c = gen.next_kernel_id();
        assert!(a.as_u64() < b.as_u64());
        assert!(b.as_u64() < c.as_u64());
    }

    #[test]
    fn test_worker_ids_are_monotonic() {
        let gen = IdGenerator::new();
        let a = gen.next_worker_id();
        let b = gen.next_worker_id();
        assert!(a.as_u64() < b.as_u64());
    }

    #[test]
    fn test_subsystem_ids_are_monotonic() {
        let gen = IdGenerator::new();
        let a = gen.next_subsystem_id();
        let b = gen.next_subsystem_id();
        assert!(a.as_u64() < b.as_u64());
    }

    #[test]
    fn test_independent_generators_have_independent_sequences() {
        let g1 = IdGenerator::new();
        let g2 = IdGenerator::new();
        assert_eq!(g1.next_kernel_id().as_u64(), 0);
        assert_eq!(g1.next_kernel_id().as_u64(), 1);
        assert_eq!(g2.next_kernel_id().as_u64(), 0);
    }

    #[test]
    fn test_concurrent_generation_is_unique() {
        const THREADS: usize = 8;
        const PER_THREAD: usize = 10_000;

        let gen = Arc::new(IdGenerator::new());
        let mut handles = Vec::with_capacity(THREADS);
        for _ in 0..THREADS {
            let gen = Arc::clone(&gen);
            handles.push(thread::spawn(move || {
                let mut local = Vec::with_capacity(PER_THREAD);
                for _ in 0..PER_THREAD {
                    local.push(gen.next_worker_id());
                }
                local
            }));
        }

        let mut all = HashSet::with_capacity(THREADS * PER_THREAD);
        for h in handles {
            for id in h.join().unwrap() {
                assert!(all.insert(id), "duplicate id generated: {}", id);
            }
        }
        assert_eq!(all.len(), THREADS * PER_THREAD);
    }

    #[test]
    fn test_display_format_for_each_id_type() {
        assert_eq!(KernelId::from_raw(7).to_string(), "KernelId(7)");
        assert_eq!(WorkerId::from_raw(42).to_string(), "WorkerId(42)");
        assert_eq!(SubsystemId::from_raw(3).to_string(), "SubsystemId(3)");
    }

    #[test]
    fn test_debug_matches_display() {
        let id = WorkerId::from_raw(42);
        assert_eq!(format!("{:?}", id), id.to_string());
    }

    #[test]
    fn test_ids_are_copy_hash_eq() {
        let mut set: HashSet<WorkerId> = HashSet::new();
        let id = WorkerId::from_raw(1);
        let copy = id;
        assert!(set.insert(id));
        assert!(!set.insert(copy));
        assert_eq!(id, copy);
    }

    #[test]
    fn test_default_generator_starts_at_zero() {
        let gen = IdGenerator::default();
        assert_eq!(gen.next_worker_id().as_u64(), 0);
    }
}
