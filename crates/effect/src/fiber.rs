//! Fiber state — per-execution machinery for interruption, scopes, and
//! (eventually) fiber identity.
//!
//! Stored in a tokio task-local so it's transparent to user code:
//! combinators consult it implicitly, and a fresh state is set up for
//! every `Effect::run` / `Effect::execute` call. Sub-tasks inside
//! `tokio::join!` share the parent's state automatically; explicit
//! `tokio::spawn` calls would lose it (which is why `Effect::fork`
//! propagates it explicitly — see Phase 1h).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

tokio::task_local! {
    pub(crate) static FIBER_STATE: FiberState;
}

/// Per-execution context shared by all combinators within one
/// `Effect::run` (and propagated to children).
#[derive(Clone)]
pub struct FiberState {
    /// Set to `true` by `Fiber::interrupt` (Phase 1h) or by an inner
    /// `Effect::interrupt()` step. Combinators at checkpoints short-
    /// circuit when this is set.
    interrupt: Arc<AtomicBool>,
}

impl Default for FiberState {
    fn default() -> Self {
        Self::new()
    }
}

impl FiberState {
    pub fn new() -> Self {
        FiberState {
            interrupt: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Trigger an interrupt; subsequent checkpoint reads will observe
    /// `true`.
    pub fn signal_interrupt(&self) {
        self.interrupt.store(true, Ordering::SeqCst);
    }

    pub fn is_interrupted(&self) -> bool {
        self.interrupt.load(Ordering::SeqCst)
    }

    /// Get a clone of the inner interrupt flag — used by `Fiber` to
    /// hand a remote interruption handle to the parent.
    pub fn interrupt_handle(&self) -> Arc<AtomicBool> {
        self.interrupt.clone()
    }
}

/// Read the current fiber state, falling back to a fresh default
/// when there's no surrounding execution (e.g. raw `.run_fn` invocation).
pub fn current() -> FiberState {
    FIBER_STATE
        .try_with(|s| s.clone())
        .unwrap_or_else(|_| FiberState::new())
}

/// True if the current fiber has been interrupted *and* we're in an
/// interruptible region. The interruptible-region tracking is a
/// separate task-local because `interruptible`/`uninterruptible`
/// stack lexically.
pub fn interrupted_here() -> bool {
    current().is_interrupted() && interruptible_now()
}

tokio::task_local! {
    pub(crate) static INTERRUPTIBLE: bool;
}

pub fn interruptible_now() -> bool {
    // Default is interruptible; only `Effect::uninterruptible` flips it.
    INTERRUPTIBLE.try_with(|b| *b).unwrap_or(true)
}
