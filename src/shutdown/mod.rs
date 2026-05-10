//! Graceful shutdown coordination.
//!
//! When shutdown is signalled — by [`Kernel::shutdown`](crate::kernel::Kernel::shutdown),
//! by an OS signal forwarded through
//! [`Kernel::install_signal_handler`](crate::kernel::Kernel::install_signal_handler),
//! or by a critical worker failure — the kernel runs the following
//! sequence:
//!
//! 1. Mark `KernelState::Stopping` and emit
//!    [`LifecycleEvent::ShutdownStarted`](crate::events::LifecycleEvent::ShutdownStarted).
//! 2. Cancel the shared [`ShutdownToken`] so all workers see it.
//! 3. Run registered [`ShutdownHook`]s in registration order, each
//!    bounded by the remaining grace period.
//! 4. Drain the supervisor's worker set; abort stragglers when the
//!    grace expires.
//! 5. Run each subsystem's `shutdown` in reverse boot order.
//! 6. Emit [`LifecycleEvent::ShutdownCompleted`](crate::events::LifecycleEvent::ShutdownCompleted)
//!    with a summary of what drained vs. aborted.
//! 7. Mark `KernelState::Stopped`.
//!
//! Hook failures are recorded in the [`ShutdownReport`] but do not
//! stop the sequence — the kernel always reaches `Stopped`.

#![cfg(feature = "tokio")]

pub mod coordinator;
pub mod drain;
pub mod hook;
pub mod report;
pub mod token;

pub use coordinator::ShutdownCoordinator;
pub use drain::{drain, DrainOutcome};
pub use hook::{HookError, ShutdownContext, ShutdownHook};
pub use report::ShutdownReport;
pub use token::ShutdownToken;
