//! Integration tests for `Effect::scoped` + `acquire_release` —
//! finalizer semantics on success, failure, and interruption.

use effect::{Cause, Effect, Exit, acquire_release};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn counter() -> Arc<AtomicUsize> {
    Arc::new(AtomicUsize::new(0))
}

#[tokio::test]
async fn finalizer_runs_on_success() {
    let releases = counter();
    let releases_clone = releases.clone();

    let program = Effect::<i32, String, ()>::scoped(
        acquire_release(
            Effect::sync(|| Ok(42)),
            move |_| {
                let releases = releases_clone.clone();
                async move {
                    releases.fetch_add(1, Ordering::SeqCst);
                }
            },
        )
        .map(|n| n + 1),
    );

    assert_eq!(program.execute().await.ok(), Some(43));
    assert_eq!(releases.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn finalizer_runs_on_typed_failure() {
    let releases = counter();
    let releases_clone = releases.clone();

    let program = Effect::<i32, String, ()>::scoped(
        acquire_release(
            Effect::sync(|| Ok::<i32, String>(42)),
            move |_| {
                let releases = releases_clone.clone();
                async move {
                    releases.fetch_add(1, Ordering::SeqCst);
                }
            },
        )
        .flat_map(|_| Effect::<i32, String, ()>::fail("boom".into())),
    );

    assert_eq!(program.execute().await.err(), Some("boom".to_string()));
    assert_eq!(releases.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn finalizer_runs_on_defect() {
    let releases = counter();
    let releases_clone = releases.clone();

    let program = Effect::<i32, String, ()>::scoped(
        acquire_release(
            Effect::sync(|| Ok::<i32, String>(42)),
            move |_| {
                let releases = releases_clone.clone();
                async move {
                    releases.fetch_add(1, Ordering::SeqCst);
                }
            },
        )
        .flat_map(|_| Effect::<i32, String, ()>::die_message("hardware fault")),
    );

    match program.execute().await {
        Exit::Failure(Cause::Die(_)) => {}
        other => panic!("expected Die, got {other:?}"),
    }
    assert_eq!(releases.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn finalizer_runs_on_interrupt() {
    let releases = counter();
    let releases_clone = releases.clone();

    let program = Effect::<i32, String, ()>::scoped(
        acquire_release(
            Effect::sync(|| Ok::<i32, String>(42)),
            move |_| {
                let releases = releases_clone.clone();
                async move {
                    releases.fetch_add(1, Ordering::SeqCst);
                }
            },
        )
        .flat_map(|_| Effect::<i32, String, ()>::interrupt()),
    );

    match program.execute().await {
        Exit::Failure(Cause::Interrupt) => {}
        other => panic!("expected Interrupt, got {other:?}"),
    }
    assert_eq!(releases.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn finalizers_run_in_lifo_order() {
    use std::sync::Mutex;
    let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    let o1 = order.clone();
    let o2 = order.clone();
    let o3 = order.clone();

    let program = Effect::<(), String, ()>::scoped(
        acquire_release(Effect::sync(|| Ok(())), move |_| {
            let o1 = o1.clone();
            async move { o1.lock().unwrap().push("first acquired"); }
        })
        .flat_map(move |_| {
            let o2 = o2.clone();
            acquire_release(Effect::sync(|| Ok(())), move |_| {
                let o2 = o2.clone();
                async move { o2.lock().unwrap().push("second acquired"); }
            })
        })
        .flat_map(move |_| {
            let o3 = o3.clone();
            acquire_release(Effect::sync(|| Ok(())), move |_| {
                let o3 = o3.clone();
                async move { o3.lock().unwrap().push("third acquired"); }
            })
        })
        .void(),
    );

    program.execute().await;
    // Acquired in order 1, 2, 3 → finalized in order 3, 2, 1 (LIFO).
    assert_eq!(
        *order.lock().unwrap(),
        vec!["third acquired", "second acquired", "first acquired"]
    );
}

#[tokio::test]
async fn acquire_release_outside_scoped_panics() {
    let releases = counter();
    let releases_clone = releases.clone();

    // Not wrapped in Effect::scoped — should panic when run.
    let program = acquire_release(
        Effect::<i32, String, ()>::sync(|| Ok(42)),
        move |_| {
            let releases = releases_clone.clone();
            async move {
                releases.fetch_add(1, Ordering::SeqCst);
            }
        },
    );

    // Caught as Cause::Die because sync wraps in catch_unwind.
    match program.execute().await {
        Exit::Failure(Cause::Die(d)) => {
            assert!(d.message.contains("outside an Effect::scoped"));
        }
        other => panic!("expected Die from missing scope, got {other:?}"),
    }
}

#[tokio::test]
async fn finalizers_run_uninterruptibly_even_if_outer_interrupt_pending() {
    // Even with the interrupt flag set when the scope closes, each
    // finalizer must still run to completion.
    let releases = counter();
    let releases_clone = releases.clone();

    let program = Effect::<i32, String, ()>::scoped(
        acquire_release(
            Effect::sync(|| Ok(0)),
            move |_| {
                let releases = releases_clone.clone();
                async move {
                    // Even if the fiber is being torn down, this runs.
                    releases.fetch_add(1, Ordering::SeqCst);
                }
            },
        )
        .flat_map(|_| Effect::<i32, String, ()>::interrupt()),
    );

    let exit = program.execute().await;
    assert!(matches!(exit, Exit::Failure(Cause::Interrupt)));
    assert_eq!(releases.load(Ordering::SeqCst), 1);
}
