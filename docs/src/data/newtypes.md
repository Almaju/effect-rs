# Newtypes and Brands

Rust has had newtypes since day one — they're just tuple structs:

```rust
pub struct UserId(u64);
```

That alone gives you type-distinct identifiers. What it *doesn't* give
you is the ergonomic plumbing: `From`/`Into` between the wrapper and
the inner type, `AsRef` for borrowing, an explicit constructor.

`#[derive(Newtype)]` writes that plumbing for you.

## The basics

```rust,no_run
use effect::Newtype;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Newtype)]
pub struct UserId(u64);

# fn main() {
let id = UserId::new(42);
let raw: u64 = id.into_inner();
let from: UserId = 7_u64.into();
let borrowed: &u64 = id.as_ref();
# }
```

The derive generates:

| Item                          | Purpose                                       |
| ----------------------------- | --------------------------------------------- |
| `From<Inner> for Self`        | `let id: UserId = 1.into();`                  |
| `From<Self> for Inner`        | `let raw: u64 = id.into();`                   |
| `AsRef<Inner> for Self`       | `id.as_ref()` returns `&u64`                  |
| `Self::new(inner)`            | explicit wrapping                             |
| `Self::into_inner(self)`      | explicit unwrapping (consuming)               |

Standard derives — `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`,
`PartialOrd`, `Ord` — stay user-controlled. Add what you need; skip
what you don't.

## What the derive *doesn't* do (on purpose)

- **No `Deref`.** Auto-deref to the inner type would defeat the point —
  you'd be able to call `u64` methods on a `UserId` and accidentally
  pass it where a `u64` is expected via deref coercion. Borrow
  explicitly with `as_ref()` when you need the inner.
- **No `Display`.** Sometimes you want it (`TodoId` formats as the
  inner integer), sometimes you don't (`Password` should redact). Write
  `impl Display` yourself when appropriate.

## When to reach for it

| Symptom                                          | Reach for                       |
| ------------------------------------------------ | ------------------------------- |
| Two `u64` parameters could be swapped silently   | Newtype each.                   |
| A `String` represents one of N domains           | Newtype per domain.             |
| An ID, version, count is repeatedly stringly-typed | Newtype.                       |
| You're tempted to add a `_user_id:` comment      | Newtype.                        |

If TypeScript users reach for `type UserId = string & { __brand: "UserId" }`
or `Brand<"UserId">`, Rust users reach for `pub struct UserId(u64);`. Same
intent, native syntax.

## Brands — validated newtypes

A *brand* (Effect-TS terminology) is a newtype with a smart constructor
that validates the input. `#[derive(Brand)]` pairs with a `Refinement`
impl that describes the validity rule:

```rust,no_run
use effect::{Brand, Newtype, Refinement};

pub struct EmailRefiner;
impl Refinement<String> for EmailRefiner {
    type Error = &'static str;
    fn check(value: &String) -> Result<(), Self::Error> {
        if !value.contains('@') { return Err("missing '@'"); }
        if !value.contains('.') { return Err("missing '.'"); }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Newtype, Brand)]
#[brand(refine_with = EmailRefiner)]
pub struct Email(String);

# fn main() {
let ok  = Email::try_new("a@b.c".to_string());    // Ok(Email("a@b.c"))
let bad = Email::try_new("nope".to_string());     // Err("missing '@'")
# }
```

The derive generates exactly:

```rust,ignore
impl Email {
    pub fn try_new(value: String)
        -> Result<Self, <EmailRefiner as Refinement<String>>::Error>
    {
        EmailRefiner::check(&value)?;
        Ok(Self(value))
    }
}
```

Once you have an `Email`, no further checking is needed downstream —
the type *is* the proof.

### Stock refinements

Common shapes live in [`effect::refinement`]:

| Refinement                 | Inner            | Behavior                       |
| -------------------------- | ---------------- | ------------------------------ |
| `NonEmpty`                 | `String`, `Vec<T>` | rejects empty                  |
| `Positive`                 | `i8`..`isize`    | requires `> 0`                 |
| `MaxLen<N>` (const generic)| `String`         | requires `len ≤ N` bytes       |

Wire them in directly:

```rust,no_run
use effect::{Brand, Newtype, refinement::NonEmpty};

#[derive(Debug, Clone, PartialEq, Eq, Newtype, Brand)]
#[brand(refine_with = NonEmpty)]
pub struct Title(String);
```

For domain-specific rules, write your own `Refinement` impl alongside
the type it constrains.

[`effect::refinement`]: https://docs.rs/effect/latest/effect/refinement/index.html

## In the wild

The todo example uses a real newtype for IDs:

```rust,ignore
// examples/todo/src/todo/model.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Newtype)]
pub struct TodoId(u64);
```

The repo's `next_id` counter stays a raw `u64`, and `TodoId::new(*next_id)`
wraps it on issue:

```rust,ignore
let id = TodoId::new(*next_id);
*next_id += 1;
```

At call sites, `get_todo(TodoId::new(999))` is the explicit form,
or `999_u64.into()` works if context disambiguates.

## What's coming

- **`#[derive(Brand)]`** for declarative refinements:
  ```rust,ignore
  #[derive(Brand)]
  #[brand(refine_with = "Email::validate")]
  pub struct Email(String);
  ```
- **Attribute opt-ins** on `#[derive(Newtype)]` for `display`,
  `deref`, and `serde` glue, for the cases where you do want them.
- **`Refinement` trait** and a small library of common refinements
  (`NonEmpty<String>`, `Positive<i64>`, `MaxLength<S, const N>`).
