//! Integration tests for `service_kernel::primitives::id`.

#![allow(clippy::unwrap_used)]

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::thread;

use service_kernel::primitives::{IdGenerator, KernelId, SubsystemId, WorkerId};

#[test]
fn test_kernel_id_in_hashmap() {
    let gen = IdGenerator::new();
    let id = gen.next_kernel_id();
    let mut map: HashMap<KernelId, &'static str> = HashMap::new();
    let _ = map.insert(id, "primary");
    assert_eq!(map.get(&id).copied(), Some("primary"));
}

#[test]
fn test_worker_id_in_hashmap() {
    let gen = IdGenerator::new();
    let id = gen.next_worker_id();
    let mut map: HashMap<WorkerId, &'static str> = HashMap::new();
    let _ = map.insert(id, "scheduler");
    assert!(map.contains_key(&id));
}

#[test]
fn test_subsystem_id_in_hashmap() {
    let gen = IdGenerator::new();
    let id = gen.next_subsystem_id();
    let mut map: HashMap<SubsystemId, &'static str> = HashMap::new();
    let _ = map.insert(id, "storage");
    assert!(map.contains_key(&id));
}

#[test]
fn test_concurrent_ids_unique_across_threads() {
    const THREADS: usize = 4;
    const PER_THREAD: usize = 5_000;

    let gen = Arc::new(IdGenerator::new());
    let mut handles = Vec::with_capacity(THREADS);
    for _ in 0..THREADS {
        let gen = Arc::clone(&gen);
        handles.push(thread::spawn(move || {
            (0..PER_THREAD)
                .map(|_| gen.next_worker_id())
                .collect::<Vec<_>>()
        }));
    }

    let mut all = HashSet::with_capacity(THREADS * PER_THREAD);
    for h in handles {
        for id in h.join().unwrap() {
            assert!(all.insert(id), "duplicate id observed");
        }
    }
    assert_eq!(all.len(), THREADS * PER_THREAD);
}

#[test]
fn test_display_formats_match_typename_value() {
    assert_eq!(KernelId::from_raw(1).to_string(), "KernelId(1)");
    assert_eq!(WorkerId::from_raw(2).to_string(), "WorkerId(2)");
    assert_eq!(SubsystemId::from_raw(3).to_string(), "SubsystemId(3)");
}
