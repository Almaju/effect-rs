//! Test helpers for the [`effect`] crate.
//!
//! The crate provides three kinds of utilities:
//!
//! - **Assertion combinators** — `expect_success`, `expect_failure`,
//!   `expect_die`, `expect_interrupted`. Each takes an `Exit` and
//!   panics with a useful message on mismatch, in the style of `assert_eq!`.
//! - **`ExitExt`** — extension methods on `Exit` for ergonomic
//!   in-test matching (`exit.assert_success(42)`).
//! - **`capture_logs`** — runs an effect with a tracing subscriber
//!   that records every event into a `Vec` you can assert on.

use std::fmt::Debug;
use std::sync::{Arc, Mutex};

use effect::{Cause, Effect, Exit};
use tracing::Level;
use tracing::field::{Field, Visit};
use tracing::subscriber::with_default;
use tracing::{Event, Subscriber};

// ── Assertion combinators ────────────────────────────────────────

/// Panics unless `exit` is `Exit::Success` carrying `expected`.
pub fn expect_success<A, E>(exit: &Exit<A, E>, expected: &A)
where
    A: Debug + PartialEq,
    E: Debug,
{
    match exit {
        Exit::Success(a) if a == expected => {}
        other => panic!("expected Success({expected:?}), got {other:?}"),
    }
}

/// Panics unless `exit` is `Exit::Failure(Cause::Fail(e))` where
/// `pred(e)` returns true.
pub fn expect_failure<A, E, F>(exit: &Exit<A, E>, mut pred: F)
where
    A: Debug,
    E: Debug,
    F: FnMut(&E) -> bool,
{
    match exit {
        Exit::Failure(Cause::Fail(e)) if pred(e) => {}
        other => panic!("expected typed Fail satisfying predicate, got {other:?}"),
    }
}

/// Panics unless `exit` is `Exit::Failure(Cause::Fail(e))` where `e == expected`.
pub fn expect_failure_eq<A, E>(exit: &Exit<A, E>, expected: &E)
where
    A: Debug,
    E: Debug + PartialEq,
{
    match exit {
        Exit::Failure(Cause::Fail(e)) if e == expected => {}
        other => panic!("expected Failure(Fail({expected:?})), got {other:?}"),
    }
}

/// Panics unless `exit` is `Exit::Failure(Cause::Die(_))`.
pub fn expect_die<A: Debug, E: Debug>(exit: &Exit<A, E>) {
    match exit {
        Exit::Failure(Cause::Die(_)) => {}
        other => panic!("expected Die, got {other:?}"),
    }
}

/// Panics unless `exit` is `Exit::Failure(Cause::Interrupt)` (or a
/// compound made up entirely of Interrupt leaves).
pub fn expect_interrupted<A: Debug, E: Debug>(exit: &Exit<A, E>) {
    match exit {
        Exit::Failure(c) if c.is_interrupted_only() => {}
        other => panic!("expected pure Interrupt, got {other:?}"),
    }
}

// ── ExitExt — fluent matchers ─────────────────────────────────

pub trait ExitExt<A, E> {
    fn assert_success(self, expected: A)
    where
        A: Debug + PartialEq,
        E: Debug;

    fn assert_failure(self, expected: E)
    where
        A: Debug,
        E: Debug + PartialEq;

    fn assert_die(self)
    where
        A: Debug,
        E: Debug;

    fn assert_interrupted(self)
    where
        A: Debug,
        E: Debug;
}

impl<A, E> ExitExt<A, E> for Exit<A, E> {
    fn assert_success(self, expected: A)
    where
        A: Debug + PartialEq,
        E: Debug,
    {
        expect_success(&self, &expected);
    }
    fn assert_failure(self, expected: E)
    where
        A: Debug,
        E: Debug + PartialEq,
    {
        expect_failure_eq(&self, &expected);
    }
    fn assert_die(self)
    where
        A: Debug,
        E: Debug,
    {
        expect_die(&self);
    }
    fn assert_interrupted(self)
    where
        A: Debug,
        E: Debug,
    {
        expect_interrupted(&self);
    }
}

// ── capture_logs ────────────────────────────────────────────────

/// Run `f` with a tracing subscriber that captures every event.
/// Returns the recorded `(Level, message)` pairs.
///
/// The closure must be **synchronous** — tracing subscribers don't
/// propagate across `await` points, so wrap an async block in
/// `runtime.block_on(...)` if you need to drive an effect.
pub fn capture_logs<F: FnOnce()>(f: F) -> Vec<(Level, String)> {
    let layer = CaptureLayer::default();
    let events = layer.events.clone();
    with_default(layer, f);
    let out = events.lock().unwrap().clone();
    out
}

#[derive(Default, Clone)]
struct CaptureLayer {
    events: Arc<Mutex<Vec<(Level, String)>>>,
}

impl Subscriber for CaptureLayer {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::Id {
        tracing::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::Id, _: &tracing::Id) {}
    fn event(&self, event: &Event<'_>) {
        let level = *event.metadata().level();
        let mut grabber = MessageGrabber(String::new());
        event.record(&mut grabber);
        self.events.lock().unwrap().push((level, grabber.0));
    }
    fn enter(&self, _: &tracing::Id) {}
    fn exit(&self, _: &tracing::Id) {}
}

struct MessageGrabber(String);
impl Visit for MessageGrabber {
    fn record_str(&mut self, _: &Field, value: &str) {
        self.0.push_str(value);
    }
    fn record_debug(&mut self, _: &Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        let _ = write!(&mut self.0, "{value:?}");
    }
}

// ── run_with_logs convenience ───────────────────────────────────

/// Run `eff` to completion on a fresh tokio runtime while capturing
/// every tracing event. Returns the `(Exit, Vec<(Level, msg)>)` pair.
pub fn run_with_logs<A, E, R>(eff: Effect<A, E, R>, ctx: R) -> (Exit<A, E>, Vec<(Level, String)>)
where
    A: Send + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let mut captured: Option<Exit<A, E>> = None;
    let events = capture_logs(|| {
        captured = Some(rt.block_on(eff.run_with(ctx)));
    });
    (captured.expect("effect did not complete"), events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use effect::Effect;

    #[test]
    fn expect_success_passes_on_match() {
        let exit: Exit<i32, String> = Exit::Success(42);
        expect_success(&exit, &42);
    }

    #[test]
    #[should_panic(expected = "expected Success")]
    fn expect_success_panics_on_failure() {
        let exit: Exit<i32, String> = Exit::Failure(Cause::Fail("boom".into()));
        expect_success(&exit, &42);
    }

    #[test]
    fn expect_failure_eq_passes_on_match() {
        let exit: Exit<i32, String> = Exit::Failure(Cause::Fail("boom".into()));
        expect_failure_eq(&exit, &"boom".to_string());
    }

    #[test]
    #[should_panic(expected = "expected Failure")]
    fn expect_failure_eq_panics_on_wrong_value() {
        let exit: Exit<i32, String> = Exit::Failure(Cause::Fail("boom".into()));
        expect_failure_eq(&exit, &"other".to_string());
    }

    #[test]
    fn expect_die_passes_on_die() {
        let exit: Exit<i32, String> = Exit::Failure(Cause::Die(effect::Defect::new("ouch")));
        expect_die(&exit);
    }

    #[test]
    fn expect_interrupted_passes_on_interrupt() {
        let exit: Exit<i32, String> = Exit::Failure(Cause::Interrupt);
        expect_interrupted(&exit);
    }

    #[test]
    #[should_panic(expected = "expected pure Interrupt")]
    fn expect_interrupted_panics_on_die() {
        let exit: Exit<i32, String> = Exit::Failure(Cause::Die(effect::Defect::new("x")));
        expect_interrupted(&exit);
    }

    #[tokio::test]
    async fn exit_ext_assert_success_chain() {
        let exit = Effect::<_, String, ()>::succeed(7).execute().await;
        exit.assert_success(7);
    }

    #[tokio::test]
    async fn exit_ext_assert_failure_chain() {
        let exit = Effect::<i32, _, ()>::fail("nope".to_string()).execute().await;
        exit.assert_failure("nope".to_string());
    }

    #[test]
    fn capture_logs_records_events() {
        let events = capture_logs(|| {
            tracing::info!("alpha");
            tracing::warn!("beta");
        });
        assert!(events.iter().any(|(l, m)| *l == Level::INFO && m.contains("alpha")));
        assert!(events.iter().any(|(l, m)| *l == Level::WARN && m.contains("beta")));
    }

    #[test]
    fn run_with_logs_returns_exit_and_events() {
        let eff = Effect::<i32, String, ()>::sync(|| {
            tracing::info!("computing");
            Ok(42)
        });
        let (exit, events) = run_with_logs(eff, ());
        assert_eq!(exit.ok(), Some(42));
        assert!(events.iter().any(|(_, m)| m.contains("computing")));
    }
}
