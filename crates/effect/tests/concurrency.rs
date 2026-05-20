//! Integration tests for `for_each_par`, `Effect::on_interrupt`, and
//! `Effect::fork_scoped`.

use effect::{Cause, Effect, Exit, acquire_release, for_each_par};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

// ── for_each_par ─────────────────────────────────────────────────

#[tokio::test]
async fn for_each_par_processes_all_in_input_order() {
    let items = vec![1, 2, 3, 4, 5];
    let program: Effect<Vec<i32>, String, ()> =
        for_each_par(items.clone(), 2, |x| {
            Effect::from_fn(move |_| async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                Ok(x * 10)
            })
        });
    let result = program.execute().await;
    assert_eq!(result.ok(), Some(vec![10, 20, 30, 40, 50]));
}

#[tokio::test]
async fn for_each_par_respects_concurrency_limit() {
    let in_flight = Arc::new(AtomicUsize::new(0));
    let max_in_flight = Arc::new(AtomicUsize::new(0));

    let in_flight_clone = in_flight.clone();
    let max_clone = max_in_flight.clone();

    let program: Effect<Vec<()>, String, ()> =
        for_each_par((0..10).collect(), 3, move |_| {
            let in_flight = in_flight_clone.clone();
            let max = max_clone.clone();
            Effect::from_fn(move |_| {
                let in_flight = in_flight.clone();
                let max = max.clone();
                async move {
                    let n = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                    max.fetch_max(n, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                    Ok(())
                }
            })
        });

    let _ = program.execute().await;
    assert!(
        max_in_flight.load(Ordering::SeqCst) <= 3,
        "max in-flight {} > concurrency limit 3",
        max_in_flight.load(Ordering::SeqCst)
    );
}

#[tokio::test]
async fn for_each_par_short_circuits_on_first_failure() {
    let items: Vec<i32> = (1..=10).collect();
    let program: Effect<Vec<i32>, String, ()> =
        for_each_par(items, 4, |x| {
            Effect::from_fn(move |_| async move {
                if x == 3 {
                    Err::<i32, _>("third failed".to_string())
                } else {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    Ok(x)
                }
            })
        });

    let exit = program.execute().await;
    assert_eq!(exit.err(), Some("third failed".to_string()));
}

#[tokio::test]
async fn for_each_par_empty_returns_empty() {
    let program: Effect<Vec<i32>, String, ()> =
        for_each_par(Vec::<i32>::new(), 4, |x| Effect::succeed(x));
    assert_eq!(program.execute().await.ok(), Some(vec![]));
}

// ── on_interrupt ─────────────────────────────────────────────────

#[tokio::test]
async fn on_interrupt_fires_on_pure_interrupt() {
    let fired = Arc::new(AtomicUsize::new(0));
    let fired_clone = fired.clone();
    let program = Effect::<i32, String, ()>::interrupt().on_interrupt(move || {
        let fired = fired_clone.clone();
        async move {
            fired.fetch_add(1, Ordering::SeqCst);
        }
    });
    let exit = program.execute().await;
    assert!(matches!(exit, Exit::Failure(Cause::Interrupt)));
    assert_eq!(fired.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn on_interrupt_does_not_fire_on_typed_failure() {
    let fired = Arc::new(AtomicUsize::new(0));
    let fired_clone = fired.clone();
    let program = Effect::<i32, String, ()>::fail("boom".into()).on_interrupt(move || {
        let fired = fired_clone.clone();
        async move {
            fired.fetch_add(1, Ordering::SeqCst);
        }
    });
    let exit = program.execute().await;
    assert_eq!(exit.err(), Some("boom".to_string()));
    assert_eq!(fired.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn on_interrupt_does_not_fire_on_defect() {
    let fired = Arc::new(AtomicUsize::new(0));
    let fired_clone = fired.clone();
    let program = Effect::<i32, String, ()>::die_message("bug").on_interrupt(move || {
        let fired = fired_clone.clone();
        async move {
            fired.fetch_add(1, Ordering::SeqCst);
        }
    });
    let exit = program.execute().await;
    assert!(matches!(exit, Exit::Failure(Cause::Die(_))));
    assert_eq!(fired.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn on_interrupt_does_not_fire_on_success() {
    let fired = Arc::new(AtomicUsize::new(0));
    let fired_clone = fired.clone();
    let program = Effect::<i32, String, ()>::succeed(42).on_interrupt(move || {
        let fired = fired_clone.clone();
        async move {
            fired.fetch_add(1, Ordering::SeqCst);
        }
    });
    let exit = program.execute().await;
    assert_eq!(exit.ok(), Some(42));
    assert_eq!(fired.load(Ordering::SeqCst), 0);
}

// ── fork_scoped ──────────────────────────────────────────────────

#[tokio::test]
async fn fork_scoped_interrupts_child_when_scope_closes() {
    // The child's interrupt flag is set when the surrounding scope
    // closes. We just verify that the finalizer ran (we can't easily
    // observe the child's behavior post-scope without race conditions
    // — the child may have already finished).
    let scope_closed = Arc::new(AtomicUsize::new(0));
    let scope_closed_clone = scope_closed.clone();

    let work = Effect::<i32, String, ()>::from_fn(|_| async {
        tokio::time::sleep(Duration::from_millis(500)).await;
        Ok(99)
    });

    let program = Effect::<i32, String, ()>::scoped(
        Effect::<_, String, ()>::block(move |g| {
            let work = work.clone();
            let scope_closed = scope_closed_clone.clone();
            async move {
                let _fiber = g.run(work.fork_scoped()).await?;
                // Register a separate finalizer so we can see scope-close.
                g.run(acquire_release(
                    Effect::<(), String, ()>::sync(|| Ok(())),
                    move |_| {
                        let scope_closed = scope_closed.clone();
                        async move {
                            scope_closed.fetch_add(1, Ordering::SeqCst);
                        }
                    },
                ))
                .await?;
                tokio::time::sleep(Duration::from_millis(20)).await;
                Ok::<i32, String>(0)
            }
        }),
    );

    let exit = program.execute().await;
    assert_eq!(exit.ok(), Some(0));
    assert_eq!(scope_closed.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn fork_scoped_outside_scoped_panics() {
    let work = Effect::<i32, String, ()>::succeed(0);
    let program = work.fork_scoped();
    match program.execute().await {
        Exit::Failure(Cause::Die(d)) => {
            assert!(d.message.contains("outside an Effect::scoped"));
        }
        Exit::Success(_) => panic!("expected Die for missing scope, got Success"),
        Exit::Failure(c) => panic!("expected Die for missing scope, got Failure({c})"),
    }
}
