//! Integration tests for `#[derive(Newtype)]` — exercises the proc-macro
//! across crate boundaries, the way real users will.

use effect::Newtype;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Newtype)]
pub struct UserId(u64);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Newtype)]
pub struct Email(String);

#[test]
fn new_constructs_the_wrapper() {
    let id = UserId::new(42);
    assert_eq!(id.into_inner(), 42);
}

#[test]
fn from_inner_constructs_via_into() {
    let id: UserId = 7_u64.into();
    assert_eq!(id, UserId::new(7));
}

#[test]
fn from_self_extracts_inner_via_into() {
    let id = UserId::new(99);
    let raw: u64 = id.into();
    assert_eq!(raw, 99);
}

#[test]
fn as_ref_borrows_inner() {
    let id = UserId::new(11);
    let borrowed: &u64 = id.as_ref();
    assert_eq!(*borrowed, 11);
}

#[test]
fn newtype_composes_with_standard_derives() {
    // Hash + Eq enable use as a HashMap key.
    let mut map: HashMap<UserId, &str> = HashMap::new();
    map.insert(UserId::new(1), "alice");
    map.insert(UserId::new(2), "bob");
    assert_eq!(map.get(&UserId::new(1)), Some(&"alice"));
}

#[test]
fn newtype_works_with_string_inner() {
    let e = Email::new("hi@example.com".to_string());
    assert_eq!(e.as_ref(), "hi@example.com");
    let raw: String = e.clone().into();
    assert_eq!(raw, "hi@example.com");
    let from: Email = "x@y.z".to_string().into();
    assert_eq!(from.into_inner(), "x@y.z");
}

#[test]
fn distinct_newtypes_do_not_unify() {
    // This is the whole point: a UserId is not a u64, so you can't
    // accidentally pass one where the other is expected.
    fn takes_user_id(_: UserId) {}
    takes_user_id(UserId::new(1));
    // takes_user_id(1_u64);  // would not compile
}
