//! A functional effect system for Rust, inspired by Effect-TS.
//!
//! [`Effect<A, E, R>`] represents a lazy, composable, async computation that:
//! - Produces a success value of type `A`
//! - Can fail with a structured [`Cause<E>`] — a typed `Fail(E)`, a defect
//!   (`Die`), or cooperative interruption
//! - Requires an environment of type `R` to execute

pub mod exit;
pub mod sync;

pub use exit::{Cause, Defect, Exit};
pub use sync::{Deferred, Queue, Ref};

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

// ── Running ───────────────────────────────────────────────────

impl<A, E, R> Effect<A, E, R>
where
    A: Send + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    /// Run the effect with a shared environment, producing an [`Exit`].
    pub async fn run(&self, ctx: Arc<R>) -> Exit<A, E> {
        (self.run_fn)(ctx).await
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
                        Exit::Success(a) => (f(a).run_fn)(r2).await,
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
