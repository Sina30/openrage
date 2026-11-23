//! Simulation clock — the particle preview's view of the shared transport.
//!
//! [`SimClock`] is an alias for [`or_transport::Transport`], the one transport type
//! the particle and animation previews share. What the manager relies on:
//!
//! - [`Transport::tick`] returns the *simulated* dt, which the manager caps at
//!   `max_dt` before handing to `EffectInst::update_with_hooks`.
//! - On a loop wrap, `tick` raises `seek_pending` and returns the pre-wrap
//!   elapsed time, so the manager replays the residual inside the wrapped
//!   section rather than skipping it.
//! - [`Transport::take_seek`] is how the manager learns it must restore a
//!   snapshot and replay forward.
//!
//! [`Transport`]: or_transport::Transport

pub use or_transport::{LoopMode, TimeRange, Transport};
pub type SimClock = Transport;
