//! Provider-agnostic LLM abstractions.
//!
//! [`LlmProvider`] is the service trait. Real providers (OpenAI,
//! Anthropic, local Ollama, etc.) implement it; tests use
//! [`FakeLlmProvider`], which serves either a single canned reply or
//! a scripted sequence.
//!
//! Effect helpers:
//! - [`complete`] — full `ChatRequest` → `ChatResponse`
//! - [`chat`] — model + messages → assistant text
//! - [`simple`] — model + single user prompt → assistant text
//!
//! ```ignore
//! use effect::Effect;
//! use effect_ai::*;
//!
//! let answer = simple::<MyProvider>("gpt-4o-mini", "What's 2 + 2?")
//!     .run_with(my_provider)
//!     .await;
//! ```

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use effect::Effect;
use thiserror::Error;

pub type AsyncResult<T> = Pin<Box<dyn Future<Output = T> + Send>>;

// ── Types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        ChatMessage { role: Role::System, content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        ChatMessage { role: Role::User, content: content.into() }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        ChatMessage { role: Role::Assistant, content: content.into() }
    }
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>, messages: Vec<ChatMessage>) -> Self {
        ChatRequest {
            model: model.into(),
            messages,
            temperature: None,
            max_tokens: None,
        }
    }
    pub fn temperature(mut self, t: f32) -> Self {
        self.temperature = Some(t);
        self
    }
    pub fn max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = Some(n);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenUsage {
    pub input: u32,
    pub output: u32,
}

impl TokenUsage {
    pub fn total(&self) -> u32 {
        self.input + self.output
    }
}

#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: String,
    pub model: String,
    pub usage: Option<TokenUsage>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Error)]
pub enum LlmError {
    #[error("network/transport error: {0}")]
    Network(String),

    #[error("provider error: {0}")]
    Provider(String),

    #[error("rate limited; retry after {retry_after_seconds:?}s")]
    RateLimit {
        retry_after_seconds: Option<u64>,
    },

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("invalid response: {0}")]
    InvalidResponse(String),

    #[error("context length exceeded (used {used}, max {max})")]
    ContextLengthExceeded {
        used: u32,
        max: u32,
    },
}

// ── Service trait ────────────────────────────────────────────────

pub trait LlmProvider: Send + Sync + 'static {
    fn complete(
        &self,
        request: ChatRequest,
    ) -> AsyncResult<Result<ChatResponse, LlmError>>;
}

// ── Effect helpers ───────────────────────────────────────────────

pub fn complete<R: LlmProvider>(request: ChatRequest) -> Effect<ChatResponse, LlmError, R> {
    Effect::from_fn(move |r: Arc<R>| {
        let request = request.clone();
        async move { r.complete(request).await }
    })
}

/// Convenience: send a single user message, return just the assistant
/// content.
pub fn simple<R: LlmProvider>(
    model: impl Into<String>,
    prompt: impl Into<String>,
) -> Effect<String, LlmError, R> {
    let request = ChatRequest::new(model, vec![ChatMessage::user(prompt)]);
    complete::<R>(request).map(|r| r.content)
}

/// Convenience: pass a full message history, return just the
/// assistant content.
pub fn chat<R: LlmProvider>(
    model: impl Into<String>,
    messages: Vec<ChatMessage>,
) -> Effect<String, LlmError, R> {
    let request = ChatRequest::new(model, messages);
    complete::<R>(request).map(|r| r.content)
}

// ── FakeLlmProvider ──────────────────────────────────────────────

/// A provider that returns canned responses for tests.
///
/// - `expect(response)` returns the same response for every call.
/// - `script([r1, r2, …])` queues a sequence; each call pops one.
/// - `recorded()` returns the requests the provider has seen.
pub struct FakeLlmProvider {
    inner: Mutex<FakeInner>,
}

enum Behavior {
    Constant(Result<ChatResponse, LlmError>),
    Script(VecDeque<Result<ChatResponse, LlmError>>),
    Empty,
}

struct FakeInner {
    behavior: Behavior,
    recorded: Vec<ChatRequest>,
}

impl FakeLlmProvider {
    pub fn new() -> Self {
        FakeLlmProvider {
            inner: Mutex::new(FakeInner {
                behavior: Behavior::Empty,
                recorded: Vec::new(),
            }),
        }
    }

    /// Every call returns the same canned response.
    pub fn expect(&self, response: ChatResponse) {
        self.inner.lock().unwrap().behavior = Behavior::Constant(Ok(response));
    }

    pub fn expect_error(&self, err: LlmError) {
        self.inner.lock().unwrap().behavior = Behavior::Constant(Err(err));
    }

    /// Calls pop responses from the queue in order. Once empty, calls
    /// produce `LlmError::Provider("script exhausted")`.
    pub fn script<I>(&self, responses: I)
    where
        I: IntoIterator<Item = Result<ChatResponse, LlmError>>,
    {
        self.inner.lock().unwrap().behavior = Behavior::Script(responses.into_iter().collect());
    }

    pub fn recorded(&self) -> Vec<ChatRequest> {
        self.inner.lock().unwrap().recorded.clone()
    }
}

impl Default for FakeLlmProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmProvider for FakeLlmProvider {
    fn complete(
        &self,
        request: ChatRequest,
    ) -> AsyncResult<Result<ChatResponse, LlmError>> {
        let mut guard = self.inner.lock().unwrap();
        guard.recorded.push(request);
        let result: Result<ChatResponse, LlmError> = match &mut guard.behavior {
            Behavior::Constant(r) => r.clone(),
            Behavior::Script(q) => q
                .pop_front()
                .unwrap_or_else(|| Err(LlmError::Provider("script exhausted".into()))),
            Behavior::Empty => Err(LlmError::Provider("FakeLlmProvider not configured".into())),
        };
        Box::pin(async move { result })
    }
}

// ── Convenience response builders for tests ──────────────────────

/// Build a `ChatResponse` with just the content.
pub fn reply(content: impl Into<String>) -> ChatResponse {
    ChatResponse {
        content: content.into(),
        model: "fake".into(),
        usage: None,
        finish_reason: Some("stop".into()),
    }
}

pub fn reply_with_usage(
    content: impl Into<String>,
    input_tokens: u32,
    output_tokens: u32,
) -> ChatResponse {
    ChatResponse {
        content: content.into(),
        model: "fake".into(),
        usage: Some(TokenUsage {
            input: input_tokens,
            output: output_tokens,
        }),
        finish_reason: Some("stop".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn simple_sends_one_user_message_and_returns_content() {
        let prov = FakeLlmProvider::new();
        prov.expect(reply("hello there"));
        let arc = Arc::new(prov);

        let exit = simple::<FakeLlmProvider>("gpt-x", "hi")
            .run(arc.clone())
            .await;
        assert_eq!(exit.ok(), Some("hello there".to_string()));

        let recorded = arc.recorded();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].model, "gpt-x");
        assert_eq!(recorded[0].messages.len(), 1);
        assert_eq!(recorded[0].messages[0].role, Role::User);
        assert_eq!(recorded[0].messages[0].content, "hi");
    }

    #[tokio::test]
    async fn chat_sends_full_history() {
        let prov = FakeLlmProvider::new();
        prov.expect(reply("ack"));
        let arc = Arc::new(prov);

        let _ = chat::<FakeLlmProvider>(
            "gpt-x",
            vec![
                ChatMessage::system("You are concise"),
                ChatMessage::user("What's the capital of France?"),
                ChatMessage::assistant("Paris."),
                ChatMessage::user("And of Spain?"),
            ],
        )
        .run(arc.clone())
        .await;

        let recorded = arc.recorded();
        assert_eq!(recorded[0].messages.len(), 4);
        assert_eq!(recorded[0].messages[0].role, Role::System);
        assert_eq!(recorded[0].messages[3].content, "And of Spain?");
    }

    #[tokio::test]
    async fn script_returns_sequential_responses() {
        let prov = FakeLlmProvider::new();
        prov.script(vec![Ok(reply("one")), Ok(reply("two")), Ok(reply("three"))]);
        let arc = Arc::new(prov);

        let first = simple::<FakeLlmProvider>("m", "x").run(arc.clone()).await;
        let second = simple::<FakeLlmProvider>("m", "y").run(arc.clone()).await;
        let third = simple::<FakeLlmProvider>("m", "z").run(arc.clone()).await;

        assert_eq!(first.ok(), Some("one".into()));
        assert_eq!(second.ok(), Some("two".into()));
        assert_eq!(third.ok(), Some("three".into()));
    }

    #[tokio::test]
    async fn script_exhausted_returns_provider_error() {
        let prov = FakeLlmProvider::new();
        prov.script(vec![Ok(reply("only one"))]);
        let arc = Arc::new(prov);
        let _ = simple::<FakeLlmProvider>("m", "x").run(arc.clone()).await;
        let second = simple::<FakeLlmProvider>("m", "y").run(arc.clone()).await;
        match second.err() {
            Some(LlmError::Provider(msg)) => assert!(msg.contains("exhausted")),
            other => panic!("expected Provider error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn expect_error_propagates_typed_failure() {
        let prov = FakeLlmProvider::new();
        prov.expect_error(LlmError::RateLimit {
            retry_after_seconds: Some(30),
        });
        let exit = simple::<FakeLlmProvider>("m", "x").run_with(prov).await;
        match exit.err() {
            Some(LlmError::RateLimit { retry_after_seconds }) => {
                assert_eq!(retry_after_seconds, Some(30));
            }
            other => panic!("expected RateLimit, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn complete_with_temperature_and_max_tokens_passes_them_through() {
        let prov = FakeLlmProvider::new();
        prov.expect(reply_with_usage("ok", 10, 5));
        let arc = Arc::new(prov);

        let request = ChatRequest::new("m", vec![ChatMessage::user("hi")])
            .temperature(0.7)
            .max_tokens(100);
        let exit = complete::<FakeLlmProvider>(request).run(arc.clone()).await;
        let response = exit.ok().unwrap();
        assert_eq!(response.usage.unwrap().total(), 15);

        let recorded = arc.recorded();
        assert_eq!(recorded[0].temperature, Some(0.7));
        assert_eq!(recorded[0].max_tokens, Some(100));
    }

    #[tokio::test]
    async fn unconfigured_fake_returns_provider_error() {
        let prov = FakeLlmProvider::new();
        let exit = simple::<FakeLlmProvider>("m", "x").run_with(prov).await;
        match exit.err() {
            Some(LlmError::Provider(msg)) => assert!(msg.contains("not configured")),
            other => panic!("expected Provider error, got {other:?}"),
        }
    }

    #[test]
    fn token_usage_total_sums_input_and_output() {
        let u = TokenUsage { input: 7, output: 3 };
        assert_eq!(u.total(), 10);
    }

    #[test]
    fn message_constructors_set_role_correctly() {
        assert_eq!(ChatMessage::system("x").role, Role::System);
        assert_eq!(ChatMessage::user("x").role, Role::User);
        assert_eq!(ChatMessage::assistant("x").role, Role::Assistant);
    }
}
