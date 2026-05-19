# Working with Collections

`effect-rs` ships three persistent, immutable collections in the
[`effect::data`] module (re-exported from the standalone `effect-data`
crate):

| Type                 | Backed by         | What it's for                       |
| -------------------- | ----------------- | ----------------------------------- |
| `Chunk<A>`           | `im::Vector`      | Immutable sequence with fast push/pop/index. |
| `HashMap<K, V>`      | `im::HashMap`     | Persistent HAMT key→value map.      |
| `HashSet<A>`         | `im::HashSet`     | Persistent HAMT set.                |

All three are *persistent* in the functional-programming sense:
operations return a new structure that shares as much of the previous
one as possible. Cloning is cheap (`Arc` under the hood) and they're
safe to pass to spawned fibers, store in a `Ref`, or hand to an `Effect`.

## `Chunk` — immutable sequence

```rust,no_run
use effect::data::Chunk;

# fn main() {
let c = Chunk::empty()
    .push_back(1)
    .push_back(2)
    .push_back(3);

assert_eq!(c.len(), 3);
assert_eq!(c.get(0), Some(&1));
assert_eq!(c.first(), Some(&1));
assert_eq!(c.last(), Some(&3));

let doubled = c.clone().map(|x| x * 2);
assert_eq!(doubled.into_vec(), vec![2, 4, 6]);

let evens = c.filter(|x| x % 2 == 0);
assert_eq!(evens.into_vec(), vec![2]);
# }
```

| Method                | Notes                                       |
| --------------------- | ------------------------------------------- |
| `empty()`             | constructor                                 |
| `singleton(value)`    | one element                                 |
| `from_iter(iter)`     | `let c: Chunk<i32> = (0..10).collect();`    |
| `len()`, `is_empty()` |                                             |
| `get(idx)`            | borrowing index                             |
| `first()`, `last()`   | borrowing endpoints                         |
| `push_back(v)` / `push_front(v)` | returns a new chunk              |
| `concat(other)`       | O(log min(m, n))                            |
| `map(f)` / `filter(f)` | new chunk                                  |
| `fold(init, f)`       | left fold                                   |
| `iter()`              | iterator over `&A`                          |
| `into_iter()`         | consuming iterator                          |
| `into_vec()`          | drop persistence; collect into a `Vec`      |

## `HashMap<K, V>`

```rust,no_run
use effect::data::HashMap;

# fn main() {
let m = HashMap::empty()
    .insert("alice", 1)
    .insert("bob",   2);

assert_eq!(m.get(&"alice"), Some(&1));
assert!(m.contains_key(&"bob"));

let without_alice = m.remove(&"alice");
assert!(!without_alice.contains_key(&"alice"));

let doubled = without_alice.map_values(|v| v * 10);
assert_eq!(doubled.get(&"bob"), Some(&20));
# }
```

| Method                | Notes                                       |
| --------------------- | ------------------------------------------- |
| `empty()`             |                                             |
| `from_iter(iter)`     | takes `(K, V)` items                        |
| `len()`, `is_empty()` |                                             |
| `get(key)`            | `Option<&V>`                                |
| `contains_key(key)`   |                                             |
| `insert(k, v)`        | replaces any existing                       |
| `remove(key)`         |                                             |
| `union(other)`        | **right-biased** — `other` overwrites       |
| `map_values(f)`       | transform every value                       |
| `iter`, `keys`, `values` |                                          |
| `into_iter()`         |                                             |

> The wrapper deliberately shadows `std::collections::HashMap` when
> imported. Use `use std::collections::HashMap as StdHashMap;` if you
> need both in scope.

## `HashSet<A>`

```rust,no_run
use effect::data::HashSet;

# fn main() {
let a: HashSet<i32> = [1, 2, 3].into_iter().collect();
let b: HashSet<i32> = [2, 3, 4].into_iter().collect();

let u = a.clone().union(b.clone());           // {1, 2, 3, 4}
let i = a.clone().intersection(b.clone());    // {2, 3}
let d = a.difference(b);                       // {1}
# }
```

| Method                | Notes                                       |
| --------------------- | ------------------------------------------- |
| `empty()`             |                                             |
| `from_iter(iter)`     |                                             |
| `len()`, `is_empty()` |                                             |
| `contains(value)`     |                                             |
| `insert(v)`, `remove(v)` |                                          |
| `union`, `intersection`, `difference` | each returns a new set       |
| `iter()`, `into_iter()` |                                           |

## When to reach for them

- **Sharing state across tasks** — persistent collections clone cheaply,
  so handing one to a spawned fiber doesn't copy data.
- **Snapshots** — store the "current" map in a `Ref`; each reader gets
  a stable view that won't change underfoot.
- **Build-then-publish** — accumulate into a `HashMap`, then publish
  via `Ref::set` so observers see an atomic flip.

## When *not* to reach for them

- **Tight loops with bulk mutation** — std's `Vec` / `HashMap` are
  faster when you don't need persistence.
- **APIs that take std collections** — `Chunk::into_vec()`,
  `HashMap::into_iter().collect::<StdHashMap<_, _>>()` are your
  escape hatches.

## What's coming

- `OrdMap` and `OrdSet` (B-tree backed; ordered key iteration).
- Mutable variants: `MutableHashMap`, `MutableHashSet`.
- Cursor-style APIs on `Chunk` for indexed traversal.
- `Equivalence`-aware variants once typeclass integration matures.
