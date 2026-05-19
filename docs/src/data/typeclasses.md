# Typeclasses

Concrete trait families for value-level operations: equality, ordering,
combining, folding. These are *not* an HKT emulation — they're plain
Rust traits — but they fill the same niche Effect-TS's typeclasses do:
**a place to plug in alternative behaviors** when `PartialEq`, `Ord`,
or `Iterator::fold` are too rigid.

| Trait          | Effect-TS analog     | When to reach for it                          |
| -------------- | -------------------- | --------------------------------------------- |
| `Equivalence`  | `Equivalence`        | Multiple notions of "equal" for one type.     |
| `Order`        | `Order`              | Multiple sort orders.                         |
| `Combiner`     | `Semigroup`/`Monoid` | Associative combining; reducing collections.  |
| `Reducer`      | `Reducer`            | Trait-style fold for generic reduction code.  |

All live in `effect::typeclass`.

## `Equivalence` — pluggable equality

```rust,no_run
use effect::typeclass::equivalence::{CaseInsensitive, Equivalence};

let cmp = CaseInsensitive;
assert!(cmp.equivalent("Hello", "hello"));
assert!(!cmp.equivalent("Hello", "world"));
```

`PartialEqEquivalence` delegates to `PartialEq`; `CaseInsensitive`
ignores ASCII case. Define more by implementing the trait:

```rust,no_run
use effect::typeclass::equivalence::Equivalence;

pub struct ApproximatelyEqual { pub epsilon: f64 }
impl Equivalence<f64> for ApproximatelyEqual {
    fn equivalent(&self, a: &f64, b: &f64) -> bool {
        (a - b).abs() < self.epsilon
    }
}
```

## `Order` — pluggable orderings

```rust,no_run
use effect::typeclass::order::{NaturalOrder, Order, OrderBy, Reverse};

let asc = NaturalOrder::<i32>::new();
let desc = Reverse(NaturalOrder::<i32>::new());
let by_length = OrderBy::new(|s: &&str| s.len());

let _ = asc.compare(&1, &2);            // Less
let _ = desc.compare(&1, &2);           // Greater
let _ = by_length.compare(&"a", &"bb"); // Less
```

`Reverse` and `OrderBy` compose with any other order.

## `Combiner` — associative combining (semigroup / monoid)

`combine(a, b)` plus an optional `empty` identity. When `empty` is
`Some`, you've got a monoid; when `None`, only a semigroup.

```rust,no_run
use effect::typeclass::combiner::{Combiner, Sum, Product, Concat, VecConcat};

assert_eq!(Sum.combine(2, 3), 5);
assert_eq!(Sum.empty(), Some(0));

assert_eq!(Product.combine(2, 3), 6);

let s = Concat.combine("foo".to_string(), "bar".to_string());
assert_eq!(s, "foobar");

let v = VecConcat.combine(vec![1, 2], vec![3, 4]);
assert_eq!(v, vec![1, 2, 3, 4]);

// `combine_all` folds an iterable with the combiner.
assert_eq!(Sum.combine_all(vec![1, 2, 3, 4]), Some(10));
```

Stock combiners: `Sum`, `Product` (integer arithmetic), `Concat`
(`String`), `VecConcat` (`Vec<T>`). Build your own by implementing the
trait.

## `Reducer` — trait-style fold

```rust,no_run
use effect::typeclass::reducer::{FnReducer, Reducer};

let r = FnReducer(|acc: i32, x: i32| acc + x);
assert_eq!(r.reduce(0, vec![1, 2, 3, 4]), 10);
```

`FnReducer` wraps a closure; implement `Reducer` directly for stateful
reductions that don't fit a single closure.

## Why concrete traits, not HKT?

Effect-TS's typeclass hierarchy uses HKT to talk about "any container".
Rust doesn't have HKT, and emulating them costs ergonomics and error
quality. `effect-rs` makes the pragmatic choice: provide trait families
keyed on the concrete container type. You don't get "fold any `Foldable`",
but you do get `Iterator::fold`, `Reducer::reduce`, and per-type
methods that compose without any HKT acrobatics.

## What's coming

- More stock combiners: `Min`, `Max`, `All`, `Any`.
- `Reducer`-aware Stream combinators (Phase 3).
- `Equivalence`-aware HashMap / Set in `effect-data`.
