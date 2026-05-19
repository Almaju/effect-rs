//! A functional effect system for Rust, inspired by Effect-TS.
//!
//! [`Effect<A, E, R>`] represents a lazy, composable, async computation that:
//! - Produces a success value of type `A`
//! - Can fail with an error of type `E`
//! - Requires an environment of type `R` to execute

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// A boxed, Send-able future.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// A lazy, composable, async effect.
///
/// Type parameters follow Effect-TS conventions:
/// - `A` — Success value
/// - `E` — Error type
/// - `R` — Required environment
pub struct Effect<A, E, R> {
    run_fn: Arc<dyn Fn(Arc<R>) -> BoxFuture<Result<A, E>> + Send + Sync>,
}

impl<A, E, R> Clone for Effect<A, E, R> {
    fn clone(&self) -> Self {
        Effect {
            run_fn: self.run_fn.clone(),
        }
    }
}

// ── Constructors ──────────────────────────────────────────────

impl<A, E, R> Effect<A, E, R>
where
    A: Send + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    /// Create an effect from an async closure that receives the environment.
    ///
    /// This is the most general constructor — similar to `Effect.gen` in Effect-TS.
    /// Use the `?` operator for early returns on error, just like `yield*` in
    /// Effect-TS generators.
    pub fn from_fn<F, Fut>(f: F) -> Self
    where
        F: Fn(Arc<R>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<A, E>> + Send + 'static,
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
                Box::pin(async move { Ok(v) })
            }),
        }
    }

    /// An effect that immediately fails with the given error.
    pub fn fail(error: E) -> Self
    where
        E: Clone + Sync,
    {
        Effect {
            run_fn: Arc::new(move |_| {
                let e = error.clone();
                Box::pin(async move { Err(e) })
            }),
        }
    }

    /// Create an effect from a synchronous, fallible function.
    pub fn sync(f: impl Fn() -> Result<A, E> + Send + Sync + 'static) -> Self {
        Effect {
            run_fn: Arc::new(move |_| {
                let result = f();
                Box::pin(async move { result })
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
    /// Run the effect with a shared environment.
    pub async fn run(&self, ctx: Arc<R>) -> Result<A, E> {
        (self.run_fn)(ctx).await
    }

    /// Run the effect with an owned environment value.
    pub async fn run_with(&self, ctx: R) -> Result<A, E> {
        self.run(Arc::new(ctx)).await
    }
}

/// Convenience methods for effects that require no environment (`R = ()`).
impl<A, E> Effect<A, E, ()>
where
    A: Send + 'static,
    E: Send + 'static,
{
    /// Run an effect that requires no environment.
    pub async fn execute(&self) -> Result<A, E> {
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

    /// Transform the error value.
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
                Box::pin(async move { run(r).await.map_err(|e| f(e)) })
            }),
        }
    }

    /// Chain effects — monadic bind (`flatMap` in Effect-TS).
    ///
    /// The function `f` receives the success value and returns a new effect.
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
                        Ok(a) => (f(a).run_fn)(r2).await,
                        Err(e) => Err(e),
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
                    let result = run(r).await;
                    if let Ok(ref a) = result {
                        f(a);
                    }
                    result
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
    /// Recover from all errors by mapping them into a new effect.
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
                        Ok(a) => Ok(a),
                        Err(e) => (handler(e).run_fn)(r2).await,
                    }
                })
            }),
        }
    }

    /// If this effect fails, try the fallback instead.
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
                        Ok(a) => Ok(a),
                        Err(_) => fallback(r2).await,
                    }
                })
            }),
        }
    }
}

// ── Parallel Composition ──────────────────────────────────────

impl<A, E, R> Effect<A, E, R>
where
    A: Send + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    /// Run two effects concurrently, collecting both results.
    pub fn zip<B: Send + 'static>(self, other: Effect<B, E, R>) -> Effect<(A, B), E, R> {
        let run1 = self.run_fn;
        let run2 = other.run_fn;
        Effect {
            run_fn: Arc::new(move |r| {
                let f1 = run1.clone();
                let f2 = run2.clone();
                let r2 = r.clone();
                Box::pin(async move {
                    let (a, b) = tokio::join!(f1(r), f2(r2));
                    Ok((a?, b?))
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
    ///
    /// Converts `Effect<A, E, R>` → `Effect<A, E, ()>`.
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
            run_fn: Arc::new(|r| Box::pin(async move { Ok(r) })),
        }
    }
}

// ── Runtime ───────────────────────────────────────────────────

/// A runtime holds a shared context and executes effects against it.
///
/// Equivalent to Effect-TS's `Runtime` — built from a `Layer`,
/// reused to run many effects with the same environment.
///
/// ```ignore
/// let rt = Runtime::new(my_context);
/// let a = rt.run(&effect_a).await?;
/// let b = rt.run(&effect_b).await?;
/// ```
pub struct Runtime<R> {
    ctx: Arc<R>,
}

impl<R: Send + Sync + 'static> Runtime<R> {
    /// Create a runtime from an owned context value.
    pub fn new(ctx: R) -> Self {
        Runtime { ctx: Arc::new(ctx) }
    }

    /// Run an effect using this runtime's context.
    pub async fn run<A: Send + 'static, E: Send + 'static>(
        &self,
        effect: &Effect<A, E, R>,
    ) -> Result<A, E> {
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
                Box::pin(async move { r })
            }),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn succeed_produces_ok() {
        let effect = Effect::<_, String, ()>::succeed(42);
        assert_eq!(effect.execute().await, Ok(42));
    }

    #[tokio::test]
    async fn fail_produces_err() {
        let effect = Effect::<i32, _, ()>::fail("oops".to_string());
        assert_eq!(effect.execute().await, Err("oops".to_string()));
    }

    #[tokio::test]
    async fn map_transforms_success() {
        let result = Effect::<_, String, ()>::succeed(21)
            .map(|x| x * 2)
            .execute()
            .await;
        assert_eq!(result, Ok(42));
    }

    #[tokio::test]
    async fn flat_map_chains_effects() {
        let result = Effect::<_, String, ()>::succeed(21)
            .flat_map(|x| Effect::succeed(x * 2))
            .execute()
            .await;
        assert_eq!(result, Ok(42));
    }

    #[tokio::test]
    async fn map_error_transforms_failure() {
        let result = Effect::<i32, _, ()>::fail("err".to_string())
            .map_error(|e| format!("wrapped: {e}"))
            .execute()
            .await;
        assert_eq!(result, Err("wrapped: err".to_string()));
    }

    #[tokio::test]
    async fn catch_all_recovers_from_errors() {
        let result = Effect::<i32, String, ()>::fail("oops".to_string())
            .catch_all(|_| Effect::<i32, String, ()>::succeed(0))
            .execute()
            .await;
        assert_eq!(result, Ok(0));
    }

    #[tokio::test]
    async fn or_else_uses_fallback() {
        let result = Effect::<_, String, ()>::fail("first".to_string())
            .or_else(Effect::succeed(42))
            .execute()
            .await;
        assert_eq!(result, Ok(42));
    }

    #[tokio::test]
    async fn zip_runs_concurrently() {
        let result = Effect::<_, String, ()>::succeed(1)
            .zip(Effect::succeed(2))
            .execute()
            .await;
        assert_eq!(result, Ok((1, 2)));
    }

    #[tokio::test]
    async fn zip_with_combines_results() {
        let result = Effect::<_, String, ()>::succeed(20)
            .zip_with(Effect::succeed(22), |a, b| a + b)
            .execute()
            .await;
        assert_eq!(result, Ok(42));
    }

    #[tokio::test]
    async fn gen_composes_with_question_mark() {
        let e1 = Effect::<_, String, ()>::succeed(10);
        let e2 = Effect::<_, String, ()>::succeed(32);

        let program: Effect<i32, String, ()> = Effect::from_fn(move |ctx: Arc<()>| {
            let e1 = e1.clone();
            let e2 = e2.clone();
            async move {
                let a = e1.run(ctx.clone()).await?;
                let b = e2.run(ctx).await?;
                Ok(a + b)
            }
        });

        assert_eq!(program.execute().await, Ok(42));
    }

    #[tokio::test]
    async fn gen_short_circuits_on_error() {
        let e1 = Effect::<i32, String, ()>::fail("boom".to_string());
        let e2 = Effect::<i32, String, ()>::succeed(32);

        let program: Effect<i32, String, ()> = Effect::from_fn(move |ctx: Arc<()>| {
            let e1 = e1.clone();
            let e2 = e2.clone();
            async move {
                let a = e1.run(ctx.clone()).await?; // fails here
                let b = e2.run(ctx).await?; // never reached
                Ok(a + b)
            }
        });

        assert_eq!(program.execute().await, Err("boom".to_string()));
    }

    #[tokio::test]
    async fn context_is_threaded_through() {
        struct Config {
            multiplier: i32,
        }

        let effect =
            Effect::<i32, String, Config>::from_fn(|ctx| async move { Ok(21 * ctx.multiplier) });

        assert_eq!(effect.run_with(Config { multiplier: 2 }).await, Ok(42));
    }

    #[tokio::test]
    async fn provide_eliminates_requirement() {
        struct Config {
            value: i32,
        }

        let effect = Effect::<i32, String, Config>::from_fn(|ctx| async move { Ok(ctx.value) })
            .provide(Config { value: 42 });

        assert_eq!(effect.execute().await, Ok(42));
    }

    #[tokio::test]
    async fn ask_accesses_environment() {
        struct Config {
            name: String,
        }

        let effect = Effect::<Arc<Config>, String, Config>::ask().map(|ctx| ctx.name.clone());

        let result = effect
            .run_with(Config {
                name: "hello".into(),
            })
            .await;
        assert_eq!(result, Ok("hello".to_string()));
    }

    #[tokio::test]
    async fn tap_performs_side_effect() {
        use std::sync::Mutex;
        let log = Arc::new(Mutex::new(Vec::new()));
        let log_clone = log.clone();

        let result = Effect::<_, String, ()>::succeed(42)
            .tap(move |x| log_clone.lock().unwrap().push(*x))
            .execute()
            .await;

        assert_eq!(result, Ok(42));
        assert_eq!(*log.lock().unwrap(), vec![42]);
    }

    #[tokio::test]
    async fn chaining_composes_pipeline() {
        let program = Effect::<_, String, ()>::succeed(10)
            .map(|x| x + 5) // 15
            .flat_map(|x| Effect::succeed(x * 2)) // 30
            .map(|x| x + 12); // 42

        assert_eq!(program.execute().await, Ok(42));
    }

    #[tokio::test]
    async fn from_result_lifts_ok() {
        let effect: Effect<i32, String, ()> = Ok(42).into();
        assert_eq!(effect.execute().await, Ok(42));
    }

    #[tokio::test]
    async fn from_result_lifts_err() {
        let effect: Effect<i32, String, ()> = Err("nope".into()).into();
        assert_eq!(effect.execute().await, Err("nope".to_string()));
    }

    #[tokio::test]
    async fn void_discards_value() {
        let result = Effect::<_, String, ()>::succeed(42).void().execute().await;
        assert_eq!(result, Ok(()));
    }
}
