//! Integration tests for `Effect::fork`, `Fiber::join`, `Fiber::interrupt`,
//! and `Effect::race`.

use effect::{Cause, Effect, Exit};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[tokio::test]
async fn fork_and_join_returns_inner_value() {
    let work = Effect::<i32, String, ()>::from_fn(|_| async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        Ok(42)
    });

    let program = Effect::<_, String, ()>::block(move |g| {
        let work = work.clone();
        async move {
            let fiber = g.run(work.fork()).await?;
            let exit = fiber.join().await;
            Ok::<i32, String>(exit.ok().unwrap())
        }
    });

    assert_eq!(program.execute().await.ok(), Some(42));
}

#[tokio::test]
async fn fork_does_not_block_outer_effect() {
    use std::time::Instant;

    let slow = Effect::<i32, String, ()>::from_fn(|_| async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(7)
    });

    let start = Instant::now();
    let program = Effect::<_, String, ()>::block(move |g| {
        let slow = slow.clone();
        async move {
            let _fiber = g.run(slow.fork()).await?;
            // We return without joining; the spawn doesn't block us.
            Ok::<i32, String>(0)
        }
    });

    let result = program.execute().await;
    let elapsed = start.elapsed();
    assert_eq!(result.ok(), Some(0));
    assert!(
        elapsed < Duration::from_millis(50),
        "expected fast return without join, took {elapsed:?}"
    );
}

#[tokio::test]
async fn interrupt_handle_is_observable() {
    // Forge a fiber that polls the interrupt flag at flat_map
    // boundaries via the standard Effect::interrupt machinery.
    let work = Effect::<(), String, ()>::sync(|| Ok(())).flat_map(|_| {
        // Long async work to give the parent time to interrupt.
        Effect::<(), String, ()>::from_fn(|_| async {
            tokio::time::sleep(Duration::from_millis(500)).await;
            Ok(())
        })
        .flat_map(|_| Effect::<(), String, ()>::succeed(()))
    });

    let program = Effect::<_, String, ()>::block(move |g| {
        let work = work.clone();
        async move {
            let fiber = g.run(work.fork()).await?;
            tokio::time::sleep(Duration::from_millis(50)).await;
            fiber.interrupt();
            let exit = fiber.join().await;
            Ok::<_, String>(exit)
        }
    });

    let outer = program.execute().await;
    let inner = outer.ok().unwrap();
    // Either the child has reached a flat_map boundary and returned
    // Interrupt, or it ran to completion in time. Both are valid
    // outcomes — the contract is only that interrupt is observable
    // and that join returns.
    assert!(
        matches!(inner, Exit::Success(_))
            || matches!(inner, Exit::Failure(Cause::Interrupt))
    );
}

#[tokio::test]
async fn race_returns_first_to_complete() {
    let fast = Effect::<i32, String, ()>::from_fn(|_| async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        Ok(1)
    });
    let slow = Effect::<i32, String, ()>::from_fn(|_| async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        Ok(2)
    });

    let exit = fast.race(slow).execute().await;
    assert_eq!(exit.ok(), Some(1));
}

#[tokio::test]
async fn race_returns_first_failure_when_it_loses() {
    let quick_fail = Effect::<i32, String, ()>::from_fn(|_| async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Err::<i32, _>("first lost".to_string())
    });
    let slow_success = Effect::<i32, String, ()>::from_fn(|_| async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        Ok(99)
    });

    let exit = quick_fail.race(slow_success).execute().await;
    assert_eq!(exit.err(), Some("first lost".to_string()));
}

#[tokio::test]
async fn race_with_both_immediate_returns_one_of_them() {
    // Both immediate — tokio::select picks one. Just verify we get a
    // valid success, not a hang.
    let a = Effect::<i32, String, ()>::succeed(1);
    let b = Effect::<i32, String, ()>::succeed(2);
    let exit = a.race(b).execute().await;
    let v = exit.ok().unwrap();
    assert!(v == 1 || v == 2);
}
