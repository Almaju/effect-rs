//! A functional effect system for Rust, inspired by Effect-TS.
//!
//! [`Effect<A, E, R>`] represents a lazy, composable, async computation that:
//! - Produces a success value of type `A`
//! - Can fail with a structured [`Cause<E>`] — a typed `Fail(E)`, a defect
//!   (`Die`), or cooperative interruption
//! - Requires an environment of type `R` to execute

pub mod exit;
pub mod fiber;
pub mod refinement;
pub mod schedule;
pub mod sync;
pub mod typeclass;

/// Persistent collections: re-export of [`effect_data`] so users can
/// reach them as `effect::data::Chunk`, etc.
pub mod data {
    pub use effect_data::*;
}

/// Schemas: re-export of [`effect_schema`] so users can reach them as
/// `effect::schema::Schema`, etc.
pub mod schema {
    pub use effect_schema::*;
}

// `effect_stream` depends on `effect` (its terminals produce `Effect`s),
// so it can't be re-exported here without a dependency cycle. Add
// `effect-stream` as a separate Cargo dep and `use effect_stream::Stream;`.

// Both the derive macro and the trait are exported under the name
// `Schema`. They live in different namespaces (macro / type) so they
// don't collide, mirroring how serde re-exports `Serialize`.
pub use effect_macros::{Brand, Newtype, Schema};
pub use effect_schema::{Schema, SchemaError};
pub use fiber::{Fiber, FiberState, Finalizer, Scope};
pub use exit::{Cause, Defect, Exit};
pub use refinement::Refinement;
pub use schedule::{Schedule, ScheduleStep};
pub use sync::{Deferred, Queue, Ref, Semaphore};

// Block is defined alongside Effect below; re-exported via the prelude pattern
// once we have one. For now, `effect::Block<R>` is reachable directly.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::Arc;

/// A boxed, Send-able future.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// A lazy, composable, async effect.
///
/// Type parameters follow Effect-TS conventions:
/// - `A` — Success value
/// - `E` — Typed failure
/// - `R` — Required environment
pub struct Effect<A, E, R> {
    run_fn: Arc<dyn Fn(Arc<R>) -> BoxFuture<Exit<A, E>> + Send + Sync>,
}

impl<A, E, R> Clone for Effect<A, E, R> {
    fn clone(&self) -> Self {
        Effect { run_fn: self.run_fn.clone() }
    }
}

// ── Constructors ──────────────────────────────────────────────

impl<A, E, R> Effect<A, E, R>
where
    A: Send + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    /// Create an effect from an async closure that receives the environment
    /// and returns a [`Result`]. `Err(e)` becomes `Cause::Fail(e)`.
    ///
    /// Use [`Effect::from_fn_exit`] when you need to produce defects or
    /// interruption directly.
    pub fn from_fn<F, Fut>(f: F) -> Self
    where
        F: Fn(Arc<R>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<A, E>> + Send + 'static,
    {
        Effect {
            run_fn: Arc::new(move |r| {
                let fut = f(r);
                Box::pin(async move {
                    match fut.await {
                        Ok(a) => Exit::Success(a),
                        Err(e) => Exit::Failure(Cause::Fail(e)),
                    }
                })
            }),
        }
    }

    /// Create an effect from an async closure that returns an [`Exit`]
    /// directly — for producing defects or interruption.
    pub fn from_fn_exit<F, Fut>(f: F) -> Self
    where
        F: Fn(Arc<R>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Exit<A, E>> + Send + 'static,
    {
        Effect {
            run_fn: Arc::new(move |r| Box::pin(f(r))),
        }
    }

    /// Build an effect from an async closure that receives a [`Gen`]
    /// helper — the do-notation equivalent. Inside the closure, run
    /// inner effects with `g.run(eff).await?` instead of the verbose
    /// `eff.run(ctx.clone()).await.into_typed_result()?` chain.
    ///
    /// ```ignore
    /// Effect::block(|g| async move {
    ///     let a = g.run(some_eff()).await?;
    ///     let b = g.run(other_eff()).await?;
    ///     Ok(a + b)
    /// })
    /// ```
    ///
    /// Like [`Effect::from_fn`], the closure returns a `Result<A, E>`
    /// which is lifted into `Cause::Fail` on `Err`. Inner effects whose
    /// run produces a defect or interruption will **panic** at the
    /// `g.run(...)` call — use `g.run_exit(...)` if you need to inspect
    /// the full cause without panicking.
    pub fn block<F, Fut>(f: F) -> Self
    where
        F: Fn(Block<R>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<A, E>> + Send + 'static,
    {
        Effect::from_fn(move |ctx: Arc<R>| {
            let g = Block { ctx };
            f(g)
        })
    }
}

/// Helper passed to [`Effect::block`] for running inner effects against
/// the captured environment.
pub struct Block<R> {
    ctx: Arc<R>,
}

impl<R: Send + Sync + 'static> Block<R> {
    /// Run an inner effect, extracting a typed `Result`. Panics on
    /// defects or interruption — use [`Block::run_exit`] to handle them
    /// explicitly.
    pub async fn run<A, E>(&self, eff: Effect<A, E, R>) -> Result<A, E>
    where
        A: Send + 'static,
        E: std::fmt::Debug + Send + 'static,
    {
        eff.run(self.ctx.clone()).await.into_typed_result()
    }

    /// Run an inner effect and return the full [`Exit`] without any
    /// conversion.
    pub async fn run_exit<A, E>(&self, eff: Effect<A, E, R>) -> Exit<A, E>
    where
        A: Send + 'static,
        E: Send + 'static,
    {
        eff.run(self.ctx.clone()).await
    }

    /// Borrow the current environment.
    pub fn context(&self) -> Arc<R> {
        self.ctx.clone()
    }
}

impl<A, E, R> Effect<A, E, R>
where
    A: Send + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    /// An effect that immediately succeeds with the given value.
    pub fn succeed(value: A) -> Self
    where
        A: Clone + Sync,
    {
        Effect {
            run_fn: Arc::new(move |_| {
                let v = value.clone();
                Box::pin(async move { Exit::Success(v) })
            }),
        }
    }

    /// An effect that immediately fails with the given typed error.
    pub fn fail(error: E) -> Self
    where
        E: Clone + Sync,
    {
        Effect {
            run_fn: Arc::new(move |_| {
                let e = error.clone();
                Box::pin(async move { Exit::Failure(Cause::Fail(e)) })
            }),
        }
    }

    /// An effect that fails with a defect (a non-recoverable error).
    pub fn die(defect: Defect) -> Self {
        Effect {
            run_fn: Arc::new(move |_| {
                let d = defect.clone();
                Box::pin(async move { Exit::Failure(Cause::Die(d)) })
            }),
        }
    }

    /// An effect that fails with a defect built from a message.
    pub fn die_message(message: impl Into<String>) -> Self {
        Self::die(Defect::new(message))
    }

    /// An effect that immediately fails with `Cause::Interrupt`.
    /// Useful for self-cancellation; see also [`Effect::uninterruptible`]
    /// and [`Fiber::interrupt`](crate::fiber) (Phase 1h) for external
    /// cancellation.
    pub fn interrupt() -> Self {
        Effect {
            run_fn: Arc::new(|_| {
                Box::pin(async move { Exit::Failure(Cause::Interrupt) })
            }),
        }
    }

    /// Run `handler` when `self` completes with a **pure interrupt**
    /// (Cause::Interrupt or a compound made up only of Interrupts).
    /// Typed failures and defects do NOT trigger the handler.
    ///
    /// The handler runs uninterruptibly so cancellation doesn't
    /// prevent its cleanup.
    pub fn on_interrupt<F, Fut>(self, handler: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let run_fn = self.run_fn;
        let handler = Arc::new(handler);
        Effect {
            run_fn: Arc::new(move |r| {
                let run = run_fn.clone();
                let handler = handler.clone();
                Box::pin(async move {
                    let exit = run(r).await;
                    let fire = match &exit {
                        Exit::Failure(c) => c.is_interrupted_only(),
                        _ => false,
                    };
                    if fire {
                        fiber::INTERRUPTIBLE
                            .scope(false, async move { handler().await })
                            .await;
                    }
                    exit
                })
            }),
        }
    }

    /// Like [`Effect::fork`], but the child is **interrupted when the
    /// surrounding [`Effect::scoped`] closes**. Panics at runtime if
    /// called outside a scoped region.
    ///
    /// Useful for "start a background helper that lives only as long
    /// as this scope". The child still runs to completion of its
    /// current step on cancel; pair with `Fiber::join().await` inside
    /// the scope if you need to wait for cleanup.
    pub fn fork_scoped(self) -> Effect<Fiber<A, E>, E, R> {
        let run_fn = self.run_fn;
        Effect {
            run_fn: Arc::new(move |r| {
                let run = run_fn.clone();
                Box::pin(async move {
                    let scope = match fiber::current_scope() {
                        Some(s) => s,
                        None => {
                            return Exit::Failure(Cause::Die(Defect::new(
                                "fork_scoped called outside an Effect::scoped region",
                            )));
                        }
                    };
                    let child_state = FiberState::new();
                    let interrupt_flag = child_state.interrupt_handle();
                    let handle = tokio::spawn(fiber::FIBER_STATE.scope(
                        child_state,
                        async move { run(r).await },
                    ));
                    let intr_for_finalizer = interrupt_flag.clone();
                    scope.add_finalizer(Box::pin(async move {
                        intr_for_finalizer
                            .store(true, std::sync::atomic::Ordering::SeqCst);
                    }));
                    Exit::Success(Fiber { handle, interrupt_flag })
                })
            }),
        }
    }

    /// Run `inner` inside a fresh [`Scope`]. Finalizers registered via
    /// [`acquire_release`] run when this effect completes — for
    /// success, typed failure, defect, or interruption alike — in
    /// LIFO order.
    pub fn scoped(inner: Effect<A, E, R>) -> Self {
        let inner_run = inner.run_fn;
        Effect {
            run_fn: Arc::new(move |r| {
                let run = inner_run.clone();
                Box::pin(async move {
                    let scope = Arc::new(Scope::new());
                    let scope_for_close = scope.clone();
                    let result = fiber::SCOPE
                        .scope(scope, async move { run(r).await })
                        .await;
                    scope_for_close.close().await;
                    result
                })
            }),
        }
    }

    /// An effect that fails with the given pre-built [`Cause`].
    pub fn from_cause(cause: Cause<E>) -> Self
    where
        E: Clone + Sync,
    {
        Effect {
            run_fn: Arc::new(move |_| {
                let c = cause.clone();
                Box::pin(async move { Exit::Failure(c) })
            }),
        }
    }

    /// Create an effect from a synchronous, fallible function.
    ///
    /// **Panics inside `f` are caught** and converted into `Cause::Die`,
    /// keeping the runtime alive. Recover from them with
    /// [`Effect::catch_all_cause`] or [`Effect::sandbox`].
    pub fn sync(f: impl Fn() -> Result<A, E> + Send + Sync + 'static) -> Self {
        Effect {
            run_fn: Arc::new(move |_| {
                let exit = match catch_unwind(AssertUnwindSafe(|| f())) {
                    Ok(Ok(v)) => Exit::Success(v),
                    Ok(Err(e)) => Exit::Failure(Cause::Fail(e)),
                    Err(payload) => Exit::Failure(Cause::Die(Defect::from_panic(payload))),
                };
                Box::pin(async move { exit })
            }),
        }
    }
}

// ── Fork / Race ───────────────────────────────────────────────────

impl<A, E, R> Effect<A, E, R>
where
    A: Send + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    /// Spawn this effect on a new tokio task and return a [`Fiber`]
    /// handle. The fiber has its own [`FiberState`] (own interrupt
    /// flag); a [`Scope`] inherited from the parent is **not**
    /// propagated — child fibers need their own `Effect::scoped`.
    ///
    /// The outer effect's `E` matches the inner's so it threads
    /// cleanly through `Block::run` etc.; spawning itself never
    /// produces a typed failure.
    pub fn fork(self) -> Effect<Fiber<A, E>, E, R> {
        let run_fn = self.run_fn;
        Effect {
            run_fn: Arc::new(move |r| {
                let run = run_fn.clone();
                Box::pin(async move {
                    let child_state = FiberState::new();
                    let interrupt_flag = child_state.interrupt_handle();
                    let handle = tokio::spawn(fiber::FIBER_STATE.scope(
                        child_state,
                        async move { run(r).await },
                    ));
                    Exit::Success(Fiber { handle, interrupt_flag })
                })
            }),
        }
    }

    /// Run `self` and `other` concurrently; return the first to
    /// complete (success **or** failure). The loser is interrupted via
    /// its fiber's interrupt flag.
    pub fn race(self, other: Effect<A, E, R>) -> Effect<A, E, R> {
        let f1 = self.run_fn;
        let f2 = other.run_fn;
        Effect {
            run_fn: Arc::new(move |r| {
                let f1 = f1.clone();
                let f2 = f2.clone();
                let r1 = r.clone();
                let r2 = r;
                Box::pin(async move {
                    let s1 = FiberState::new();
                    let s2 = FiberState::new();
                    let intr1 = s1.interrupt_handle();
                    let intr2 = s2.interrupt_handle();
                    let h1 = tokio::spawn(
                        fiber::FIBER_STATE.scope(s1, async move { (f1)(r1).await }),
                    );
                    let h2 = tokio::spawn(
                        fiber::FIBER_STATE.scope(s2, async move { (f2)(r2).await }),
                    );
                    tokio::select! {
                        res1 = h1 => {
                            intr2.store(
                                true,
                                std::sync::atomic::Ordering::SeqCst,
                            );
                            match res1 {
                                Ok(exit) => exit,
                                Err(je) if je.is_cancelled() => {
                                    Exit::Failure(Cause::Interrupt)
                                }
                                Err(je) => Exit::Failure(Cause::Die(
                                    Defect::new(format!("race fiber panic: {je}"))
                                )),
                            }
                        }
                        res2 = h2 => {
                            intr1.store(
                                true,
                                std::sync::atomic::Ordering::SeqCst,
                            );
                            match res2 {
                                Ok(exit) => exit,
                                Err(je) if je.is_cancelled() => {
                                    Exit::Failure(Cause::Interrupt)
                                }
                                Err(je) => Exit::Failure(Cause::Die(
                                    Defect::new(format!("race fiber panic: {je}"))
                                )),
                            }
                        }
                    }
                })
            }),
        }
    }
}

// ── for_each_par ──────────────────────────────────────────────────

/// Apply `f` to every item with at most `concurrency` effects running
/// concurrently. Returns the results in input order.
///
/// All child tasks share the **parent's interrupt flag**: a failure of
/// any one signals the others to short-circuit (structured concurrency
/// in the small). On the first observed failure the result is returned
/// immediately; in-flight tasks continue running but their results are
/// discarded.
pub fn for_each_par<T, B, E, R, F>(
    items: Vec<T>,
    concurrency: usize,
    f: F,
) -> Effect<Vec<B>, E, R>
where
    T: Clone + Send + Sync + 'static,
    B: Send + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
    F: Fn(T) -> Effect<B, E, R> + Send + Sync + 'static,
{
    let items = Arc::new(items);
    let f = Arc::new(f);
    Effect::from_fn_exit(move |r: Arc<R>| {
        let f = f.clone();
        let items = items.clone();
        Box::pin(async move {
            let parent_state = fiber::current();
            let sem = Arc::new(tokio::sync::Semaphore::new(concurrency.max(1)));
            let mut handles = Vec::with_capacity(items.len());
            for item in items.iter().cloned() {
                let permit = match sem.clone().acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => {
                        return Exit::Failure(Cause::Die(Defect::new(
                            "for_each_par: semaphore closed unexpectedly",
                        )));
                    }
                };
                let f = f.clone();
                let r = r.clone();
                let state = parent_state.clone();
                handles.push(tokio::spawn(fiber::FIBER_STATE.scope(
                    state,
                    async move {
                        let _permit = permit;
                        (f(item).run_fn)(r).await
                    },
                )));
            }

            let mut results = Vec::with_capacity(handles.len());
            for h in handles {
                match h.await {
                    Ok(Exit::Success(b)) => results.push(b),
                    Ok(failure) => {
                        // Signal siblings to short-circuit via the
                        // shared interrupt flag.
                        parent_state.signal_interrupt();
                        return failure.map(|_| unreachable!());
                    }
                    Err(je) if je.is_cancelled() => {
                        return Exit::Failure(Cause::Interrupt);
                    }
                    Err(je) => {
                        return Exit::Failure(Cause::Die(Defect::new(format!(
                            "for_each_par fork panic: {je}"
                        ))));
                    }
                }
            }
            Exit::Success(results)
        })
    })
}

// ── acquire_release ───────────────────────────────────────────────

/// Acquire a resource and register a finalizer to release it when the
/// surrounding [`Effect::scoped`] closes.
///
/// **Panics** at runtime if called outside an `Effect::scoped` region —
/// that's a programmer error, surfaced loudly.
///
/// The finalizer always runs (success, failure, or interruption) and
/// runs uninterruptibly so an in-flight cancel can't tear down a
/// connection mid-cleanup.
pub fn acquire_release<A, E, R, Rel, RelFut>(
    acquire: Effect<A, E, R>,
    release: Rel,
) -> Effect<A, E, R>
where
    A: Clone + Send + Sync + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
    Rel: Fn(A) -> RelFut + Send + Sync + 'static,
    RelFut: Future<Output = ()> + Send + 'static,
{
    let release = Arc::new(release);
    acquire.flat_map(move |acquired: A| {
        let release = release.clone();
        let acquired_for_finalizer = acquired.clone();
        Effect::sync(move || {
            let scope = fiber::current_scope()
                .expect("acquire_release called outside an Effect::scoped region");
            let acquired_clone = acquired_for_finalizer.clone();
            let release_clone = release.clone();
            scope.add_finalizer(Box::pin(async move {
                (release_clone)(acquired_clone).await;
            }));
            Ok(acquired.clone())
        })
    })
}

// ── Running ───────────────────────────────────────────────────

impl<A, E, R> Effect<A, E, R>
where
    A: Send + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    /// Run the effect with a shared environment, producing an [`Exit`].
    ///
    /// At the outermost call, a fresh [`FiberState`] is set up as a
    /// task-local so combinators can observe interruption and (later)
    /// scopes. Nested `run` calls inherit the surrounding state, so
    /// `Block::run`-style threading naturally propagates the interrupt
    /// flag.
    pub async fn run(&self, ctx: Arc<R>) -> Exit<A, E> {
        // If we're already inside a FIBER_STATE scope (nested run from
        // inside a Block closure, say), reuse it.
        if fiber::FIBER_STATE.try_with(|_| ()).is_ok() {
            return (self.run_fn)(ctx).await;
        }
        let state = FiberState::new();
        let run_fn = self.run_fn.clone();
        fiber::FIBER_STATE
            .scope(state, async move { (run_fn)(ctx).await })
            .await
    }

    /// Run the effect with an owned environment.
    pub async fn run_with(&self, ctx: R) -> Exit<A, E> {
        self.run(Arc::new(ctx)).await
    }
}

/// Convenience for effects that require no environment (`R = ()`).
impl<A, E> Effect<A, E, ()>
where
    A: Send + 'static,
    E: Send + 'static,
{
    /// Run an effect that requires no environment, producing an [`Exit`].
    pub async fn execute(&self) -> Exit<A, E> {
        self.run_with(()).await
    }
}

// ── Transformations ───────────────────────────────────────────

impl<A, E, R> Effect<A, E, R>
where
    A: Send + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    /// Transform the success value.
    pub fn map<B>(self, f: impl Fn(A) -> B + Send + Sync + 'static) -> Effect<B, E, R>
    where
        B: Send + 'static,
    {
        let run_fn = self.run_fn;
        let f = Arc::new(f);
        Effect {
            run_fn: Arc::new(move |r| {
                let run = run_fn.clone();
                let f = f.clone();
                Box::pin(async move { run(r).await.map(|a| f(a)) })
            }),
        }
    }

    /// Transform the typed failure. Defects and interruption pass through
    /// unchanged.
    pub fn map_error<E2>(self, f: impl Fn(E) -> E2 + Send + Sync + 'static) -> Effect<A, E2, R>
    where
        E2: Send + 'static,
    {
        let run_fn = self.run_fn;
        let f = Arc::new(f);
        Effect {
            run_fn: Arc::new(move |r| {
                let run = run_fn.clone();
                let f = f.clone();
                Box::pin(async move { run(r).await.map_error(|e| f(e)) })
            }),
        }
    }

    /// Chain effects — monadic bind (`flatMap` in Effect-TS). The
    /// continuation runs only on success; any failure passes through.
    ///
    /// Checks the fiber's interrupt flag before invoking the
    /// continuation; if set (and we're in an interruptible region),
    /// short-circuits with `Cause::Interrupt`.
    pub fn flat_map<B>(
        self,
        f: impl Fn(A) -> Effect<B, E, R> + Send + Sync + 'static,
    ) -> Effect<B, E, R>
    where
        B: Send + 'static,
    {
        let run_fn = self.run_fn;
        let f = Arc::new(f);
        Effect {
            run_fn: Arc::new(move |r| {
                let run = run_fn.clone();
                let f = f.clone();
                let r2 = r.clone();
                Box::pin(async move {
                    match run(r).await {
                        Exit::Success(a) => {
                            if fiber::interrupted_here() {
                                return Exit::Failure(Cause::Interrupt);
                            }
                            (f(a).run_fn)(r2).await
                        }
                        Exit::Failure(c) => Exit::Failure(c),
                    }
                })
            }),
        }
    }

    /// Side-effect on the success value without changing it.
    pub fn tap(self, f: impl Fn(&A) + Send + Sync + 'static) -> Self {
        let run_fn = self.run_fn;
        let f = Arc::new(f);
        Effect {
            run_fn: Arc::new(move |r| {
                let run = run_fn.clone();
                let f = f.clone();
                Box::pin(async move {
                    let exit = run(r).await;
                    if let Exit::Success(ref a) = exit {
                        f(a);
                    }
                    exit
                })
            }),
        }
    }

    /// Replace the success value with a constant.
    pub fn as_value<B: Clone + Send + Sync + 'static>(self, value: B) -> Effect<B, E, R> {
        self.map(move |_| value.clone())
    }

    /// Discard the success value.
    pub fn void(self) -> Effect<(), E, R> {
        self.as_value(())
    }

    /// Mark a region as interruptible. Within it, `flat_map` boundaries
    /// observe the fiber's interrupt flag and may short-circuit with
    /// `Cause::Interrupt`. This is the **default** — use this only to
    /// re-enable interruption inside an `.uninterruptible()` block.
    pub fn interruptible(self) -> Self {
        let run_fn = self.run_fn;
        Effect {
            run_fn: Arc::new(move |r| {
                let run = run_fn.clone();
                Box::pin(async move {
                    fiber::INTERRUPTIBLE
                        .scope(true, async move { run(r).await })
                        .await
                })
            }),
        }
    }

    /// Mark a region as **un**interruptible. Inside it, the interrupt
    /// flag is still observable (via `FiberState::is_interrupted`) but
    /// `flat_map` boundaries no longer short-circuit — critical
    /// sections finish.
    pub fn uninterruptible(self) -> Self {
        let run_fn = self.run_fn;
        Effect {
            run_fn: Arc::new(move |r| {
                let run = run_fn.clone();
                Box::pin(async move {
                    fiber::INTERRUPTIBLE
                        .scope(false, async move { run(r).await })
                        .await
                })
            }),
        }
    }
}

// ── Error Handling ────────────────────────────────────────────

impl<A, E, R> Effect<A, E, R>
where
    A: Send + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    /// Recover from typed failures (`Cause::Fail`). Defects (`Die`) and
    /// interruption pass through unchanged — use
    /// [`Effect::catch_all_cause`] if you need to inspect them.
    ///
    /// **Known limitation:** for compound causes (produced by `zip` when
    /// both arms fail) that contain typed failures, `catch_all` does not
    /// recover and will panic when converting types. Use `catch_all_cause`
    /// or `sandbox` to handle them explicitly.
    pub fn catch_all<E2>(
        self,
        handler: impl Fn(E) -> Effect<A, E2, R> + Send + Sync + 'static,
    ) -> Effect<A, E2, R>
    where
        E2: Send + 'static,
    {
        let run_fn = self.run_fn;
        let handler = Arc::new(handler);
        Effect {
            run_fn: Arc::new(move |r| {
                let run = run_fn.clone();
                let handler = handler.clone();
                let r2 = r.clone();
                Box::pin(async move {
                    match run(r).await {
                        Exit::Success(a) => Exit::Success(a),
                        Exit::Failure(Cause::Fail(e)) => (handler(e).run_fn)(r2).await,
                        Exit::Failure(other) => Exit::Failure(other.change_failure_type()),
                    }
                })
            }),
        }
    }

    /// Recover from any failure — typed, defect, or interruption — by
    /// inspecting the full [`Cause`].
    pub fn catch_all_cause<E2>(
        self,
        handler: impl Fn(Cause<E>) -> Effect<A, E2, R> + Send + Sync + 'static,
    ) -> Effect<A, E2, R>
    where
        E2: Send + 'static,
    {
        let run_fn = self.run_fn;
        let handler = Arc::new(handler);
        Effect {
            run_fn: Arc::new(move |r| {
                let run = run_fn.clone();
                let handler = handler.clone();
                let r2 = r.clone();
                Box::pin(async move {
                    match run(r).await {
                        Exit::Success(a) => Exit::Success(a),
                        Exit::Failure(c) => (handler(c).run_fn)(r2).await,
                    }
                })
            }),
        }
    }

    /// If this effect fails with a typed failure, run the fallback instead.
    /// Defects and interruption pass through.
    pub fn or_else(self, fallback: Effect<A, E, R>) -> Self {
        let run_fn = self.run_fn;
        let fallback_fn = fallback.run_fn;
        Effect {
            run_fn: Arc::new(move |r| {
                let run = run_fn.clone();
                let fallback = fallback_fn.clone();
                let r2 = r.clone();
                Box::pin(async move {
                    match run(r).await {
                        Exit::Success(a) => Exit::Success(a),
                        Exit::Failure(Cause::Fail(_)) => fallback(r2).await,
                        Exit::Failure(other) => Exit::Failure(other),
                    }
                })
            }),
        }
    }

    /// Lift the full [`Cause`] into the typed error channel. After this,
    /// `catch_all` sees defects and interruption alongside typed failures
    /// — they appear as `Cause::Fail(c)` where `c` is the original cause.
    pub fn sandbox(self) -> Effect<A, Cause<E>, R> {
        let run_fn = self.run_fn;
        Effect {
            run_fn: Arc::new(move |r| {
                let run = run_fn.clone();
                Box::pin(async move {
                    match run(r).await {
                        Exit::Success(a) => Exit::Success(a),
                        Exit::Failure(c) => Exit::Failure(Cause::Fail(c)),
                    }
                })
            }),
        }
    }
}

/// Inverse of [`Effect::sandbox`] — flatten a sandboxed effect back to
/// the original cause structure.
impl<A, E, R> Effect<A, Cause<E>, R>
where
    A: Send + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    pub fn unsandbox(self) -> Effect<A, E, R> {
        let run_fn = self.run_fn;
        Effect {
            run_fn: Arc::new(move |r| {
                let run = run_fn.clone();
                Box::pin(async move {
                    match run(r).await {
                        Exit::Success(a) => Exit::Success(a),
                        Exit::Failure(c) => Exit::Failure(flatten_cause_cause(c)),
                    }
                })
            }),
        }
    }
}

fn flatten_cause_cause<E>(c: Cause<Cause<E>>) -> Cause<E> {
    match c {
        Cause::Fail(inner) => inner,
        Cause::Die(d) => Cause::Die(d),
        Cause::Interrupt => Cause::Interrupt,
        Cause::Sequential(a, b) => Cause::Sequential(
            Box::new(flatten_cause_cause(*a)),
            Box::new(flatten_cause_cause(*b)),
        ),
        Cause::Parallel(a, b) => Cause::Parallel(
            Box::new(flatten_cause_cause(*a)),
            Box::new(flatten_cause_cause(*b)),
        ),
    }
}

// ── Parallel Composition ──────────────────────────────────────

impl<A, E, R> Effect<A, E, R>
where
    A: Send + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    /// Run two effects concurrently, collecting both results. If both
    /// fail, the causes are combined as [`Cause::Parallel`].
    pub fn zip<B: Send + 'static>(self, other: Effect<B, E, R>) -> Effect<(A, B), E, R> {
        let run1 = self.run_fn;
        let run2 = other.run_fn;
        Effect {
            run_fn: Arc::new(move |r| {
                let f1 = run1.clone();
                let f2 = run2.clone();
                let r2 = r.clone();
                Box::pin(async move {
                    let (ea, eb) = tokio::join!(f1(r), f2(r2));
                    match (ea, eb) {
                        (Exit::Success(a), Exit::Success(b)) => Exit::Success((a, b)),
                        (Exit::Failure(ca), Exit::Failure(cb)) => {
                            Exit::Failure(Cause::Parallel(Box::new(ca), Box::new(cb)))
                        }
                        (Exit::Failure(c), _) | (_, Exit::Failure(c)) => Exit::Failure(c),
                    }
                })
            }),
        }
    }

    /// Run two effects concurrently, combining results with a function.
    pub fn zip_with<B, C>(
        self,
        other: Effect<B, E, R>,
        f: impl Fn(A, B) -> C + Send + Sync + 'static,
    ) -> Effect<C, E, R>
    where
        B: Send + 'static,
        C: Send + 'static,
    {
        self.zip(other).map(move |(a, b)| f(a, b))
    }
}

// ── Retry / Repeat ────────────────────────────────────────────

impl<A, E, R> Effect<A, E, R>
where
    A: Send + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    /// Retry this effect on typed failure (`Cause::Fail`) according to
    /// the schedule. Defects and interruption are not retried — they
    /// surface immediately.
    ///
    /// On success, returns the value. When the schedule says `Done`,
    /// returns the last failure.
    pub fn retry(self, schedule: Schedule) -> Effect<A, E, R> {
        let run_fn = self.run_fn;
        Effect {
            run_fn: Arc::new(move |r| {
                let run = run_fn.clone();
                let mut sched = schedule.clone();
                Box::pin(async move {
                    loop {
                        let exit = run(r.clone()).await;
                        match exit {
                            Exit::Success(_) => return exit,
                            Exit::Failure(Cause::Fail(_)) => match sched.step() {
                                ScheduleStep::Done => return exit,
                                ScheduleStep::Continue(delay, next) => {
                                    sched = next;
                                    if !delay.is_zero() {
                                        tokio::time::sleep(delay).await;
                                    }
                                }
                            },
                            // Defects, interruption, and compound causes
                            // bypass retry entirely.
                            _ => return exit,
                        }
                    }
                })
            }),
        }
    }

    /// Repeat this effect on success according to the schedule. Returns
    /// the value from the **last** successful iteration.
    ///
    /// If any iteration fails (typed, defect, or interruption), the
    /// failure is returned immediately and the schedule is abandoned.
    pub fn repeat(self, schedule: Schedule) -> Effect<A, E, R>
    where
        A: Clone,
    {
        let run_fn = self.run_fn;
        Effect {
            run_fn: Arc::new(move |r| {
                let run = run_fn.clone();
                let mut sched = schedule.clone();
                Box::pin(async move {
                    // First iteration is unconditional.
                    let mut last = match run(r.clone()).await {
                        Exit::Success(a) => a,
                        other => return other,
                    };
                    loop {
                        match sched.step() {
                            ScheduleStep::Done => return Exit::Success(last),
                            ScheduleStep::Continue(delay, next) => {
                                sched = next;
                                if !delay.is_zero() {
                                    tokio::time::sleep(delay).await;
                                }
                                match run(r.clone()).await {
                                    Exit::Success(a) => last = a,
                                    other => return other,
                                }
                            }
                        }
                    }
                })
            }),
        }
    }
}

// ── Environment ───────────────────────────────────────────────

impl<A, E, R> Effect<A, E, R>
where
    A: Send + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    /// Provide the environment, eliminating the `R` requirement.
    pub fn provide(self, ctx: R) -> Effect<A, E, ()> {
        let run_fn = self.run_fn;
        let ctx = Arc::new(ctx);
        Effect {
            run_fn: Arc::new(move |_: Arc<()>| {
                let run = run_fn.clone();
                let ctx = ctx.clone();
                Box::pin(async move { run(ctx).await })
            }),
        }
    }
}

/// Access the environment (Reader monad's `ask`).
impl<E, R> Effect<Arc<R>, E, R>
where
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    pub fn ask() -> Self {
        Effect {
            run_fn: Arc::new(|r| Box::pin(async move { Exit::Success(r) })),
        }
    }
}

// ── Runtime ───────────────────────────────────────────────────

/// A runtime holds a shared context and executes effects against it.
///
/// ```ignore
/// let rt = Runtime::new(my_context);
/// let exit_a = rt.run(&effect_a).await;
/// let exit_b = rt.run(&effect_b).await;
/// ```
pub struct Runtime<R> {
    ctx: Arc<R>,
}

impl<R: Send + Sync + 'static> Runtime<R> {
    pub fn new(ctx: R) -> Self {
        Runtime { ctx: Arc::new(ctx) }
    }

    pub async fn run<A: Send + 'static, E: Send + 'static>(
        &self,
        effect: &Effect<A, E, R>,
    ) -> Exit<A, E> {
        effect.run(self.ctx.clone()).await
    }
}

// ── Conversions ───────────────────────────────────────────────

impl<A, E, R> From<Result<A, E>> for Effect<A, E, R>
where
    A: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    fn from(result: Result<A, E>) -> Self {
        Effect {
            run_fn: Arc::new(move |_| {
                let r = result.clone();
                Box::pin(async move {
                    match r {
                        Ok(a) => Exit::Success(a),
                        Err(e) => Exit::Failure(Cause::Fail(e)),
                    }
                })
            }),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Basic constructors ─────────────────────────────────

    #[tokio::test]
    async fn succeed_produces_success() {
        let effect = Effect::<_, String, ()>::succeed(42);
        assert_eq!(effect.execute().await.ok(), Some(42));
    }

    #[tokio::test]
    async fn fail_produces_typed_failure() {
        let effect = Effect::<i32, _, ()>::fail("oops".to_string());
        assert_eq!(effect.execute().await.err(), Some("oops".to_string()));
    }

    // ── Transformations ────────────────────────────────────

    #[tokio::test]
    async fn map_transforms_success() {
        let result = Effect::<_, String, ()>::succeed(21)
            .map(|x| x * 2)
            .execute()
            .await;
        assert_eq!(result.ok(), Some(42));
    }

    #[tokio::test]
    async fn flat_map_chains_effects() {
        let result = Effect::<_, String, ()>::succeed(21)
            .flat_map(|x| Effect::succeed(x * 2))
            .execute()
            .await;
        assert_eq!(result.ok(), Some(42));
    }

    #[tokio::test]
    async fn flat_map_short_circuits_on_failure() {
        let result = Effect::<i32, _, ()>::fail("boom".to_string())
            .flat_map(|x| Effect::succeed(x * 2))
            .execute()
            .await;
        assert_eq!(result.err(), Some("boom".to_string()));
    }

    #[tokio::test]
    async fn map_error_transforms_typed_failure() {
        let result = Effect::<i32, _, ()>::fail("err".to_string())
            .map_error(|e| format!("wrapped: {e}"))
            .execute()
            .await;
        assert_eq!(result.err(), Some("wrapped: err".to_string()));
    }

    // ── Error handling ─────────────────────────────────────

    #[tokio::test]
    async fn catch_all_recovers_from_typed_failure() {
        let result = Effect::<i32, String, ()>::fail("oops".to_string())
            .catch_all(|_| Effect::<i32, String, ()>::succeed(0))
            .execute()
            .await;
        assert_eq!(result.ok(), Some(0));
    }

    #[tokio::test]
    async fn or_else_uses_fallback_on_typed_failure() {
        let result = Effect::<_, String, ()>::fail("first".to_string())
            .or_else(Effect::succeed(42))
            .execute()
            .await;
        assert_eq!(result.ok(), Some(42));
    }

    // ── Concurrency ────────────────────────────────────────

    #[tokio::test]
    async fn zip_runs_concurrently() {
        let result = Effect::<_, String, ()>::succeed(1)
            .zip(Effect::succeed(2))
            .execute()
            .await;
        assert_eq!(result.ok(), Some((1, 2)));
    }

    #[tokio::test]
    async fn zip_with_combines_results() {
        let result = Effect::<_, String, ()>::succeed(20)
            .zip_with(Effect::succeed(22), |a, b| a + b)
            .execute()
            .await;
        assert_eq!(result.ok(), Some(42));
    }

    #[tokio::test]
    async fn zip_combines_parallel_failures() {
        let a = Effect::<i32, String, ()>::fail("a".into());
        let b = Effect::<i32, String, ()>::fail("b".into());
        match a.zip(b).execute().await {
            Exit::Failure(Cause::Parallel(left, right)) => {
                assert_eq!(*left, Cause::Fail("a".to_string()));
                assert_eq!(*right, Cause::Fail("b".to_string()));
            }
            other => panic!("expected Parallel cause, got {other:?}"),
        }
    }

    // ── Async constructor ─────────────────────────────────

    #[tokio::test]
    async fn from_fn_async_closure_succeeds() {
        let program = Effect::<i32, String, ()>::from_fn(|_| async move { Ok(42) });
        assert_eq!(program.execute().await.ok(), Some(42));
    }

    #[tokio::test]
    async fn from_fn_async_closure_returning_err_becomes_fail() {
        let program = Effect::<i32, String, ()>::from_fn(|_| async move {
            Err::<i32, _>("boom".to_string())
        });
        assert_eq!(program.execute().await.err(), Some("boom".to_string()));
    }

    // ── gen / do-notation ─────────────────────────────────

    #[tokio::test]
    async fn gen_threads_two_effects() {
        let program = Effect::<i32, String, ()>::block(|g| async move {
            let a = g.run(Effect::<_, String, ()>::succeed(20)).await?;
            let b = g.run(Effect::<_, String, ()>::succeed(22)).await?;
            Ok(a + b)
        });
        assert_eq!(program.execute().await.ok(), Some(42));
    }

    #[tokio::test]
    async fn gen_short_circuits_on_failure() {
        let program = Effect::<i32, String, ()>::block(|g| async move {
            let _ = g.run(Effect::<i32, String, ()>::fail("boom".into())).await?;
            let _ = g.run(Effect::<i32, String, ()>::succeed(42)).await?;
            Ok(0)
        });
        assert_eq!(program.execute().await.err(), Some("boom".to_string()));
    }

    #[tokio::test]
    async fn gen_threads_context() {
        struct Config { value: i32 }
        let leaf = Effect::<i32, String, Config>::from_fn(|ctx| async move { Ok(ctx.value * 2) });
        let program = Effect::<i32, String, Config>::block(move |g| {
            let leaf = leaf.clone();
            async move { g.run(leaf).await }
        });
        assert_eq!(
            program.run_with(Config { value: 21 }).await.ok(),
            Some(42)
        );
    }

    #[tokio::test]
    async fn gen_run_exit_surfaces_die() {
        let program = Effect::<i32, String, ()>::block(|g| async move {
            let exit = g.run_exit(Effect::<i32, String, ()>::die_message("bug")).await;
            match exit {
                Exit::Failure(Cause::Die(_)) => Ok(-1),
                _ => Ok(0),
            }
        });
        assert_eq!(program.execute().await.ok(), Some(-1));
    }

    // ── Retry / Repeat ─────────────────────────────────────

    #[tokio::test]
    async fn retry_recovers_after_n_failures() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_inner = attempts.clone();
        let effect = Effect::<i32, String, ()>::sync(move || {
            let n = attempts_inner.fetch_add(1, Ordering::SeqCst) + 1;
            if n < 3 { Err("not yet".to_string()) } else { Ok(42) }
        });
        let exit = effect.retry(Schedule::recurs(5)).execute().await;
        assert_eq!(exit.ok(), Some(42));
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn retry_gives_up_when_schedule_exhausts() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_inner = attempts.clone();
        let effect = Effect::<i32, String, ()>::sync(move || {
            attempts_inner.fetch_add(1, Ordering::SeqCst);
            Err("always".to_string())
        });
        let exit = effect.retry(Schedule::recurs(2)).execute().await;
        assert_eq!(exit.err(), Some("always".to_string()));
        // 1 initial + 2 retries
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn retry_does_not_retry_defects() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_inner = attempts.clone();
        let effect = Effect::<i32, String, ()>::sync(move || {
            attempts_inner.fetch_add(1, Ordering::SeqCst);
            panic!("bug");
        });
        let exit = effect.retry(Schedule::recurs(5)).execute().await;
        assert!(matches!(exit, Exit::Failure(Cause::Die(_))));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn repeat_runs_n_plus_one_times() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let count = Arc::new(AtomicUsize::new(0));
        let count_inner = count.clone();
        let effect = Effect::<i32, String, ()>::sync(move || {
            let n = count_inner.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(n as i32)
        });
        let exit = effect.repeat(Schedule::recurs(3)).execute().await;
        // initial + 3 repeats = 4 runs; last returned value is 4.
        assert_eq!(exit.ok(), Some(4));
        assert_eq!(count.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn repeat_stops_at_first_failure() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let count = Arc::new(AtomicUsize::new(0));
        let count_inner = count.clone();
        let effect = Effect::<i32, String, ()>::sync(move || {
            let n = count_inner.fetch_add(1, Ordering::SeqCst) + 1;
            if n == 3 { Err("third".to_string()) } else { Ok(n as i32) }
        });
        let exit = effect.repeat(Schedule::recurs(10)).execute().await;
        assert_eq!(exit.err(), Some("third".to_string()));
        assert_eq!(count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn retry_observes_schedule_delays() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::{Duration, Instant};
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_inner = attempts.clone();
        let effect = Effect::<i32, String, ()>::sync(move || {
            let n = attempts_inner.fetch_add(1, Ordering::SeqCst) + 1;
            if n < 3 { Err("nope".to_string()) } else { Ok(0) }
        });
        let start = Instant::now();
        effect
            .retry(Schedule::spaced(Duration::from_millis(20)).max_attempts(5))
            .execute()
            .await;
        let elapsed = start.elapsed();
        // 2 retries × 20ms ≥ 40ms (allow scheduling slack).
        assert!(elapsed >= Duration::from_millis(35), "elapsed {elapsed:?}");
    }

    // ── Environment ────────────────────────────────────────

    #[tokio::test]
    async fn context_is_threaded_through() {
        struct Config { multiplier: i32 }
        let effect = Effect::<i32, String, Config>::from_fn(|ctx| async move {
            Ok(21 * ctx.multiplier)
        });
        let exit = effect.run_with(Config { multiplier: 2 }).await;
        assert_eq!(exit.ok(), Some(42));
    }

    #[tokio::test]
    async fn provide_eliminates_requirement() {
        struct Config { value: i32 }
        let effect = Effect::<i32, String, Config>::from_fn(|ctx| async move { Ok(ctx.value) })
            .provide(Config { value: 42 });
        assert_eq!(effect.execute().await.ok(), Some(42));
    }

    #[tokio::test]
    async fn ask_accesses_environment() {
        struct Config { name: String }
        let effect = Effect::<Arc<Config>, String, Config>::ask().map(|ctx| ctx.name.clone());
        let exit = effect.run_with(Config { name: "hello".into() }).await;
        assert_eq!(exit.ok(), Some("hello".to_string()));
    }

    // ── Misc ───────────────────────────────────────────────

    #[tokio::test]
    async fn tap_performs_side_effect() {
        use std::sync::Mutex;
        let log = Arc::new(Mutex::new(Vec::new()));
        let log_clone = log.clone();
        let result = Effect::<_, String, ()>::succeed(42)
            .tap(move |x| log_clone.lock().unwrap().push(*x))
            .execute()
            .await;
        assert_eq!(result.ok(), Some(42));
        assert_eq!(*log.lock().unwrap(), vec![42]);
    }

    #[tokio::test]
    async fn chaining_composes_pipeline() {
        let program = Effect::<_, String, ()>::succeed(10)
            .map(|x| x + 5)
            .flat_map(|x| Effect::succeed(x * 2))
            .map(|x| x + 12);
        assert_eq!(program.execute().await.ok(), Some(42));
    }

    #[tokio::test]
    async fn from_result_lifts_ok() {
        let effect: Effect<i32, String, ()> = Ok(42).into();
        assert_eq!(effect.execute().await.ok(), Some(42));
    }

    #[tokio::test]
    async fn from_result_lifts_err() {
        let effect: Effect<i32, String, ()> = Err("nope".to_string()).into();
        assert_eq!(effect.execute().await.err(), Some("nope".to_string()));
    }

    #[tokio::test]
    async fn void_discards_value() {
        let result = Effect::<_, String, ()>::succeed(42).void().execute().await;
        assert_eq!(result.ok(), Some(()));
    }

    // ── Cause / Exit / panic handling ─────────────────────

    #[tokio::test]
    async fn sync_catches_panics_as_die() {
        let effect: Effect<i32, String, ()> = Effect::sync(|| panic!("boom"));
        match effect.execute().await {
            Exit::Failure(Cause::Die(d)) => assert!(d.message.contains("boom")),
            other => panic!("expected Die, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn catch_all_ignores_die() {
        let effect = Effect::<i32, String, ()>::sync(|| panic!("boom"))
            .catch_all(|_| Effect::<i32, String, ()>::succeed(0));
        match effect.execute().await {
            Exit::Failure(Cause::Die(_)) => {}
            other => panic!("expected Die to propagate, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn catch_all_cause_catches_die() {
        let effect = Effect::<i32, String, ()>::sync(|| panic!("boom"))
            .catch_all_cause(|c| {
                assert!(c.is_die());
                Effect::<i32, String, ()>::succeed(0)
            });
        assert_eq!(effect.execute().await.ok(), Some(0));
    }

    #[tokio::test]
    async fn die_constructor_produces_die_cause() {
        let effect: Effect<i32, String, ()> = Effect::die_message("oh no");
        match effect.execute().await {
            Exit::Failure(Cause::Die(d)) => assert_eq!(d.message, "oh no"),
            other => panic!("expected Die, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn sandbox_lifts_cause_into_e_channel() {
        let sandboxed: Effect<i32, Cause<String>, ()> =
            Effect::<i32, String, ()>::sync(|| panic!("boom")).sandbox();
        match sandboxed.execute().await {
            Exit::Failure(Cause::Fail(inner)) => assert!(inner.is_die()),
            other => panic!("expected Fail(Die), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unsandbox_restores_original_cause() {
        let original: Effect<i32, String, ()> = Effect::sync(|| panic!("boom"));
        let round_tripped = original.sandbox().unsandbox();
        match round_tripped.execute().await {
            Exit::Failure(Cause::Die(_)) => {}
            other => panic!("expected Die after round-trip, got {other:?}"),
        }
    }

    // ── Interrupt ──────────────────────────────────────────

    #[tokio::test]
    async fn interrupt_constructor_produces_interrupt_cause() {
        let effect = Effect::<i32, String, ()>::interrupt();
        match effect.execute().await {
            Exit::Failure(Cause::Interrupt) => {}
            other => panic!("expected Interrupt, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn interrupt_propagates_through_flat_map() {
        let program = Effect::<i32, String, ()>::interrupt()
            .flat_map(|_| Effect::<i32, String, ()>::succeed(99));
        match program.execute().await {
            Exit::Failure(Cause::Interrupt) => {}
            other => panic!("expected Interrupt, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn interrupt_flag_short_circuits_at_flat_map_boundary() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let after = Arc::new(AtomicUsize::new(0));
        let after_clone = after.clone();
        let program = Effect::<_, String, ()>::sync(|| {
            // Set the interrupt flag mid-stream.
            crate::fiber::current().signal_interrupt();
            Ok::<i32, String>(1)
        })
        .flat_map(move |_| {
            let after_clone = after_clone.clone();
            Effect::<_, String, ()>::sync(move || {
                after_clone.fetch_add(1, Ordering::SeqCst);
                Ok(2)
            })
        });

        match program.execute().await {
            Exit::Failure(Cause::Interrupt) => {}
            other => panic!("expected Interrupt, got {other:?}"),
        }
        // The post-interrupt step must NOT have run.
        assert_eq!(after.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn uninterruptible_finishes_critical_section() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let after = Arc::new(AtomicUsize::new(0));
        let after_clone = after.clone();
        let critical = Effect::<_, String, ()>::sync(|| {
            crate::fiber::current().signal_interrupt();
            Ok::<i32, String>(1)
        })
        .flat_map(move |_| {
            let after_clone = after_clone.clone();
            Effect::<_, String, ()>::sync(move || {
                after_clone.fetch_add(1, Ordering::SeqCst);
                Ok(2)
            })
        })
        .uninterruptible();

        // Inside the uninterruptible region, the flag is set but the
        // boundary does not short-circuit.
        let exit = critical.execute().await;
        assert_eq!(exit.ok(), Some(2));
        assert_eq!(after.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn interruptible_undoes_outer_uninterruptible() {
        // interruptible/uninterruptible nest lexically: the
        // **innermost** wrapping wins. So an inner .interruptible()
        // re-enables short-circuit even inside an outer
        // .uninterruptible().
        use std::sync::atomic::{AtomicUsize, Ordering};
        let after = Arc::new(AtomicUsize::new(0));
        let after_clone = after.clone();
        let program = Effect::<_, String, ()>::sync(|| {
            crate::fiber::current().signal_interrupt();
            Ok::<i32, String>(1)
        })
        .flat_map(move |_| {
            let after_clone = after_clone.clone();
            Effect::<_, String, ()>::sync(move || {
                after_clone.fetch_add(1, Ordering::SeqCst);
                Ok(2)
            })
        })
        .interruptible()
        .uninterruptible();

        match program.execute().await {
            Exit::Failure(Cause::Interrupt) => {}
            other => panic!("expected Interrupt from innermost interruptible, got {other:?}"),
        }
        assert_eq!(after.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn or_else_does_not_recover_from_die() {
        let effect = Effect::<i32, String, ()>::sync(|| panic!("boom"))
            .or_else(Effect::succeed(0));
        match effect.execute().await {
            Exit::Failure(Cause::Die(_)) => {}
            other => panic!("expected Die to propagate, got {other:?}"),
        }
    }
}
