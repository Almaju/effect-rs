//! Persistent, immutable collections for the [`effect`] ecosystem.
//!
//! Thin, consistently-named wrappers over [`im`]'s persistent
//! collections. Cheap to clone (Arc-shared structural sharing) and
//! safe to pass around between fibers.
//!
//! - [`Chunk<A>`]   — an immutable sequence backed by `im::Vector`.
//! - [`HashMap<K, V>`] — a persistent HAMT map.
//! - [`HashSet<A>`]    — a persistent HAMT set.

pub mod chunk;
pub mod hash_map;
pub mod hash_set;

pub use chunk::Chunk;
pub use hash_map::HashMap;
pub use hash_set::HashSet;
