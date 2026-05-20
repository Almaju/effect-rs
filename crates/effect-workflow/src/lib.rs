//! Durable workflows over an effect-typed step journal.
//!
//! A workflow is identified by a string ID. Each named step inside the
//! workflow body is **journaled** to storage on first run; subsequent
//! runs of the same workflow ID replay the journal, skipping the
//! actual work and reusing the recorded result. This is the
//! "event-sourced replay" pattern (Temporal / Cadence / Effect-TS's
//! Workflow).
//!
//! ```ignore
//! use effect_workflow::*;
//! use effect::Schema;
//! use std::sync::Arc;
//!
//! let storage = Arc::new(InMemoryWorkflowStorage::new());
//!
//! let body = |ctx: Arc<WorkflowContext<InMemoryWorkflowStorage>>| async move {
//!     let user = ctx.step("fetch_user", async {
//!         Ok::<_, WorkflowError>(42_u64)
//!     }).await?;
//!     let email = ctx.step("fetch_email", async move {
//!         Ok::<_, WorkflowError>(format!("user-{user}@example.com"))
//!     }).await?;
//!     Ok::<_, WorkflowError>(email)
//! };
//!
//! let result = workflow::<_, _, _, _>("wf-1", body).run_with(storage).await;
//! ```

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use effect::{Effect, Schema};
use effect_schema::serde_json;
use thiserror::Error;

pub type AsyncResult<T> = Pin<Box<dyn Future<Output = T> + Send>>;

// ── Storage ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct StepRecord {
    pub name: String,
    pub result_json: Vec<u8>,
}

pub trait WorkflowStorage: Send + Sync + 'static {
    fn load(&self, workflow_id: &str) -> AsyncResult<Result<Vec<StepRecord>, WorkflowError>>;
    fn append(
        &self,
        workflow_id: &str,
        record: StepRecord,
    ) -> AsyncResult<Result<(), WorkflowError>>;
}

/// In-memory journal — for tests and ephemeral workflows.
pub struct InMemoryWorkflowStorage {
    inner: Mutex<HashMap<String, Vec<StepRecord>>>,
}

impl InMemoryWorkflowStorage {
    pub fn new() -> Self {
        InMemoryWorkflowStorage {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryWorkflowStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkflowStorage for InMemoryWorkflowStorage {
    fn load(&self, workflow_id: &str) -> AsyncResult<Result<Vec<StepRecord>, WorkflowError>> {
        let id = workflow_id.to_string();
        let recs = self
            .inner
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .unwrap_or_default();
        Box::pin(async move { Ok(recs) })
    }
    fn append(
        &self,
        workflow_id: &str,
        record: StepRecord,
    ) -> AsyncResult<Result<(), WorkflowError>> {
        let id = workflow_id.to_string();
        self.inner.lock().unwrap().entry(id).or_default().push(record);
        Box::pin(async move { Ok(()) })
    }
}

// ── Errors ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Error)]
pub enum WorkflowError {
    #[error("storage error: {0}")]
    Storage(String),

    #[error("schema error encoding step result: {0}")]
    EncodeStep(String),

    #[error("schema error decoding journaled step result for '{step}': {message}")]
    DecodeStep { step: String, message: String },

    #[error(
        "journal mismatch: step at position {position} was '{recorded}' but body asked for '{requested}'"
    )]
    JournalMismatch {
        position: usize,
        recorded: String,
        requested: String,
    },

    #[error("step '{0}' failed: {1}")]
    StepFailed(String, String),
}

// ── Context ──────────────────────────────────────────────────────

pub struct WorkflowContext<S: WorkflowStorage> {
    storage: Arc<S>,
    workflow_id: String,
    state: Mutex<CtxState>,
}

struct CtxState {
    journal: Vec<StepRecord>,
    cursor: usize,
}

impl<S: WorkflowStorage> WorkflowContext<S> {
    /// Run-or-replay a named step. On first run, executes `compute`,
    /// journals the (Schema-encoded) result. On replay, decodes the
    /// journaled value and skips `compute` entirely.
    pub async fn step<T, Fut>(&self, name: &str, compute: Fut) -> Result<T, WorkflowError>
    where
        T: Schema + Send + 'static,
        Fut: Future<Output = Result<T, WorkflowError>> + Send + 'static,
    {
        // First, see if we have a journaled record at this cursor.
        let (have_replay, position) = {
            let st = self.state.lock().unwrap();
            (st.cursor < st.journal.len(), st.cursor)
        };

        if have_replay {
            let st = self.state.lock().unwrap();
            let record = st.journal[position].clone();
            drop(st);

            if record.name != name {
                return Err(WorkflowError::JournalMismatch {
                    position,
                    recorded: record.name,
                    requested: name.to_string(),
                });
            }

            let json: serde_json::Value =
                serde_json::from_slice(&record.result_json).map_err(|e| {
                    WorkflowError::DecodeStep {
                        step: name.to_string(),
                        message: format!("invalid stored JSON: {e}"),
                    }
                })?;
            let value = T::parse_json(&json).map_err(|e| WorkflowError::DecodeStep {
                step: name.to_string(),
                message: format!("{e}"),
            })?;
            self.state.lock().unwrap().cursor += 1;
            return Ok(value);
        }

        // Not in journal — run for real, then journal the result.
        let value = compute.await?;
        let json = value.encode_json();
        let bytes = serde_json::to_vec(&json)
            .map_err(|e| WorkflowError::EncodeStep(e.to_string()))?;
        let record = StepRecord {
            name: name.to_string(),
            result_json: bytes,
        };
        self.storage
            .append(&self.workflow_id, record.clone())
            .await?;
        {
            let mut st = self.state.lock().unwrap();
            st.journal.push(record);
            st.cursor += 1;
        }
        Ok(value)
    }

    /// Read-only view of how many steps have been journaled so far.
    pub fn journal_len(&self) -> usize {
        self.state.lock().unwrap().journal.len()
    }
}

// ── workflow() constructor ──────────────────────────────────────

/// Wrap a `body` async closure as an Effect that loads/replays/extends
/// a workflow journal keyed by `id`.
pub fn workflow<S, T, F, Fut>(id: impl Into<String>, body: F) -> Effect<T, WorkflowError, S>
where
    S: WorkflowStorage,
    T: Send + 'static,
    F: Fn(Arc<WorkflowContext<S>>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<T, WorkflowError>> + Send + 'static,
{
    let id = id.into();
    let body = Arc::new(body);
    Effect::from_fn(move |s: Arc<S>| {
        let id = id.clone();
        let body = body.clone();
        async move {
            let journal = s.load(&id).await?;
            let ctx = Arc::new(WorkflowContext {
                storage: s,
                workflow_id: id,
                state: Mutex::new(CtxState { journal, cursor: 0 }),
            });
            body(ctx).await
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn first_run_executes_step_and_journals_it() {
        let storage = Arc::new(InMemoryWorkflowStorage::new());
        let counter = Arc::new(AtomicUsize::new(0));

        let counter_clone = counter.clone();
        let exit = workflow::<_, u64, _, _>(
            "wf-1",
            move |ctx| {
                let counter = counter_clone.clone();
                async move {
                    let n = ctx
                        .step("count", async move {
                            Ok(counter.fetch_add(1, Ordering::SeqCst) as u64 + 1)
                        })
                        .await?;
                    Ok(n)
                }
            },
        )
        .run(storage.clone())
        .await;

        assert_eq!(exit.ok(), Some(1));
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        let recs = storage.load("wf-1").await.ok().unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].name, "count");
    }

    #[tokio::test]
    async fn second_run_replays_journal_without_re_executing() {
        let storage = Arc::new(InMemoryWorkflowStorage::new());
        let counter = Arc::new(AtomicUsize::new(0));

        let body = {
            let counter = counter.clone();
            move |ctx: Arc<WorkflowContext<InMemoryWorkflowStorage>>| {
                let counter = counter.clone();
                async move {
                    let n = ctx
                        .step("count", async move {
                            Ok(counter.fetch_add(1, Ordering::SeqCst) as u64 + 1)
                        })
                        .await?;
                    Ok(n)
                }
            }
        };

        // First run: actually runs the body, counter → 1.
        let _ = workflow::<_, u64, _, _>("wf-2", body.clone())
            .run(storage.clone())
            .await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Second run: replays from journal, counter unchanged.
        let exit2 = workflow::<_, u64, _, _>("wf-2", body)
            .run(storage.clone())
            .await;
        assert_eq!(exit2.ok(), Some(1));
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn multiple_steps_journal_independently() {
        let storage = Arc::new(InMemoryWorkflowStorage::new());

        let exit = workflow::<_, String, _, _>(
            "wf-3",
            |ctx| async move {
                let a = ctx
                    .step("a", async { Ok(10_u64) })
                    .await?;
                let b = ctx
                    .step("b", async move {
                        Ok(format!("a was {a}"))
                    })
                    .await?;
                Ok(b)
            },
        )
        .run(storage.clone())
        .await;

        assert_eq!(exit.ok(), Some("a was 10".to_string()));
        let recs = storage.load("wf-3").await.ok().unwrap();
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].name, "a");
        assert_eq!(recs[1].name, "b");
    }

    #[tokio::test]
    async fn journal_mismatch_is_a_typed_error() {
        let storage = Arc::new(InMemoryWorkflowStorage::new());

        // First run: journal "first".
        let _ = workflow::<_, u64, _, _>(
            "wf-4",
            |ctx| async move {
                ctx.step("first", async { Ok(1_u64) }).await
            },
        )
        .run(storage.clone())
        .await;

        // Second run with a different step name at the same position.
        let exit = workflow::<_, u64, _, _>(
            "wf-4",
            |ctx| async move {
                ctx.step("second", async { Ok(2_u64) }).await
            },
        )
        .run(storage.clone())
        .await;

        match exit.err() {
            Some(WorkflowError::JournalMismatch {
                position,
                recorded,
                requested,
            }) => {
                assert_eq!(position, 0);
                assert_eq!(recorded, "first");
                assert_eq!(requested, "second");
            }
            other => panic!("expected JournalMismatch, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn step_failure_propagates_without_journaling() {
        let storage = Arc::new(InMemoryWorkflowStorage::new());

        let exit = workflow::<_, u64, _, _>(
            "wf-5",
            |ctx| async move {
                ctx.step("will_fail", async {
                    Err::<u64, _>(WorkflowError::StepFailed(
                        "will_fail".into(),
                        "nope".into(),
                    ))
                })
                .await
            },
        )
        .run(storage.clone())
        .await;

        assert!(matches!(exit.err(), Some(WorkflowError::StepFailed(_, _))));
        let recs = storage.load("wf-5").await.ok().unwrap();
        assert!(recs.is_empty(), "failed steps must not be journaled");
    }

    #[tokio::test]
    async fn resumption_skips_completed_steps_and_runs_new_ones() {
        // Simulate: crash after step "a", then resume with same body
        // which adds step "b". Step "a" must NOT re-run; step "b"
        // should run for the first time.
        let storage = Arc::new(InMemoryWorkflowStorage::new());
        let a_runs = Arc::new(AtomicUsize::new(0));
        let b_runs = Arc::new(AtomicUsize::new(0));

        // First run — only step "a" is requested (simulates body crashing
        // before reaching "b").
        let a1 = a_runs.clone();
        let _ = workflow::<_, u64, _, _>(
            "wf-6",
            move |ctx| {
                let a1 = a1.clone();
                async move {
                    ctx.step("a", async move {
                        a1.fetch_add(1, Ordering::SeqCst);
                        Ok(1_u64)
                    })
                    .await
                }
            },
        )
        .run(storage.clone())
        .await;

        // Second run — body now reaches "b" too.
        let a2 = a_runs.clone();
        let b2 = b_runs.clone();
        let exit = workflow::<_, u64, _, _>(
            "wf-6",
            move |ctx| {
                let a2 = a2.clone();
                let b2 = b2.clone();
                async move {
                    let av = ctx
                        .step("a", async move {
                            a2.fetch_add(1, Ordering::SeqCst);
                            Ok(1_u64)
                        })
                        .await?;
                    let bv = ctx
                        .step("b", async move {
                            b2.fetch_add(1, Ordering::SeqCst);
                            Ok(av + 10)
                        })
                        .await?;
                    Ok(bv)
                }
            },
        )
        .run(storage.clone())
        .await;

        assert_eq!(exit.ok(), Some(11));
        // a ran exactly once (first time); b ran exactly once (second time).
        assert_eq!(a_runs.load(Ordering::SeqCst), 1);
        assert_eq!(b_runs.load(Ordering::SeqCst), 1);
    }
}
