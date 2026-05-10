//! Phase machine and run cycle for the kernel.
//!
//! The lifecycle module owns two related concepts:
//!
//! - **Phase** ([`Phase`]) — coarse stage of the run cycle.
//!   `Idle → Boot → Load → Exec → Shutdown`.
//! - **State** ([`KernelState`]) — fine-grained runtime status.
//!   `Created → Booting → Loading → Running → Degraded → Stopping →
//!   Stopped`, plus the terminal `Failed`.
//!
//! Transitions between states are validated against a constant
//! legal-transition table (see [`is_legal`] and [`assert_legal`]).
//! The [`LifecycleController`] owns the current state, exposes a
//! [`LifecycleSnapshot`] for inspection, and rejects illegal moves
//! with [`TransitionError`].
//!
//! The controller emits no events at this milestone; event wiring
//! lands in Milestone C alongside the kernel's typed event bus.

pub mod controller;
pub mod phase;
pub mod state;
pub mod transition;

pub use controller::{LifecycleController, LifecycleSnapshot};
pub use phase::Phase;
pub use state::KernelState;
pub use transition::{assert_legal, is_legal, TransitionError};
