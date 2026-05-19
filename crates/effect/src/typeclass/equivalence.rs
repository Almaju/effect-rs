//! [`Equivalence`] — a value-level equality.
//!
//! Useful when a type can be compared in *several* meaningful ways
//! and there's no single canonical `PartialEq`. For example,
//! case-sensitive vs case-insensitive string equality, or numeric
//! equality that tolerates rounding error.

use std::marker::PhantomData;

pub trait Equivalence<A: ?Sized> {
    fn equivalent(&self, a: &A, b: &A) -> bool;
}

/// The standard equivalence delegating to `PartialEq`.
pub struct PartialEqEquivalence<A: ?Sized>(PhantomData<fn() -> A>);

impl<A: ?Sized> PartialEqEquivalence<A> {
    pub const fn new() -> Self {
        PartialEqEquivalence(PhantomData)
    }
}

impl<A: ?Sized + PartialEq> Equivalence<A> for PartialEqEquivalence<A> {
    fn equivalent(&self, a: &A, b: &A) -> bool {
        a == b
    }
}

/// Case-insensitive ASCII string equivalence.
pub struct CaseInsensitive;

impl Equivalence<str> for CaseInsensitive {
    fn equivalent(&self, a: &str, b: &str) -> bool {
        a.eq_ignore_ascii_case(b)
    }
}

impl Equivalence<String> for CaseInsensitive {
    fn equivalent(&self, a: &String, b: &String) -> bool {
        a.eq_ignore_ascii_case(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_eq_equivalence_matches_eq() {
        let eq = PartialEqEquivalence::<i32>::new();
        assert!(eq.equivalent(&1, &1));
        assert!(!eq.equivalent(&1, &2));
    }

    #[test]
    fn case_insensitive_ignores_case() {
        let eq = CaseInsensitive;
        assert!(eq.equivalent("Hello", "hello"));
        assert!(eq.equivalent("FOO", "foo"));
        assert!(!eq.equivalent("foo", "bar"));
    }
}
