mod todo;

use effect::Effect;
use std::sync::Arc;
use todo::*;

#[tokio::main]
async fn main() {
    println!("=== Effect-TS Style Todolist in Rust ===\n");

    // ── 1. Layer + Runtime ──────────────────────────────
    // Build a context from layers, convert to a Runtime.
    // Like Effect-TS:  Layer.merge(RepoLayer, LoggerLayer) |> Runtime

    println!("── Layer → Runtime ─────────────────────");

    let runtime = Layer::new()
        .with_repo(InMemoryTodoRepo::new())
        .with_logger()
        .into_runtime();

    // create_and_log requires R: HasRepo + HasLogger.
    // The runtime's context satisfies both — this compiles!
    let result = runtime.run(&create_and_log("Buy groceries".into())).await;
    println!("  {:?}\n", result);

    // ┌──────────────────────────────────────────────────────────┐
    // │ COMPILE-TIME SAFETY: uncomment to see the error!        │
    // │                                                         │
    // │ // Missing HasLogger — won't compile:                   │
    // │ // let bad = Layer::new()                               │
    // │ //     .with_repo(InMemoryTodoRepo::new())              │
    // │ //     .into_runtime();                                 │
    // │ // bad.run(&create_and_log("nope".into())).await;       │
    // │ //         ^^^^^^^^^^^^^^^^ HasLogger not satisfied      │
    // └──────────────────────────────────────────────────────────┘

    // ── 2. Combinator style ─────────────────────────────
    // Chain effects with .map(), .flat_map(), .tap().
    // Bounds accumulate across the chain.

    println!("── Combinator Style ────────────────────");

    let program = create_and_log("Write Rust code".into())
        .flat_map(|_| create_and_log("Learn Effect patterns".into()))
        .flat_map(|t| complete_todo(t.id))
        .tap(|t| println!("  Completed: {t}"))
        .flat_map(|_| todo_summary())
        .tap(|s| println!("  Summary: {s}"));

    let runtime = Layer::new()
        .with_repo(InMemoryTodoRepo::new())
        .with_logger()
        .into_runtime();

    match runtime.run(&program).await {
        Ok(_) => println!("  ✨ Done\n"),
        Err(e) => eprintln!("  ❌ {e}\n"),
    }

    // ── 3. Do-notation style (from_fn + ?) ──────────────
    // Like Effect.gen(function*() { yield* ... }) in Effect-TS.
    // The `?` operator gives monadic short-circuiting.

    println!("── Do-Notation Style ───────────────────");

    // Note: R is generic — this function works with ANY context
    // that provides HasRepo + HasLogger.
    fn program_gen<R: HasRepo + HasLogger>() -> Effect<(), TodoError, R> {
        Effect::from_fn(|ctx: Arc<R>| async move {
            let t1 = create_and_log("Read a book".into())
                .run(ctx.clone())
                .await?;
            let _t2 = create_and_log("Go for a walk".into())
                .run(ctx.clone())
                .await?;
            let t3 = create_and_log("Cook dinner".into())
                .run(ctx.clone())
                .await?;

            complete_todo(t1.id).run(ctx.clone()).await?;
            complete_todo(t3.id).run(ctx.clone()).await?;

            let todos = list_todos().run(ctx.clone()).await?;
            println!("\n  📋 Todo List:");
            for todo in &todos {
                println!("    {todo}");
            }

            let s = todo_summary().run(ctx).await?;
            println!("\n  📊 {s}");
            Ok(())
        })
    }

    let runtime = Layer::new()
        .with_repo(InMemoryTodoRepo::new())
        .with_logger()
        .into_runtime();

    match runtime.run(&program_gen()).await {
        Ok(_) => println!("  ✨ Done\n"),
        Err(e) => eprintln!("  ❌ {e}\n"),
    }

    // ── 4. Error handling ───────────────────────────────
    // catch_all, or_else, map_error — typed recovery.

    println!("── Error Handling ──────────────────────");

    let runtime = Layer::new()
        .with_repo(InMemoryTodoRepo::new())
        .with_logger()
        .into_runtime();

    // Validation error → catch_all recovers
    // Turbofish on succeed tells the compiler E2 = TodoError
    let validated = create_todo("".into())
        .map(|t: Todo| t.title)
        .catch_all(|e: TodoError| Effect::<_, TodoError, _>::succeed(format!("Recovered: {e}")));
    println!("  Validation:  {:?}", runtime.run(&validated).await);

    // Not found → or_else provides a fallback
    let fallback = get_todo(999).or_else(Effect::succeed(Todo {
        id: 0,
        title: "default todo".into(),
        completed: false,
    }));
    println!("  Fallback:    {:?}", runtime.run(&fallback).await);

    // map_error to wrap domain errors
    let mapped = get_todo(999).map_error(|e| format!("App error: {e}"));
    println!("  map_error:   {:?}", runtime.run(&mapped).await);
    println!();

    // ── 5. Parallel composition (zip) ───────────────────

    println!("── Parallel Composition ────────────────");

    fn parallel_demo<R: HasRepo + HasLogger>() -> Effect<(), TodoError, R> {
        Effect::from_fn(|ctx: Arc<R>| async move {
            create_and_log("Task A".into()).run(ctx.clone()).await?;
            create_and_log("Task B".into()).run(ctx.clone()).await?;

            // zip runs both concurrently via tokio::join!
            let (a, b) = get_todo(1).zip(get_todo(2)).run(ctx.clone()).await?;
            println!("  Concurrent: {a}  &  {b}");

            // zip_with combines results
            let msg = get_todo(1)
                .zip_with(todo_summary(), |todo, summary| {
                    format!("'{}' — overall {}", todo.title, summary)
                })
                .run(ctx)
                .await?;
            println!("  Combined:   {msg}");
            Ok(())
        })
    }

    let runtime = Layer::new()
        .with_repo(InMemoryTodoRepo::new())
        .with_logger()
        .into_runtime();

    match runtime.run(&parallel_demo()).await {
        Ok(_) => println!("  ✨ Done\n"),
        Err(e) => eprintln!("  ❌ {e}\n"),
    }

    // ── 6. Provide (eliminates R) ───────────────────────
    // .provide() bakes in the context, turning R into ().
    // Then .execute() works — no context needed at call site.

    println!("── Provide (eliminates R) ──────────────");

    let needs_deps = create_and_log("Standalone todo".into()).map(|t| format!("  {t}"));

    // provide() erases R → Effect<String, TodoError, ()>
    let layer = Layer::new()
        .with_repo(InMemoryTodoRepo::new())
        .with_logger();
    let standalone = needs_deps.provide(layer.into_ctx());

    // execute() only exists on Effect<_, _, ()>
    match standalone.execute().await {
        Ok(msg) => println!("{msg}"),
        Err(e) => eprintln!("  ❌ {e}"),
    }
    println!("  ✨ Done");
}
