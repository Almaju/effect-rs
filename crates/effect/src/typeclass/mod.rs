//! Concrete trait families inspired by Effect-TS's typeclass hierarchy.
//!
//! These are **abstractions for values that compose**: equality
//! ([`Equivalence`]), ordering ([`Order`]), combination
//! ([`Combiner`]), and folding ([`Reducer`]). Each is a regular Rust
//! trait — no HKT emulation. Where multiple notions of "the right
//! equivalence" or "the right ordering" exist for a type, you instantiate
//! a struct rather than picking a single canonical `PartialEq`.

pub mod combiner;
pub mod equivalence;
pub mod order;
pub mod reducer;

pub use combiner::Combiner;
pub use equivalence::Equivalence;
pub use order::Order;
pub use reducer::Reducer;
