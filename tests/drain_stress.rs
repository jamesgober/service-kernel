//! Drain stress test: 1000 tasks, mixed quick + slow, single drain
//! call. Verifies drained / aborted counts and bounded elapsed time.

#![cfg(feature = "tokio")]
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use service_kernel::shutdown::drain;
use tokio::task::JoinSet;

const TOTAL: usize = 1000;
const QUICK_FRACTION: usize = 800;
const GRACE: Duration = Duration::from_millis(200);
const ELAPSED_CAP: Duration = Duration::from_millis(500);

#[test]
fn test_drain_handles_thousand_task_mix_within_grace() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    runtime.block_on(async {
        let mut set: JoinSet<()> = JoinSet::new();
        for _ in 0..QUICK_FRACTION {
            let _ = set.spawn(async {
                tokio::time::sleep(Duration::from_millis(10)).await;
            });
        }
        for _ in 0..(TOTAL - QUICK_FRACTION) {
            let _ = set.spawn(async {
                tokio::time::sleep(Duration::from_secs(60)).await;
            });
        }

        let outcome = drain(&mut set, GRACE).await;
        assert_eq!(outcome.total(), TOTAL);
        assert!(
            outcome.drained >= QUICK_FRACTION - 50,
            "expected ~{} drained, got {}",
            QUICK_FRACTION,
            outcome.drained
        );
        assert!(
            outcome.aborted >= (TOTAL - QUICK_FRACTION) - 10,
            "expected ~{} aborted, got {}",
            TOTAL - QUICK_FRACTION,
            outcome.aborted
        );
        assert!(
            outcome.elapsed <= ELAPSED_CAP,
            "drain elapsed {:?}, exceeded cap {:?}",
            outcome.elapsed,
            ELAPSED_CAP
        );
    });
}
