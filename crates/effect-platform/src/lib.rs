//! Platform services for [`effect`] — `Clock`, `Random`, `FileSystem`.
//!
//! Each service is a trait you bound `R` by. Tests swap in a fake
//! implementation; production wires in the `Live*` impls.
//!
//! ```ignore
//! use effect_platform::{clock, fs, FileSystem, LiveFileSystem, LiveClock, ClockServices};
//!
//! fn read_then_log<R: FileSystem + ClockServices>(path: &str) -> Effect<String, std::io::Error, R> {
//!     let path = path.to_string();
//!     Effect::block(move |g| {
//!         let path = path.clone();
//!         async move {
//!             let when = g.run(clock::now()).await?;
//!             let contents = g.run(fs::read_to_string(&path)).await?;
//!             Ok(format!("[{when:?}] {contents}"))
//!         }
//!     })
//! }
//! ```

pub mod clock;
pub mod filesystem;
pub mod random;

pub use clock::{Clock, ClockServices, LiveClock};
pub use filesystem::{FileSystem, LiveFileSystem};
pub use random::{LiveRandom, Random};
