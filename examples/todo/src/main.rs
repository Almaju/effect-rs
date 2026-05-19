mod todo;

use effect::{Cause, Effect, Exit};
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

    // create_and_log requires R: TodoRepo + Logger.
    // The runtime's context satisfies both — this compiles!
    let exit = runtime.run(&create_and_log("Buy groceries".into())).await;
    println!("  {exit:?}\n");

    // ┌──────────────────────────────────────────────────────────┐
    // │ COMPILE-TIME SAFETY: uncomment to see the error!        │
    // │                                                         │
    // │ // Missing Logger — won't compile:                      │
    // │ // let bad = Layer::new()                               │
    // │ //     .with_repo(InMemoryTodoRepo::new())              │
    // │ //     .into_runtime();                                 │
    // │ // bad.run(&create_and_log("nope".into())).await;       │
    // │ //         ^^^^^^^^^^^^^^^^ Logger not satisfied         │
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

    report(runtime.run(&program).await);

    // ── 3. Do-notation style (from_fn + ?) ──────────────
    // Like Effect.gen(function*() { yield* ... }) in Effect-TS.
    // `Exit::into_typed_result()?` is the per-step plumbing — the
    // upcoming `eff!` macro will hide it.

    println!("── Do-Notation Style ───────────────────");

    fn program_gen<R: TodoRepo + Logger>() -> Effect<(), TodoError, R> {
        Effect::from_fn(|ctx: Arc<R>| async move {
            let t1 = create_and_log("Read a book".into())
                .run(ctx.clone())
                .await
                .into_typed_result()?;
            let _t2 = create_and_log("Go for a walk".into())
                .run(ctx.clone())
                .await
                .into_typed_result()?;
            let t3 = create_and_log("Cook dinner".into())
                .run(ctx.clone())
                .await
                .into_typed_result()?;

            complete_todo(t1.id).run(ctx.clone()).await.into_typed_result()?;
            complete_todo(t3.id).run(ctx.clone()).await.into_typed_result()?;

            let todos = list_todos().run(ctx.clone()).await.into_typed_result()?;
            println!("\n  📋 Todo List:");
            for todo in &todos {
                println!("    {todo}");
            }

            let s = todo_summary().run(ctx).await.into_typed_result()?;
            println!("\n  📊 {s}");
            Ok(())
        })
    }

    let runtime = Layer::new()
        .with_repo(InMemoryTodoRepo::new())
        .with_logger()
        .into_runtime();

    report(runtime.run(&program_gen()).await);

    // ── 4. Error handling ───────────────────────────────
    // catch_all, or_else, map_error — typed recovery.

    println!("── Error Handling ──────────────────────");

    let runtime = Layer::new()
        .with_repo(InMemoryTodoRepo::new())
        .with_logger()
        .into_runtime();

    // Validation error → catch_all recovers
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

    // catch_all_cause recovers from defects (Die) too
    let with_defect = Effect::<i32, TodoError, _>::die_message("hardware fault")
        .catch_all_cause(|c| {
            println!("  caught cause: {c}");
            Effect::<i32, TodoError, _>::succeed(-1)
        });
    println!("  catch_cause: {:?}\n", runtime.run(&with_defect).await);

    // ── 5. Parallel composition (zip) ───────────────────

    println!("── Parallel Composition ────────────────");

    fn parallel_demo<R: TodoRepo + Logger>() -> Effect<(), TodoError, R> {
        Effect::from_fn(|ctx: Arc<R>| async move {
            create_and_log("Task A".into())
                .run(ctx.clone())
                .await
                .into_typed_result()?;
            create_and_log("Task B".into())
                .run(ctx.clone())
                .await
                .into_typed_result()?;

            // zip runs both concurrently via tokio::join!
            let (a, b) = get_todo(1)
                .zip(get_todo(2))
                .run(ctx.clone())
                .await
                .into_typed_result()?;
            println!("  Concurrent: {a}  &  {b}");

            // zip_with combines results
            let msg = get_todo(1)
                .zip_with(todo_summary(), |todo, summary| {
                    format!("'{}' — overall {}", todo.title, summary)
                })
                .run(ctx)
                .await
                .into_typed_result()?;
            println!("  Combined:   {msg}");
            Ok(())
        })
    }

    let runtime = Layer::new()
        .with_repo(InMemoryTodoRepo::new())
        .with_logger()
        .into_runtime();

    report(runtime.run(&parallel_demo()).await);

    // ── 6. Provide (eliminates R) ───────────────────────
    // .provide() bakes in the context, turning R into ().
    // Then .execute() works — no context needed at call site.

    println!("── Provide (eliminates R) ──────────────");

    let needs_deps = create_and_log("Standalone todo".into()).map(|t| format!("  {t}"));

    let layer = Layer::new()
        .with_repo(InMemoryTodoRepo::new())
        .with_logger();
    let standalone = needs_deps.provide(layer.into_ctx());

    match standalone.execute().await {
        Exit::Success(msg) => println!("{msg}"),
        Exit::Failure(c) => eprintln!("  ❌ {c}"),
    }
    println!("  ✨ Done");
}

/// Print a success/failure marker for any `Exit<_, TodoError>`.
fn report<A>(exit: Exit<A, TodoError>) {
    match exit {
        Exit::Success(_) => println!("  ✨ Done\n"),
        Exit::Failure(Cause::Fail(e)) => eprintln!("  ❌ {e}\n"),
        Exit::Failure(c) => eprintln!("  💥 {c}\n"),
    }
}
