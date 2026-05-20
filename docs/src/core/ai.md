# AI

`effect-ai` is the provider-agnostic LLM layer. `LlmProvider` is a
tiny trait; tests use `FakeLlmProvider`; production wires in whichever
real provider you've implemented (OpenAI, Anthropic, local Ollama,
etc. — adapter crates are deferred to a follow-on pass).

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
}
```

Plus three Effect helpers:

| Helper                                | When                              |
| ------------------------------------- | --------------------------------- |
| `complete::<R>(request)`              | full control over `ChatRequest`   |
| `chat::<R>(model, msgs)`              | full message history → text       |
| `simple::<R>(model, prompt)`          | single user prompt → text         |

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

- **Provider adapters** — `effect-ai-openai`, `effect-ai-anthropic`,
  `effect-ai-ollama`. Each ~50 lines around an HttpClient.
- **Streaming completions** — `Stream<DeltaToken, LlmError, R>` so
  output renders as it arrives.
- **Tool / function calling** — typed tool declarations, structured
  outputs validated via Schema.
- **Embeddings** — `embed::<R>(model, text) -> Effect<Vec<f32>, …>`.
- **Conversation memory** — `Ref<Vec<ChatMessage>>`-backed assistant
  state with prune-by-token-budget.

[`Effect::retry`]: ./retries.md
[`Schedule`]: ./retries.md#schedule-combinators
[`Schedule::spaced`]: ./retries.md#schedule-combinators
