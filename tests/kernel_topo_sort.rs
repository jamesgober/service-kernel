//! Integration tests for subsystem topological ordering.

#![allow(clippy::unwrap_used)]

use service_kernel::errors::KernelError;
use service_kernel::kernel::{KernelBuilder, KernelContext, Subsystem};

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

fn boot_order(kernel: &service_kernel::kernel::Kernel) -> Vec<&'static str> {
    kernel.snapshot().subsystems.iter().map(|s| s.name).collect()
}

fn position(order: &[&'static str], name: &str) -> usize {
    order.iter().position(|n| *n == name).expect("subsystem not in boot order")
}

#[test]
fn test_diamond_dependencies_resolve() {
    // top -> {left, right}, both -> bottom
    let kernel = KernelBuilder::new("test")
        .with_subsystem(Stub {
            name: "bottom",
            deps: &[],
        })
        .with_subsystem(Stub {
            name: "left",
            deps: &["bottom"],
        })
        .with_subsystem(Stub {
            name: "right",
            deps: &["bottom"],
        })
        .with_subsystem(Stub {
            name: "top",
            deps: &["left", "right"],
        })
        .build()
        .unwrap();

    let order = boot_order(&kernel);
    assert!(position(&order, "bottom") < position(&order, "left"));
    assert!(position(&order, "bottom") < position(&order, "right"));
    assert!(position(&order, "left") < position(&order, "top"));
    assert!(position(&order, "right") < position(&order, "top"));
}

#[test]
fn test_deep_chain_orders_correctly() {
    let kernel = KernelBuilder::new("test")
        .with_subsystem(Stub {
            name: "level0",
            deps: &[],
        })
        .with_subsystem(Stub {
            name: "level1",
            deps: &["level0"],
        })
        .with_subsystem(Stub {
            name: "level2",
            deps: &["level1"],
        })
        .with_subsystem(Stub {
            name: "level3",
            deps: &["level2"],
        })
        .with_subsystem(Stub {
            name: "level4",
            deps: &["level3"],
        })
        .build()
        .unwrap();

    let order = boot_order(&kernel);
    let positions: Vec<usize> = (0..5)
        .map(|i| {
            order
                .iter()
                .position(|n| {
                    *n == match i {
                        0 => "level0",
                        1 => "level1",
                        2 => "level2",
                        3 => "level3",
                        _ => "level4",
                    }
                })
                .unwrap()
        })
        .collect();
    for window in positions.windows(2) {
        assert!(window[0] < window[1]);
    }
}

#[test]
fn test_sibling_subsystems_can_share_a_dependency() {
    let kernel = KernelBuilder::new("test")
        .with_subsystem(Stub {
            name: "common",
            deps: &[],
        })
        .with_subsystem(Stub {
            name: "alpha",
            deps: &["common"],
        })
        .with_subsystem(Stub {
            name: "beta",
            deps: &["common"],
        })
        .with_subsystem(Stub {
            name: "gamma",
            deps: &["common"],
        })
        .build()
        .unwrap();

    let order = boot_order(&kernel);
    let common_pos = position(&order, "common");
    for sibling in ["alpha", "beta", "gamma"] {
        assert!(common_pos < position(&order, sibling));
    }
}

#[test]
fn test_consumer_subsystem_with_multiple_builtin_deps() {
    let kernel = KernelBuilder::new("test")
        .with_subsystem(Stub {
            name: "myapp",
            deps: &["events", "errors", "metrics"],
        })
        .build()
        .unwrap();

    let order = boot_order(&kernel);
    let myapp_pos = position(&order, "myapp");
    for dep in ["events", "errors", "metrics"] {
        assert!(position(&order, dep) < myapp_pos);
    }
}
