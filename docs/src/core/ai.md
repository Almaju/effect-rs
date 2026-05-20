# AI

`effect-ai` is the provider-agnostic LLM layer. `LlmProvider` is a
tiny trait; tests use `FakeLlmProvider`; production wires in whichever
real provider you've implemented. Shipping adapters: `effect-ai-openai`
(`/v1/chat/completions`) and `effect-ai-anthropic` (`/v1/messages`).

```toml
[dependencies]
effect    = { version = "0.0.1" }
effect-ai = { version = "0.0.1" }
```

## The shape

```rust,ignore
pub trait LlmProvider: Send + Sync + 'static {
    fn complete(&self, request: ChatRequest)
        -> AsyncResult<Result<ChatResponse, LlmError>>;

    fn complete_stream(&self, request: ChatRequest)
        -> AsyncResult<BoxStream<'static, Result<Delta, LlmError>>>;
}
```

`complete_stream` has a default impl that wraps `complete` into a
three-event stream — good enough to develop and test against; real
SSE forwarding is per-provider.

Plus six Effect / Stream helpers:

| Helper                                | When                              |
| ------------------------------------- | --------------------------------- |
| `complete::<R>(request)`              | full control over `ChatRequest`   |
| `chat::<R>(model, msgs)`              | full message history → text       |
| `simple::<R>(model, prompt)`          | single user prompt → text         |
| `complete_stream::<R>(request)`       | streaming, full `ChatRequest`     |
| `chat_stream::<R>(model, msgs)`       | streaming, full history           |
| `simple_stream::<R>(model, prompt)`   | streaming, single prompt          |

## Build a request

```rust,no_run
use effect_ai::*;

let request = ChatRequest::new("gpt-4o-mini", vec![
    ChatMessage::system("You answer in haiku."),
    ChatMessage::user("Why is the sky blue?"),
])
.temperature(0.7)
.max_tokens(60);
```

## Call it

```rust,no_run
use effect::Effect;
use effect_ai::*;

# pub struct MyProvider;
# impl LlmProvider for MyProvider {
#   fn complete(&self, _: ChatRequest)
#     -> AsyncResult<Result<ChatResponse, LlmError>> {
#     Box::pin(async { Ok(reply("hi")) })
#   }
# }
# #[tokio::main] async fn main() {
let answer = simple::<MyProvider>("gpt-4o-mini", "What's 2 + 2?")
    .run_with(MyProvider)
    .await;
# }
```

`complete` / `chat` / `simple` are bound on `R: LlmProvider`, so the
program declares "I need an LLM" and you wire one in at the edge.

## Streaming the response

Sometimes you want the assistant text rendering as it arrives — for a
chat UI, a streaming CLI, or to start consuming structured output
before the model has finished writing it. Use the `*_stream` helpers:

```rust,no_run
use effect_ai::*;
use effect_stream::Stream;

# pub struct P;
# impl LlmProvider for P {
#   fn complete(&self, _: ChatRequest) -> AsyncResult<Result<ChatResponse, LlmError>> {
#     Box::pin(async { Ok(reply("hi")) })
#   }
# }
# #[tokio::main] async fn main() {
let stream: Stream<Delta, LlmError, P> =
    simple_stream::<P>("gpt-4o-mini", "summarize the news");

let _ = stream
    .run_for_each(|delta| match delta {
        Delta::Content(text) => print!("{text}"),
        Delta::Usage(u)      => eprintln!("\n[{} tokens]", u.total()),
        Delta::Finish(why)   => eprintln!("[{why}]"),
    })
    .run_with(P)
    .await;
# }
```

A `Delta` is one of:

| Variant                | Meaning                                |
| ---------------------- | -------------------------------------- |
| `Delta::Content(String)` | a chunk of assistant text           |
| `Delta::Usage(TokenUsage)` | usage report (optional, near end) |
| `Delta::Finish(String)`  | stop reason (`"stop"`, `"length"`, …) |

The default `LlmProvider::complete_stream` calls `complete` and emits
the response as a synthetic three-event stream. The current shipping
adapters (`effect-ai-openai`, `effect-ai-anthropic`) inherit that
default — they will be upgraded to forward real SSE deltas once
`effect-http` grows a streaming-body API. Until then, `*_stream`
gives you the same surface and per-delta API; rolling out true
incremental rendering becomes a per-provider change with no impact on
calling code.

## Errors

```rust,ignore
pub enum LlmError {
    Network(String),
    Provider(String),
    RateLimit { retry_after_seconds: Option<u64> },
    InvalidRequest(String),
    InvalidResponse(String),
    ContextLengthExceeded { used: u32, max: u32 },
}
```

`RateLimit` carries a hint so you can pair with [`Effect::retry`] and
a [`Schedule::spaced`] honouring the suggested back-off.

## Testing — `FakeLlmProvider`

Two modes:

```rust,no_run
use effect_ai::*;

# fn main() {
let p = FakeLlmProvider::new();

// Mode 1: every call returns the same reply.
p.expect(reply("OK"));

// Mode 2: scripted sequence — pop one per call.
p.script(vec![
    Ok(reply("first")),
    Ok(reply("second")),
    Err(LlmError::RateLimit { retry_after_seconds: Some(5) }),
]);

// Or a one-shot error:
p.expect_error(LlmError::Network("connection refused".into()));
# }
```

And `p.recorded()` returns every `ChatRequest` that's been sent, so
you can assert on the prompt, model, message history, temperature,
or max-tokens without inspecting any wire format.

## Composition example

Pair with [`Effect::retry`] + [`Schedule`] for resilient calls:

```rust,no_run
use effect::{Effect, Schedule};
use effect_ai::*;
use std::time::Duration;

# pub struct P;
# impl LlmProvider for P {
#   fn complete(&self, _: ChatRequest) -> AsyncResult<Result<ChatResponse, LlmError>> {
#     Box::pin(async { Ok(reply("ok")) })
#   }
# }
# #[tokio::main] async fn main() {
let resilient: Effect<String, LlmError, P> =
    simple::<P>("gpt-x", "summarize this article")
        .retry(
            Schedule::exponential(Duration::from_millis(500))
                .max_attempts(3)
                .bounded(Duration::from_secs(30))
        );
# }
```

## What's coming

- **Real SSE streaming** for `effect-ai-openai` /
  `effect-ai-anthropic`, once `effect-http` exposes a streaming
  response body. The `Stream<Delta, …>` surface won't change.
- **Local providers** — `effect-ai-ollama`, llama.cpp adapter.
- **Tool / function calling** — typed tool declarations, structured
  outputs validated via Schema.
- **Embeddings** — `embed::<R>(model, text) -> Effect<Vec<f32>, …>`.
- **Conversation memory** — `Ref<Vec<ChatMessage>>`-backed assistant
  state with prune-by-token-budget.

[`Effect::retry`]: ./retries.md
[`Schedule`]: ./retries.md#schedule-combinators
[`Schedule::spaced`]: ./retries.md#schedule-combinators
