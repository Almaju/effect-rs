//! Concurrency primitives for sharing state and coordinating between
//! async tasks: [`Ref`], [`Deferred`], and [`Queue`].
//!
//! All three are `Clone` and cheaply share an inner state via `Arc`, so
//! a handle can be passed freely across `flat_map`, `from_fn`, and
//! spawned futures.

pub mod deferred;
pub mod queue;
pub mod reference;

pub use deferred::Deferred;
pub use queue::Queue;
pub use reference::Ref;
