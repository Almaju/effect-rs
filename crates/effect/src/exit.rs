//! Structured failure types for effects: [`Defect`], [`Cause`], and [`Exit`].
//!
//! `effect-rs` distinguishes three kinds of failure:
//!
//! | Kind          | Meaning                                                            |
//! | ------------- | ------------------------------------------------------------------ |
//! | `Cause::Fail` | An *expected* error of type `E`. The kind you handle in business logic. |
//! | `Cause::Die`  | A *defect* — a panic, broken invariant, or other unexpected fault. |
//! | `Cause::Interrupt` | A cooperative cancellation from another fiber. (Placeholder until fibers land.) |
//!
//! Running an [`Effect`](crate::Effect) produces an [`Exit<A, E>`] — either a
//! success value or a [`Cause<E>`]. `.catch_all` and `.or_else` only recover
//! from `Fail`. To inspect or recover from defects and interruption use
//! `.catch_all_cause` or `.sandbox`.

use std::any::Any;
use std::fmt;

/// An unexpected failure — a panic, broken invariant, or other bug. Not
/// part of the typed error channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Defect {
    pub message: String,
}

impl Defect {
    /// Construct a defect from a human-readable message.
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }

    /// Construct a defect from a panic payload as produced by
    /// `std::panic::catch_unwind`.
    pub fn from_panic(payload: Box<dyn Any + Send>) -> Self {
        let message = if let Some(s) = payload.downcast_ref::<&'static str>() {
            (*s).to_owned()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "(panic payload not a string)".to_owned()
        };
        Self { message }
    }
}

impl fmt::Display for Defect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// The structured failure of an effect.
///
/// `Sequential` and `Parallel` represent multiple causes arising during the
/// same evaluation. They're rare today (no fibers, no `race`) but present
/// in the type so future combinators don't break the API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cause<E> {
    /// An expected, typed failure.
    Fail(E),
    /// A defect — a panic or other unexpected failure.
    Die(Defect),
    /// Cooperative cancellation.
    Interrupt,
    /// Two causes that occurred one after the other (e.g. finalizer error
    /// following a primary failure).
    Sequential(Box<Cause<E>>, Box<Cause<E>>),
    /// Two causes that occurred concurrently (e.g. both arms of `zip`
    /// failing).
    Parallel(Box<Cause<E>>, Box<Cause<E>>),
}

impl<E> Cause<E> {
    pub fn fail(error: E) -> Self { Cause::Fail(error) }
    pub fn die(defect: Defect) -> Self { Cause::Die(defect) }
    pub fn die_message(message: impl Into<String>) -> Self {
        Cause::Die(Defect::new(message))
    }
    pub fn interrupt() -> Self { Cause::Interrupt }

    pub fn is_fail(&self) -> bool { matches!(self, Cause::Fail(_)) }
    pub fn is_die(&self) -> bool { matches!(self, Cause::Die(_)) }
    pub fn is_interrupt(&self) -> bool { matches!(self, Cause::Interrupt) }

    /// The first typed failure in this cause, if any.
    pub fn failure(&self) -> Option<&E> {
        match self {
            Cause::Fail(e) => Some(e),
            Cause::Sequential(a, b) | Cause::Parallel(a, b) => {
                a.failure().or_else(|| b.failure())
            }
            _ => None,
        }
    }

    /// Take the first typed failure out of this cause, if any.
    pub fn into_failure(self) -> Option<E> {
        match self {
            Cause::Fail(e) => Some(e),
            Cause::Sequential(a, _) | Cause::Parallel(a, _) => a.into_failure(),
            _ => None,
        }
    }

    /// True if every leaf cause is `Interrupt` (no failures, no defects).
    pub fn is_interrupted_only(&self) -> bool {
        match self {
            Cause::Interrupt => true,
            Cause::Fail(_) | Cause::Die(_) => false,
            Cause::Sequential(a, b) | Cause::Parallel(a, b) => {
                a.is_interrupted_only() && b.is_interrupted_only()
            }
        }
    }

    /// Combine two causes in order (sequential composition).
    pub fn then(self, other: Cause<E>) -> Cause<E> {
        Cause::Sequential(Box::new(self), Box::new(other))
    }

    /// Combine two causes concurrently (parallel composition).
    pub fn both(self, other: Cause<E>) -> Cause<E> {
        Cause::Parallel(Box::new(self), Box::new(other))
    }

    /// Map every typed failure inside this cause.
    pub fn map<E2, F: FnMut(E) -> E2>(self, mut f: F) -> Cause<E2> {
        self.map_impl(&mut f)
    }

    /// Convert this cause to any `Cause<E2>`. Intended for use after the
    /// `Cause::Fail` case has already been handled — panics if it
    /// encounters a `Fail`.
    ///
    /// **Known limitation:** compound causes (`Sequential` / `Parallel`)
    /// produced by parallel combinators may contain `Fail` leaves; in that
    /// case this method panics. Use [`Cause::map`] or
    /// [`crate::Effect::catch_all_cause`] instead.
    pub fn change_failure_type<E2>(self) -> Cause<E2> {
        self.map(|_| panic!("Cause::change_failure_type called on a Cause containing Fail"))
    }

    fn map_impl<E2, F: FnMut(E) -> E2>(self, f: &mut F) -> Cause<E2> {
        match self {
            Cause::Fail(e) => Cause::Fail(f(e)),
            Cause::Die(d) => Cause::Die(d),
            Cause::Interrupt => Cause::Interrupt,
            Cause::Sequential(a, b) => Cause::Sequential(
                Box::new(a.map_impl(f)),
                Box::new(b.map_impl(f)),
            ),
            Cause::Parallel(a, b) => Cause::Parallel(
                Box::new(a.map_impl(f)),
                Box::new(b.map_impl(f)),
            ),
        }
    }
}

impl<E: fmt::Display> fmt::Display for Cause<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Cause::Fail(e) => write!(f, "Fail: {e}"),
            Cause::Die(d) => write!(f, "Die: {d}"),
            Cause::Interrupt => f.write_str("Interrupt"),
            Cause::Sequential(a, b) => write!(f, "{a}; then {b}"),
            Cause::Parallel(a, b) => write!(f, "{a} & {b}"),
        }
    }
}

/// The outcome of running an effect: a value or a structured failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exit<A, E> {
    Success(A),
    Failure(Cause<E>),
}

impl<A, E> Exit<A, E> {
    pub fn succeed(value: A) -> Self { Exit::Success(value) }
    pub fn fail(error: E) -> Self { Exit::Failure(Cause::Fail(error)) }
    pub fn die(defect: Defect) -> Self { Exit::Failure(Cause::Die(defect)) }
    pub fn interrupt() -> Self { Exit::Failure(Cause::Interrupt) }
    pub fn from_cause(cause: Cause<E>) -> Self { Exit::Failure(cause) }

    pub fn is_success(&self) -> bool { matches!(self, Exit::Success(_)) }
    pub fn is_failure(&self) -> bool { matches!(self, Exit::Failure(_)) }

    /// The success value, if any.
    pub fn ok(self) -> Option<A> {
        match self {
            Exit::Success(a) => Some(a),
            Exit::Failure(_) => None,
        }
    }

    /// The first typed failure, if any. Defects and interruption return
    /// `None` — use [`Exit::into_result`] or [`Exit::cause`] if you need to
    /// see them.
    pub fn err(self) -> Option<E> {
        match self {
            Exit::Failure(c) => c.into_failure(),
            Exit::Success(_) => None,
        }
    }

    /// Borrow the full failure cause, if any.
    pub fn cause(&self) -> Option<&Cause<E>> {
        match self {
            Exit::Failure(c) => Some(c),
            Exit::Success(_) => None,
        }
    }

    /// Convert into a `Result` whose error is the full structured cause.
    pub fn into_result(self) -> Result<A, Cause<E>> {
        match self {
            Exit::Success(a) => Ok(a),
            Exit::Failure(c) => Err(c),
        }
    }

    /// Convert into `Result<A, E>`, **panicking** on defects (`Die`) or
    /// interruption. Use when you've established no defect can occur at
    /// this point — typical when threading through an `async` block with
    /// `?`. Prefer [`Exit::into_result`] when you need to handle defects.
    pub fn into_typed_result(self) -> Result<A, E>
    where
        E: std::fmt::Debug,
    {
        match self {
            Exit::Success(a) => Ok(a),
            Exit::Failure(Cause::Fail(e)) => Err(e),
            Exit::Failure(c) => panic!("expected a typed failure, got {c:?}"),
        }
    }

    pub fn map<B>(self, f: impl FnOnce(A) -> B) -> Exit<B, E> {
        match self {
            Exit::Success(a) => Exit::Success(f(a)),
            Exit::Failure(c) => Exit::Failure(c),
        }
    }

    pub fn map_error<E2>(self, f: impl FnMut(E) -> E2) -> Exit<A, E2> {
        match self {
            Exit::Success(a) => Exit::Success(a),
            Exit::Failure(c) => Exit::Failure(c.map(f)),
        }
    }
}
