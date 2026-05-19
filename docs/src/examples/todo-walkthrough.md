# Todo Walkthrough

The `examples/todo` crate is a small but complete demonstration of
effect-rs as it stands today: typed errors, trait-based service
injection, layer composition, do-notation style, error recovery, and
concurrent reads. It runs end-to-end with:

```bash
cargo run -p effect-example-todo
```

Source layout:

```
examples/todo/
├── Cargo.toml
└── src/
    ├── main.rs         — the demo program
    └── todo/
        ├── mod.rs
        ├── model.rs    — Todo, TodoId
        ├── error.rs    — TodoError
        ├── repo.rs     — InMemoryTodoRepo
        ├── traits.rs   — HasRepo, HasLogger (the service tags)
        ├── service.rs  — business logic, generic over R
        └── layer.rs    — typed context builder
```

## 1. Domain types

A plain `Todo` struct and a typed error — nothing effect-specific yet.

```rust,ignore
// model.rs
pub type TodoId = u64;
pub struct Todo { pub id: TodoId, pub title: String, pub completed: bool }

// error.rs
pub enum TodoError {
    NotFound(TodoId),
    InvalidTitle(String),
}
```

## 2. Service tags

A service is a trait. Bounding `R` by it declares the requirement.

```rust,ignore
// traits.rs
pub trait HasRepo: Send + Sync + 'static {
    fn repo(&self) -> &InMemoryTodoRepo;
}

pub trait HasLogger: Send + Sync + 'static {
    fn log(&self, msg: &str);
}
```

## 3. Business logic — generic over `R`

The service layer is the heart of the example. Each function declares
only the services it actually uses. Composition accumulates bounds:

```rust,ignore
// service.rs
pub fn create_todo<R: HasRepo>(title: String) -> Effect<Todo, TodoError, R> { … }
pub fn log_action<R: HasLogger>(msg: String) -> Effect<(), TodoError, R> { … }

// Combined — needs *both* services. The compiler enforces it.
pub fn create_and_log<R: HasRepo + HasLogger>(
    title: String,
) -> Effect<Todo, TodoError, R> {
    create_todo(title).flat_map(|t| {
        let msg = format!("Created: {}", t.title);
        log_action(msg).as_value(t)
    })
}
```

## 4. Wiring with a Layer

`Layer` accumulates services in a typestate; `into_runtime()` only
compiles once every required trait is satisfied.

```rust,ignore
let runtime = Layer::new()
    .with_repo(InMemoryTodoRepo::new())
    .with_logger()
    .into_runtime();

runtime.run(&create_and_log("Buy groceries".into())).await?;
```

Comment out `.with_logger()` and the compiler refuses:

```text
error[E0277]: the trait bound `WithRepo<()>: HasLogger` is not satisfied
```

That's the whole game — dependencies in the type.

## 5. Styles of composition

The example showcases three equivalent styles for chaining work:

**Combinator style** — methods on `Effect`:

```rust,ignore
create_and_log("Write Rust code".into())
    .flat_map(|_| create_and_log("Learn Effect patterns".into()))
    .flat_map(|t| complete_todo(t.id))
    .tap(|t| println!("  Completed: {t}"))
    .flat_map(|_| todo_summary())
```

**Do-notation style** — `async` block + `?`:

```rust,ignore
fn program_gen<R: HasRepo + HasLogger>() -> Effect<(), TodoError, R> {
    Effect::from_fn(|ctx: Arc<R>| async move {
        let t1 = create_and_log("Read a book".into()).run(ctx.clone()).await?;
        let _  = create_and_log("Go for a walk".into()).run(ctx.clone()).await?;
        let t3 = create_and_log("Cook dinner".into()).run(ctx.clone()).await?;

        complete_todo(t1.id).run(ctx.clone()).await?;
        complete_todo(t3.id).run(ctx.clone()).await?;

        let todos = list_todos().run(ctx.clone()).await?;
        for todo in &todos { println!("  {todo}"); }
        Ok(())
    })
}
```

The Phase 1 `eff!` macro will let you drop the `.run(ctx.clone()).await?`
boilerplate.

## 6. Error handling

```rust,ignore
// catch_all — recover, possibly changing the error type
let validated = create_todo("".into())
    .map(|t: Todo| t.title)
    .catch_all(|e: TodoError| Effect::<_, TodoError, _>::succeed(format!("Recovered: {e}")));

// or_else — supply a fallback effect
let fallback = get_todo(999).or_else(Effect::succeed(default_todo()));

// map_error — translate domain errors
let mapped = get_todo(999).map_error(|e| format!("App error: {e}"));
```

## 7. Concurrency

```rust,ignore
let (a, b) = get_todo(1).zip(get_todo(2)).run(ctx).await?;
let msg    = get_todo(1)
    .zip_with(todo_summary(), |todo, summary| {
        format!("'{}' — overall {}", todo.title, summary)
    })
    .run(ctx).await?;
```

## 8. Eliminating `R` with `.provide()`

```rust,ignore
let layer = Layer::new()
    .with_repo(InMemoryTodoRepo::new())
    .with_logger();

let needs_deps  = create_and_log("Standalone todo".into());
let standalone  = needs_deps.provide(layer.into_ctx());   // Effect<_, _, ()>

standalone.execute().await?;
```

`.provide()` is how you produce a `() `-environment effect from one that
needed services — useful when handing an effect to code that has no
opinions about your context.

## Where this evolves

Once Phase 1 lands the example will be updated to use:

- `Cause`/`Exit` (with `.sandbox()` showing the inspectable form),
- a `Scope` for the repo (so a real DB connection could replace the
  in-memory store with no service-level changes),
- a small `effect-test` harness swapping `InMemoryTodoRepo` for a fake.
