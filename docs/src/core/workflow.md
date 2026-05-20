# Workflows

`effect-workflow` is **event-sourced replay** in ~250 lines. Each
named step inside a workflow body is journaled to storage on first
run; subsequent runs of the same workflow ID replay the journal,
skipping the actual work and reusing the recorded result.

This is the kernel of what Temporal / Cadence / Effect-TS Workflow do
— enough to write a durable workflow today, while a richer feature
set (timers, signals, child workflows) lands later.

```toml
[dependencies]
effect          = { version = "0.0.1" }
effect-schema   = { version = "0.0.1" }
effect-workflow = { version = "0.0.1" }
```

## A first workflow

```rust,no_run
use effect_workflow::*;
use effect::Schema;
use std::sync::Arc;

# #[tokio::main] async fn main() {
let storage = Arc::new(InMemoryWorkflowStorage::new());

let result = workflow::<_, String, _, _>("user-onboard-42", |ctx| async move {
    let user_id = ctx.step("create_user", async {
        // Imagine this is an HTTP call. On replay, the result comes
        // straight from the journal — no HTTP call made.
        Ok(42_u64)
    }).await?;

    let email = ctx.step("send_welcome", async move {
        Ok(format!("welcomed user-{user_id}"))
    }).await?;

    Ok(email)
})
.run_with(storage)
.await;

assert_eq!(result.ok(), Some("welcomed user-42".to_string()));
# }
```

The body is a regular async closure. Each `ctx.step(name, fut)` call:

1. Looks at the journal at its position.
2. If a record exists with the same name, decodes it via `Schema` and
   returns immediately — the future inside is never polled.
3. Otherwise runs the future, encodes the result via `Schema`,
   appends the record to storage, advances the cursor, returns.

## The story it tells

A workflow's *contract* is: "if you crash mid-way, restart the
workflow with the same ID and same body, and it'll pick up where it
left off — without re-doing any side effects you already committed."

That's the failure model. Concretely:

| Where the body left off | What the next run does                          |
| ----------------------- | ----------------------------------------------- |
| `step("a")` already ran | replays `a` from journal, skips its body        |
| `step("b")` not yet     | runs `b` for real, journals the result          |
| body returned successfully | further runs replay the whole journal end-to-end |

```rust,no_run
# use effect_workflow::*;
# use std::sync::Arc;
# use std::sync::atomic::{AtomicUsize, Ordering};
# #[tokio::main] async fn main() {
let storage = Arc::new(InMemoryWorkflowStorage::new());
let runs = Arc::new(AtomicUsize::new(0));

// First run: actually runs.
let runs1 = runs.clone();
let _ = workflow::<_, u64, _, _>("wf", move |ctx| {
    let runs = runs1.clone();
    async move {
        ctx.step("compute", async move {
            Ok(runs.fetch_add(1, Ordering::SeqCst) as u64 + 1)
        }).await
    }
}).run(storage.clone()).await;
assert_eq!(runs.load(Ordering::SeqCst), 1);

// Second run: replays from journal — counter unchanged.
let runs2 = runs.clone();
let _ = workflow::<_, u64, _, _>("wf", move |ctx| {
    let runs = runs2.clone();
    async move {
        ctx.step("compute", async move {
            Ok(runs.fetch_add(1, Ordering::SeqCst) as u64 + 1)
        }).await
    }
}).run(storage).await;
assert_eq!(runs.load(Ordering::SeqCst), 1);   // ← still 1
# }
```

## Determinism contract

For replay to be correct, the body must call the same `step(name, …)`
sequence on every run — in the same order, with the same names.

- **Reordering a step** breaks replay (caught as `JournalMismatch`).
- **Renaming a step** breaks replay (also `JournalMismatch`).
- **Conditional logic that depends on time/randomness** breaks
  replay — those should themselves be inside steps so their results
  are journaled.

The general rule: **anything observable from outside should live
inside a `step`**.

## Errors

```rust,ignore
pub enum WorkflowError {
    Storage(String),
    EncodeStep(String),
    DecodeStep { step, message },
    JournalMismatch { position, recorded, requested },
    StepFailed(String, String),
}
```

A failed step is **not** journaled — re-runs will retry the step. If
you need at-most-once semantics, encode the partial state explicitly
and check it in the next step.

## Storage

`WorkflowStorage` is a tiny trait:

```rust,ignore
pub trait WorkflowStorage: Send + Sync + 'static {
    fn load(&self, workflow_id: &str)   -> AsyncResult<Result<Vec<StepRecord>, WorkflowError>>;
    fn append(&self, workflow_id: &str, record: StepRecord) -> AsyncResult<Result<(), WorkflowError>>;
}
```

`InMemoryWorkflowStorage` ships in this crate (for tests and ephemeral
workflows). For real durability, implement `WorkflowStorage` against
your database of choice — sqlite / postgres are natural fits via
[`effect-sql`](./sql.md):

```rust,ignore
struct SqliteWorkflowStorage(pub SqlitePool);

impl WorkflowStorage for SqliteWorkflowStorage {
    fn load(&self, id: &str) -> AsyncResult<Result<Vec<StepRecord>, WorkflowError>> {
        // SELECT name, result_json FROM workflow_journal
        //   WHERE workflow_id = ? ORDER BY position
    }
    fn append(&self, id: &str, record: StepRecord) -> AsyncResult<Result<(), WorkflowError>> {
        // INSERT INTO workflow_journal (workflow_id, position, name, result_json)
        //   VALUES (?, ?, ?, ?)
    }
}
```

## What's coming

- **Timers** — `ctx.sleep_until(time)` that journals the wake-up.
- **Signals** — external events that wake up a paused workflow.
- **Child workflows** — `ctx.workflow("child", body)` with their own
  journal under a parent.
- **Compensation** — `step` variants with explicit rollback.
- **Cluster integration** — when `effect-cluster` lands, workflows
  pin to entity shards for at-most-one execution.

## Why not a full Temporal in this crate?

For two reasons:

1. **Trust.** Real workflow systems are subtle (idempotency keys,
   replay safety on schema changes, durable timers under retries).
   A confident pre-alpha can't promise correctness here without much
   more design.
2. **Ecosystem.** If you need Temporal semantics today, use Temporal
   — there's a Rust SDK. `effect-workflow` is for the case where
   "journal each step to a database" is enough.
