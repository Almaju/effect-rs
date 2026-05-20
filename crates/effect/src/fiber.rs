//! Fiber state — per-execution machinery for interruption, scopes, and
//! (eventually) fiber identity.
//!
//! Stored in a tokio task-local so it's transparent to user code:
//! combinators consult it implicitly, and a fresh state is set up for
//! every `Effect::run` / `Effect::execute` call. Sub-tasks inside
//! `tokio::join!` share the parent's state automatically; explicit
//! `tokio::spawn` calls would lose it (which is why `Effect::fork`
//! propagates it explicitly — see Phase 1h).

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

/// A boxed finalizer future.
pub type Finalizer = Pin<Box<dyn Future<Output = ()> + Send>>;

tokio::task_local! {
    pub(crate) static FIBER_STATE: FiberState;
    pub(crate) static SCOPE: Arc<Scope>;
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

// ── Scope ─────────────────────────────────────────────────────────

/// A registry of finalizers that run when the scope closes.
///
/// Finalizers run in **LIFO** order regardless of how the scope ended
/// (success, typed failure, defect, or interruption). Each runs to
/// completion under an uninterruptible mask.
pub struct Scope {
    finalizers: Mutex<Vec<Finalizer>>,
}

impl Default for Scope {
    fn default() -> Self {
        Self::new()
    }
}

impl Scope {
    pub fn new() -> Self {
        Scope {
            finalizers: Mutex::new(Vec::new()),
        }
    }

    /// Register a finalizer. It'll run when the surrounding
    /// [`Effect::scoped`](crate::Effect::scoped) call closes the scope.
    pub fn add_finalizer(&self, f: Finalizer) {
        self.finalizers
            .lock()
            .expect("Scope mutex poisoned")
            .push(f);
    }

    /// Run all registered finalizers in LIFO order, swallowing panics
    /// to ensure subsequent finalizers still get a chance.
    pub async fn close(&self) {
        let mut taken = self
            .finalizers
            .lock()
            .expect("Scope mutex poisoned")
            .drain(..)
            .collect::<Vec<_>>();
        while let Some(fin) = taken.pop() {
            // Each finalizer runs uninterruptibly so a pending
            // interrupt doesn't prevent cleanup.
            INTERRUPTIBLE.scope(false, fin).await;
        }
    }
}

/// Find the current scope, or `None` if `acquire_release` was called
/// outside an `Effect::scoped` region.
pub fn current_scope() -> Option<Arc<Scope>> {
    SCOPE.try_with(|s| s.clone()).ok()
}
