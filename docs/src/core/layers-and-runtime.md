# Layers and the Runtime

The `R` parameter of `Effect<A, E, R>` is more than a Reader monad — it
is how `effect-rs` does **compile-time dependency injection**. A function
declares the services it needs as trait bounds on `R`; the bounds
accumulate as effects compose; nothing runs until you provide a context
that satisfies every accumulated bound.

## Declaring a service

A service is just a trait. The Rust compiler treats `R: Foo + Bar`
as a structural requirement that propagates through composition.

```rust,no_run
pub trait Logger: Send + Sync + 'static {
    fn log(&self, msg: &str);
}

pub trait TodoRepo: Send + Sync + 'static {
    fn repo(&self) -> &InMemoryTodoRepo;
}
# pub struct InMemoryTodoRepo;
```

> A future `tag!` macro will reduce this boilerplate; see the
> [Roadmap](../project/roadmap.md).

## Writing service-generic effects

Make your business logic generic over `R`, bounded by the services it
uses. The compiler will refuse to run anything that doesn't have every
required service.

```rust,no_run
# use effect::Effect; use std::sync::Arc;
# pub trait TodoRepo: Send + Sync + 'static { fn repo(&self) -> &InMemoryTodoRepo; }
# pub trait Logger: Send + Sync + 'static { fn log(&self, msg: &str); }
# pub struct InMemoryTodoRepo;
# impl InMemoryTodoRepo {
#     pub fn create(&self, title: String) -> Result<Todo, TodoError> {
#         Ok(Todo { id: 1, title, completed: false })
#     }
# }
# #[derive(Clone)] pub struct Todo { pub id: u64, pub title: String, pub completed: bool }
# #[derive(Debug)] pub enum TodoError { Invalid(String) }
fn create_todo<R: TodoRepo>(title: String) -> Effect<Todo, TodoError, R> {
    Effect::from_fn(move |ctx: Arc<R>| {
        let title = title.clone();
        async move { ctx.repo().create(title) }
    })
}

fn log_action<R: Logger>(msg: String) -> Effect<(), TodoError, R> {
    Effect::from_fn(move |ctx: Arc<R>| {
        let msg = msg.clone();
        async move { ctx.log(&msg); Ok(()) }
    })
}

// Bounds accumulate automatically:
fn create_and_log<R: TodoRepo + Logger>(title: String) -> Effect<Todo, TodoError, R> {
    create_todo(title).flat_map(|t| {
        let msg = format!("Created: {}", t.title);
        log_action(msg).as_value(t)
    })
}
```

The signature `R: TodoRepo + Logger` is **the contract**. Skip
providing a logger and the program won't compile.

## Building the context — the Layer pattern

A `Layer` is a typed builder that wraps the context one service at a
time. Each `.with_*()` call extends the type so that the resulting
context implements one more trait.

```rust,no_run
# pub trait TodoRepo: Send + Sync + 'static { fn repo(&self) -> &InMemoryTodoRepo; }
# pub trait Logger: Send + Sync + 'static { fn log(&self, msg: &str); }
# pub struct InMemoryTodoRepo;
# impl InMemoryTodoRepo { pub fn new() -> Self { Self } }
use effect::Runtime;

pub struct ConsoleLogger;

pub struct WithRepo<I> { repo: InMemoryTodoRepo, inner: I }
pub struct WithLogger<I> { logger: ConsoleLogger, inner: I }

// Direct impls — each wrapper provides one service.
impl<I: Send + Sync + 'static> TodoRepo for WithRepo<I> {
    fn repo(&self) -> &InMemoryTodoRepo { &self.repo }
}
impl<I: Send + Sync + 'static> Logger for WithLogger<I> {
    fn log(&self, msg: &str) { println!("[LOG] {msg}"); }
}

// Forwarding impls — each wrapper passes through services it doesn't
// itself provide. This is what makes ordering irrelevant.
impl<I: Logger + Send + Sync + 'static> Logger for WithRepo<I> {
    fn log(&self, msg: &str) { self.inner.log(msg) }
}
impl<I: TodoRepo + Send + Sync + 'static> TodoRepo for WithLogger<I> {
    fn repo(&self) -> &InMemoryTodoRepo { self.inner.repo() }
}

pub struct Layer<Ctx> { ctx: Ctx }

impl Layer<()> { pub fn new() -> Self { Layer { ctx: () } } }

impl<Ctx> Layer<Ctx> {
    pub fn with_repo(self, repo: InMemoryTodoRepo) -> Layer<WithRepo<Ctx>> {
        Layer { ctx: WithRepo { repo, inner: self.ctx } }
    }
    pub fn with_logger(self) -> Layer<WithLogger<Ctx>> {
        Layer { ctx: WithLogger { logger: ConsoleLogger, inner: self.ctx } }
    }
    pub fn into_runtime(self) -> Runtime<Ctx>
        where Ctx: Send + Sync + 'static
    { Runtime::new(self.ctx) }
}
```

## Running an effect against a runtime

```rust,ignore
let runtime = Layer::new()
    .with_repo(InMemoryTodoRepo::new())
    .with_logger()
    .into_runtime();

// Compiles only because runtime's context satisfies TodoRepo + Logger.
runtime.run(&create_and_log("Buy milk".into())).await?;
```

Forgetting a layer is a **compile error**, not a runtime panic:

```text
error[E0277]: the trait bound `WithRepo<()>: Logger` is not satisfied
```

That's the whole point — the dependencies are part of the type.

## Testing

Because the runtime is just a value, swap the layer in your test:

```rust,ignore
let runtime = Layer::new()
    .with_repo(InMemoryTodoRepo::new())   // real for the unit under test
    .with_logger_fake()                    // capture logs in a vec
    .into_runtime();

runtime.run(&program()).await?;
assert_eq!(runtime.ctx().logs(), vec!["Created: Buy milk"]);
```

A dedicated `effect-test` crate will land in Phase 1 to formalize this.

## What's coming

Today's `Layer` is a hand-rolled typestate. Phase 1 upgrades it to a
first-class `Layer<R, E>` with:

- `Layer::merge`, `Layer::compose`, `Layer::provide_merge`
- failable construction (`E` channel for builders that can fail)
- `Layer::scoped` for layers that acquire/release resources
- `ManagedRuntime` for long-lived application contexts

The mental model above carries over.
