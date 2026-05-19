//! [`Refinement`] — a witness that an inner value satisfies a
//! predicate.
//!
//! Used with [`#[derive(Brand)]`](crate::Brand) to generate a typed
//! `try_new` constructor on a newtype:
//!
//! ```ignore
//! #[derive(Newtype, Brand)]
//! #[brand(refine_with = NonEmptyString)]
//! pub struct Title(String);
//!
//! Title::try_new("hi".into())?;   // Ok
//! Title::try_new("".into());      // Err
//! ```
//!
//! Standard refinements live in this module; anything domain-specific
//! goes alongside the type it constrains.

/// A predicate-style validator: returns `Ok(())` if `value` is
/// acceptable, or `Err(_)` describing why not.
pub trait Refinement<T> {
    type Error;
    fn check(value: &T) -> Result<(), Self::Error>;
}

// ── Stock refinements ─────────────────────────────────────────────

/// "Must not be empty" — for `String` and `Vec<_>`.
pub struct NonEmpty;

impl Refinement<String> for NonEmpty {
    type Error = &'static str;
    fn check(value: &String) -> Result<(), Self::Error> {
        if value.is_empty() {
            Err("must not be empty")
        } else {
            Ok(())
        }
    }
}

impl<T> Refinement<Vec<T>> for NonEmpty {
    type Error = &'static str;
    fn check(value: &Vec<T>) -> Result<(), Self::Error> {
        if value.is_empty() {
            Err("must not be empty")
        } else {
            Ok(())
        }
    }
}

/// "Must be > 0" — for the standard signed integer types.
pub struct Positive;

macro_rules! impl_positive {
    ($($t:ty),*) => {
        $(
            impl Refinement<$t> for Positive {
                type Error = &'static str;
                fn check(value: &$t) -> Result<(), Self::Error> {
                    if *value > 0 { Ok(()) } else { Err("must be positive") }
                }
            }
        )*
    };
}
impl_positive!(i8, i16, i32, i64, i128, isize);

/// "Length ≤ N" for `String` — `N` is the maximum byte length.
pub struct MaxLen<const N: usize>;

impl<const N: usize> Refinement<String> for MaxLen<N> {
    type Error = &'static str;
    fn check(value: &String) -> Result<(), Self::Error> {
        if value.len() <= N {
            Ok(())
        } else {
            Err("exceeds maximum length")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_empty_string_rejects_empty() {
        assert!(NonEmpty::check(&String::new()).is_err());
        assert!(NonEmpty::check(&"hi".to_string()).is_ok());
    }

    #[test]
    fn non_empty_vec_rejects_empty() {
        let v: Vec<i32> = Vec::new();
        assert!(NonEmpty::check(&v).is_err());
        assert!(NonEmpty::check(&vec![1]).is_ok());
    }

    #[test]
    fn positive_rejects_zero_and_negatives() {
        assert!(Positive::check(&0_i32).is_err());
        assert!(Positive::check(&-1_i32).is_err());
        assert!(Positive::check(&1_i32).is_ok());
    }

    #[test]
    fn max_len_enforces_byte_length() {
        type Short = MaxLen<3>;
        assert!(Short::check(&"abc".to_string()).is_ok());
        assert!(Short::check(&"abcd".to_string()).is_err());
    }
}
