//! Layer builder — composes services into a context, like Effect-TS's `Layer.merge`.
//!
//! Each `.with_*()` call wraps the context in a new type that implements
//! the corresponding service trait. Forwarding impls ensure inner services
//! remain accessible.
//!
//! ```ignore
//! let rt = Layer::new()                    // Layer<()>
//!     .with_repo(InMemoryTodoRepo::new())  // Layer<WithRepo<()>>
//!     .with_logger(ConsoleLogger)          // Layer<WithLogger<WithRepo<()>>>
//!     .into_runtime();                     // Runtime<WithLogger<WithRepo<()>>>
//!
//! // The context now satisfies HasRepo + HasLogger.
//! rt.run(&program()).await;
//! ```

use effect::Runtime;

use super::repo::InMemoryTodoRepo;
use super::traits::{HasLogger, HasRepo};

// ── Concrete service implementations ────────────────────────

/// A simple logger that prints to stdout.
pub struct ConsoleLogger;

// ── Layer wrappers ──────────────────────────────────────────
// Each wrapper adds one service and forwards the rest.

pub struct WithRepo<Inner> {
    repo: InMemoryTodoRepo,
    inner: Inner,
}

pub struct WithLogger<Inner> {
    logger: ConsoleLogger,
    inner: Inner,
}

// ── Direct trait impls ──────────────────────────────────────

impl<I: Send + Sync + 'static> HasRepo for WithRepo<I> {
    fn repo(&self) -> &InMemoryTodoRepo {
        &self.repo
    }
}

impl<I: Send + Sync + 'static> HasLogger for WithLogger<I> {
    fn log(&self, msg: &str) {
        println!("  [LOG] {msg}");
    }
}

// ── Forwarding impls ────────────────────────────────────────
// Each wrapper forwards traits it doesn't directly provide,
// so inner services stay accessible regardless of nesting order.

impl<I: HasLogger + Send + Sync + 'static> HasLogger for WithRepo<I> {
    fn log(&self, msg: &str) {
        self.inner.log(msg)
    }
}

impl<I: HasRepo + Send + Sync + 'static> HasRepo for WithLogger<I> {
    fn repo(&self) -> &InMemoryTodoRepo {
        self.inner.repo()
    }
}

// ── Builder ─────────────────────────────────────────────────

/// Type-safe layer builder.
///
/// The type parameter `Ctx` tracks which services have been added.
/// You can only call `.into_runtime()` when `Ctx` satisfies the
/// bounds your effects need — otherwise you get a compile error.
pub struct Layer<Ctx> {
    ctx: Ctx,
}

impl Layer<()> {
    pub fn new() -> Self {
        Layer { ctx: () }
    }
}

impl<Ctx> Layer<Ctx> {
    /// Add a todo repository.
    pub fn with_repo(self, repo: InMemoryTodoRepo) -> Layer<WithRepo<Ctx>> {
        Layer {
            ctx: WithRepo {
                repo,
                inner: self.ctx,
            },
        }
    }

    /// Add a console logger.
    pub fn with_logger(self) -> Layer<WithLogger<Ctx>> {
        Layer {
            ctx: WithLogger {
                logger: ConsoleLogger,
                inner: self.ctx,
            },
        }
    }

    /// Convert into a `Runtime` for executing effects.
    ///
    /// This only compiles if `Ctx` satisfies all the trait bounds
    /// your effects require (e.g. `HasRepo + HasLogger`).
    pub fn into_runtime(self) -> Runtime<Ctx>
    where
        Ctx: Send + Sync + 'static,
    {
        Runtime::new(self.ctx)
    }

    /// Extract the raw context (for use with `effect.provide()`).
    pub fn into_ctx(self) -> Ctx {
        self.ctx
    }
}
