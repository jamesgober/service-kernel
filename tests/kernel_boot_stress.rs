//! Stress test for kernel boot ordering with deep dependency chains.
//!
//! Builds a kernel with 100 consumer subsystems forming a 10-deep
//! dependency chain (10 peers per level, each peer at level N depends
//! on every peer at level N-1). Verifies the topological order is
//! respected, then drives the kernel through 50 boot/shutdown cycles
//! to flush out flakiness. Wall-clock cap: 30 seconds.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::time::{Duration, Instant};

use service_kernel::errors::KernelError;
use service_kernel::kernel::{KernelBuilder, KernelContext, Subsystem};
use service_kernel::lifecycle::KernelState;

const LEVELS: usize = 10;
const PEERS_PER_LEVEL: usize = 10;
const ITERATIONS: usize = 50;
const WALL_CLOCK_CAP: Duration = Duration::from_secs(30);

/// Generates the (name, dependencies) pairs for the test graph.
///
/// `LEVELS × PEERS_PER_LEVEL` subsystems are produced. Each peer at
/// level N declares dependencies on every peer at level N-1.
fn graph() -> Vec<(&'static str, &'static [&'static str])> {
    static NAMES: [&str; LEVELS * PEERS_PER_LEVEL] = make_names();
    static DEPS_LEVEL_0: [&str; 0] = [];
    static DEPS: [[&str; PEERS_PER_LEVEL]; LEVELS] = make_deps();

    let mut out = Vec::with_capacity(NAMES.len());
    for level in 0..LEVELS {
        for peer in 0..PEERS_PER_LEVEL {
            let idx = level * PEERS_PER_LEVEL + peer;
            let deps: &[&str] = if level == 0 {
                &DEPS_LEVEL_0
            } else {
                &DEPS[level - 1]
            };
            out.push((NAMES[idx], deps));
        }
    }
    out
}

const fn make_names() -> [&'static str; LEVELS * PEERS_PER_LEVEL] {
    let mut out: [&'static str; LEVELS * PEERS_PER_LEVEL] = [""; LEVELS * PEERS_PER_LEVEL];
    let raw = NAMES_RAW;
    let mut i = 0;
    while i < raw.len() {
        out[i] = raw[i];
        i += 1;
    }
    out
}

const fn make_deps() -> [[&'static str; PEERS_PER_LEVEL]; LEVELS] {
    let mut out: [[&'static str; PEERS_PER_LEVEL]; LEVELS] =
        [[""; PEERS_PER_LEVEL]; LEVELS];
    let raw = NAMES_RAW;
    let mut level = 0;
    while level < LEVELS {
        let mut peer = 0;
        while peer < PEERS_PER_LEVEL {
            out[level][peer] = raw[level * PEERS_PER_LEVEL + peer];
            peer += 1;
        }
        level += 1;
    }
    out
}

const NAMES_RAW: [&str; LEVELS * PEERS_PER_LEVEL] = [
    "n0_0", "n0_1", "n0_2", "n0_3", "n0_4", "n0_5", "n0_6", "n0_7", "n0_8", "n0_9", "n1_0",
    "n1_1", "n1_2", "n1_3", "n1_4", "n1_5", "n1_6", "n1_7", "n1_8", "n1_9", "n2_0", "n2_1",
    "n2_2", "n2_3", "n2_4", "n2_5", "n2_6", "n2_7", "n2_8", "n2_9", "n3_0", "n3_1", "n3_2",
    "n3_3", "n3_4", "n3_5", "n3_6", "n3_7", "n3_8", "n3_9", "n4_0", "n4_1", "n4_2", "n4_3",
    "n4_4", "n4_5", "n4_6", "n4_7", "n4_8", "n4_9", "n5_0", "n5_1", "n5_2", "n5_3", "n5_4",
    "n5_5", "n5_6", "n5_7", "n5_8", "n5_9", "n6_0", "n6_1", "n6_2", "n6_3", "n6_4", "n6_5",
    "n6_6", "n6_7", "n6_8", "n6_9", "n7_0", "n7_1", "n7_2", "n7_3", "n7_4", "n7_5", "n7_6",
    "n7_7", "n7_8", "n7_9", "n8_0", "n8_1", "n8_2", "n8_3", "n8_4", "n8_5", "n8_6", "n8_7",
    "n8_8", "n8_9", "n9_0", "n9_1", "n9_2", "n9_3", "n9_4", "n9_5", "n9_6", "n9_7", "n9_8",
    "n9_9",
];

struct Stub {
    name: &'static str,
    deps: &'static [&'static str],
}

impl Subsystem for Stub {
    fn name(&self) -> &'static str {
        self.name
    }
    fn dependencies(&self) -> &'static [&'static str] {
        self.deps
    }
    fn boot(&self, _ctx: &KernelContext) -> Result<(), KernelError> {
        Ok(())
    }
}

fn build_kernel() -> service_kernel::kernel::Kernel {
    let mut builder = KernelBuilder::new("stress");
    for (name, deps) in graph() {
        builder = builder.with_subsystem(Stub { name, deps });
    }
    builder.build().unwrap()
}

#[test]
fn test_deep_dependency_graph_boots_and_shuts_down_repeatedly() {
    let start = Instant::now();

    for _ in 0..ITERATIONS {
        let kernel = build_kernel();
        let order: Vec<&'static str> = kernel
            .snapshot()
            .subsystems
            .iter()
            .map(|s| s.name)
            .collect();

        // Build a position map and verify every subsystem appears
        // after its declared dependencies.
        let positions: HashMap<&'static str, usize> = order
            .iter()
            .enumerate()
            .map(|(i, name)| (*name, i))
            .collect();
        for (name, deps) in graph() {
            let here = positions[name];
            for dep in deps {
                assert!(
                    positions[dep] < here,
                    "dependency {} of {} is out of order",
                    dep,
                    name,
                );
            }
        }

        kernel.shutdown();
        kernel.run().unwrap();
        assert_eq!(kernel.snapshot().lifecycle.state, KernelState::Stopped);
    }

    let elapsed = start.elapsed();
    assert!(
        elapsed < WALL_CLOCK_CAP,
        "boot stress took {:?}, exceeded {:?}",
        elapsed,
        WALL_CLOCK_CAP,
    );
}
