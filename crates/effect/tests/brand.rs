//! Integration tests for `#[derive(Brand)]`.

use effect::{Brand, Newtype, Refinement, refinement::NonEmpty};

// Inline refiner with a String error type.
pub struct EmailRefiner;
impl Refinement<String> for EmailRefiner {
    type Error = &'static str;
    fn check(value: &String) -> Result<(), Self::Error> {
        if !value.contains('@') {
            return Err("missing '@'");
        }
        if !value.contains('.') {
            return Err("missing '.'");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Newtype, Brand)]
#[brand(refine_with = EmailRefiner)]
pub struct Email(String);

#[derive(Debug, Clone, PartialEq, Eq, Newtype, Brand)]
#[brand(refine_with = NonEmpty)]
pub struct Title(String);

#[test]
fn try_new_accepts_valid_email() {
    let e = Email::try_new("a@b.c".to_string()).unwrap();
    assert_eq!(e.into_inner(), "a@b.c");
}

#[test]
fn try_new_rejects_missing_at() {
    let err = Email::try_new("nope".to_string()).unwrap_err();
    assert!(err.contains("missing '@'"));
}

#[test]
fn try_new_rejects_missing_dot() {
    let err = Email::try_new("nope@nodomain".to_string()).unwrap_err();
    assert!(err.contains("missing '.'"));
}

#[test]
fn try_new_accepts_non_empty_title() {
    let t = Title::try_new("Hello".to_string()).unwrap();
    assert_eq!(t.as_ref(), "Hello");
}

#[test]
fn try_new_rejects_empty_title() {
    assert!(Title::try_new(String::new()).is_err());
}

#[test]
fn brand_still_supports_newtype_apis() {
    // Brand and Newtype compose: try_new for validation, From/Into
    // and as_ref for the standard plumbing.
    let t = Title::try_new("ok".to_string()).unwrap();
    let raw: String = t.clone().into();
    assert_eq!(raw, "ok");
    assert_eq!(t.as_ref(), "ok");
}
