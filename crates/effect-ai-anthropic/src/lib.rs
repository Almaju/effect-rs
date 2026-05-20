//! Anthropic provider for [`effect_ai`].
//!
//! POSTs to `/v1/messages`. Differences from the OpenAI shape this
//! adapter smooths over:
//!
//! - **System messages** in `ChatRequest` are joined and sent as
//!   Anthropic's top-level `system` string. The `messages` array
//!   contains only `user` and `assistant` turns.
//! - **`max_tokens` is required** by Anthropic. If the user didn't
//!   set one we default to `1024`.
//! - **Auth** is via `x-api-key` (not `Authorization: Bearer ...`).
//! - **Versioning** sent via `anthropic-version` header (defaults to
//!   `2023-06-01`).
//!
//! ```no_run
//! use effect_ai::*;
//! use effect_ai_anthropic::Anthropic;
//! use effect_http::LiveHttpClient;
//!
//! # #[tokio::main] async fn main() {
//! let provider = Anthropic::new(
//!     std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY"),
//!     LiveHttpClient::new(),
//! );
//!
//! let answer = simple::<Anthropic<LiveHttpClient>>("claude-sonnet-4-5", "What's 2+2?")
//!     .run_with(provider)
//!     .await;
//! # }
//! ```

use effect_ai::{
    AsyncResult, ChatMessage, ChatRequest, ChatResponse, LlmError, LlmProvider, Role,
    TokenUsage,
};
use effect_http::{HttpClient, HttpError, Method, Request as HttpRequest, Response as HttpResponse};
use effect_schema::serde_json::{self, Value, json};

pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com/v1";
pub const DEFAULT_VERSION: &str = "2023-06-01";
pub const DEFAULT_MAX_TOKENS: u32 = 1024;

pub struct Anthropic<H: HttpClient> {
    pub base_url: String,
    pub api_key: String,
    pub anthropic_version: String,
    pub http: H,
}

impl<H: HttpClient> Anthropic<H> {
    /// Construct with the default base URL and API version.
    pub fn new(api_key: impl Into<String>, http: H) -> Self {
        Anthropic {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: api_key.into(),
            anthropic_version: DEFAULT_VERSION.to_string(),
            http,
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.anthropic_version = version.into();
        self
    }
}

impl<H: HttpClient> LlmProvider for Anthropic<H> {
    fn complete(
        &self,
        request: ChatRequest,
    ) -> AsyncResult<Result<ChatResponse, LlmError>> {
        // Pull system messages out (Anthropic wants them as a
        // top-level `system` string, not in the messages array).
        let mut system_parts: Vec<String> = Vec::new();
        let mut messages: Vec<Value> = Vec::new();
        for m in &request.messages {
            match m.role {
                Role::System => system_parts.push(m.content.clone()),
                Role::User => messages.push(json!({
                    "role": "user",
                    "content": m.content,
                })),
                Role::Assistant => messages.push(json!({
                    "role": "assistant",
                    "content": m.content,
                })),
            }
        }

        let mut body = json!({
            "model": request.model,
            "max_tokens": request.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            "messages": messages,
        });
        if !system_parts.is_empty() {
            body["system"] = json!(system_parts.join("\n\n"));
        }
        if let Some(t) = request.temperature {
            body["temperature"] = json!(t);
        }

        let body_bytes = match serde_json::to_vec(&body) {
            Ok(v) => v,
            Err(e) => {
                return Box::pin(async move {
                    Err(LlmError::InvalidRequest(format!("encode failed: {e}")))
                });
            }
        };

        let url = format!("{}/messages", self.base_url);
        let http_req = HttpRequest {
            method: Method::Post,
            url,
            headers: [
                ("x-api-key".to_string(), self.api_key.clone()),
                (
                    "anthropic-version".to_string(),
                    self.anthropic_version.clone(),
                ),
                ("Content-Type".to_string(), "application/json".to_string()),
            ]
            .into_iter()
            .collect(),
            body: Some(body_bytes),
        };

        let send_fut = self.http.send(http_req);

        Box::pin(async move {
            let resp = match send_fut.await {
                Ok(r) => r,
                Err(HttpError::Timeout) => return Err(LlmError::Network("timeout".into())),
                Err(e) => return Err(LlmError::Network(e.to_string())),
            };
            decode_response(resp)
        })
    }
}

fn decode_response(resp: HttpResponse) -> Result<ChatResponse, LlmError> {
    let status = resp.status;
    let body = resp.body;
    let headers = resp.headers;

    if status == 401 {
        return Err(LlmError::InvalidRequest("authentication failed (401)".into()));
    }
    if status == 429 {
        let retry_after = headers
            .get("retry-after")
            .or_else(|| headers.get("Retry-After"))
            .and_then(|s| s.parse::<u64>().ok());
        return Err(LlmError::RateLimit {
            retry_after_seconds: retry_after,
        });
    }
    if !(200..300).contains(&status) {
        let body_str = String::from_utf8_lossy(&body).to_string();
        return Err(LlmError::Provider(format!("HTTP {status}: {body_str}")));
    }

    let value: Value = serde_json::from_slice(&body)
        .map_err(|e| LlmError::InvalidResponse(format!("not JSON: {e}")))?;

    // Anthropic returns content as an array of blocks; concatenate
    // every text block (the common case is one text block).
    let content = value
        .get("content")
        .and_then(|c| c.as_array())
        .ok_or_else(|| LlmError::InvalidResponse("missing content array".into()))?
        .iter()
        .filter_map(|block| {
            block.get("type").and_then(|t| t.as_str()).and_then(|ty| {
                if ty == "text" {
                    block.get("text").and_then(|t| t.as_str())
                } else {
                    None
                }
            })
        })
        .collect::<Vec<_>>()
        .join("");

    if content.is_empty() {
        return Err(LlmError::InvalidResponse(
            "content array contained no text blocks".into(),
        ));
    }

    let model = value
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("unknown")
        .to_string();

    let usage = value.get("usage").map(|u| TokenUsage {
        input: u.get("input_tokens").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
        output: u.get("output_tokens").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
    });

    let finish_reason = value
        .get("stop_reason")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());

    Ok(ChatResponse {
        content,
        model,
        usage,
        finish_reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use effect_ai::*;
    use effect_http::{FakeHttpClient, Response};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn anthropic_response(text: &str, model: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "id": "msg_test",
            "type": "message",
            "role": "assistant",
            "model": model,
            "content": [{"type": "text", "text": text}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 6}
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn happy_path_returns_concatenated_text_blocks() {
        let http = FakeHttpClient::new();
        http.expect(
            "https://api.anthropic.com/v1/messages",
            Response {
                status: 200,
                headers: HashMap::new(),
                body: anthropic_response("Paris.", "claude-sonnet-4-5"),
            },
        );
        let provider = Anthropic::new("test-key", http);

        let exit = simple::<Anthropic<FakeHttpClient>>("claude-sonnet-4-5", "Capital of France?")
            .run_with(provider)
            .await;
        assert_eq!(exit.ok(), Some("Paris.".to_string()));
    }

    #[tokio::test]
    async fn multiple_text_blocks_are_concatenated() {
        let http = FakeHttpClient::new();
        let body = serde_json::to_vec(&json!({
            "id": "msg_x",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4-5",
            "content": [
                {"type": "text", "text": "Hello, "},
                {"type": "text", "text": "world!"}
            ],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 1, "output_tokens": 4}
        })).unwrap();
        http.expect(
            "https://api.anthropic.com/v1/messages",
            Response { status: 200, headers: HashMap::new(), body },
        );
        let exit = simple::<Anthropic<FakeHttpClient>>("claude-sonnet-4-5", "hi")
            .run_with(Anthropic::new("k", http))
            .await;
        assert_eq!(exit.ok(), Some("Hello, world!".to_string()));
    }

    #[tokio::test]
    async fn system_messages_are_extracted_to_top_level_field() {
        let http = FakeHttpClient::new();
        http.expect(
            "https://api.anthropic.com/v1/messages",
            Response {
                status: 200,
                headers: HashMap::new(),
                body: anthropic_response("ok", "m"),
            },
        );
        let arc = Arc::new(http);
        let provider = Anthropic {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: "k".into(),
            anthropic_version: DEFAULT_VERSION.into(),
            http: arc.clone(),
        };
        let _ = chat::<Anthropic<Arc<FakeHttpClient>>>(
            "m",
            vec![
                ChatMessage::system("You are concise."),
                ChatMessage::system("You answer in haiku."),
                ChatMessage::user("Why is the sky blue?"),
            ],
        )
        .run_with(provider)
        .await;

        let recorded = arc.recorded_requests();
        let body: Value = serde_json::from_slice(recorded[0].body.as_ref().unwrap()).unwrap();
        // system is a single string joined with \n\n
        assert_eq!(
            body["system"],
            json!("You are concise.\n\nYou answer in haiku.")
        );
        // messages array excludes systems
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
    }

    #[tokio::test]
    async fn max_tokens_defaults_to_1024_when_not_specified() {
        let http = FakeHttpClient::new();
        http.expect(
            "https://api.anthropic.com/v1/messages",
            Response {
                status: 200,
                headers: HashMap::new(),
                body: anthropic_response("ok", "m"),
            },
        );
        let arc = Arc::new(http);
        let provider = Anthropic {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: "k".into(),
            anthropic_version: DEFAULT_VERSION.into(),
            http: arc.clone(),
        };
        let _ = simple::<Anthropic<Arc<FakeHttpClient>>>("m", "x")
            .run_with(provider)
            .await;

        let body: Value =
            serde_json::from_slice(arc.recorded_requests()[0].body.as_ref().unwrap()).unwrap();
        assert_eq!(body["max_tokens"], json!(DEFAULT_MAX_TOKENS));
    }

    #[tokio::test]
    async fn explicit_max_tokens_is_passed_through() {
        let http = FakeHttpClient::new();
        http.expect(
            "https://api.anthropic.com/v1/messages",
            Response {
                status: 200,
                headers: HashMap::new(),
                body: anthropic_response("ok", "m"),
            },
        );
        let arc = Arc::new(http);
        let provider = Anthropic {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: "k".into(),
            anthropic_version: DEFAULT_VERSION.into(),
            http: arc.clone(),
        };
        let req =
            ChatRequest::new("m", vec![ChatMessage::user("hi")]).max_tokens(50);
        let _ = complete::<Anthropic<Arc<FakeHttpClient>>>(req)
            .run_with(provider)
            .await;
        let body: Value =
            serde_json::from_slice(arc.recorded_requests()[0].body.as_ref().unwrap()).unwrap();
        assert_eq!(body["max_tokens"], json!(50));
    }

    #[tokio::test]
    async fn auth_failure_maps_to_invalid_request() {
        let http = FakeHttpClient::new();
        http.expect(
            "https://api.anthropic.com/v1/messages",
            Response {
                status: 401,
                headers: HashMap::new(),
                body: b"".to_vec(),
            },
        );
        let exit = simple::<Anthropic<FakeHttpClient>>("m", "x")
            .run_with(Anthropic::new("bad", http))
            .await;
        assert!(matches!(exit.err(), Some(LlmError::InvalidRequest(_))));
    }

    #[tokio::test]
    async fn rate_limit_surfaces_retry_after() {
        let http = FakeHttpClient::new();
        let mut headers = HashMap::new();
        headers.insert("retry-after".to_string(), "60".to_string());
        http.expect(
            "https://api.anthropic.com/v1/messages",
            Response {
                status: 429,
                headers,
                body: b"".to_vec(),
            },
        );
        let exit = simple::<Anthropic<FakeHttpClient>>("m", "x")
            .run_with(Anthropic::new("k", http))
            .await;
        match exit.err() {
            Some(LlmError::RateLimit { retry_after_seconds }) => {
                assert_eq!(retry_after_seconds, Some(60));
            }
            other => panic!("expected RateLimit, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn server_error_surfaces_as_provider_with_body() {
        let http = FakeHttpClient::new();
        http.expect(
            "https://api.anthropic.com/v1/messages",
            Response {
                status: 529,
                headers: HashMap::new(),
                body: b"overloaded".to_vec(),
            },
        );
        let exit = simple::<Anthropic<FakeHttpClient>>("m", "x")
            .run_with(Anthropic::new("k", http))
            .await;
        match exit.err() {
            Some(LlmError::Provider(msg)) => {
                assert!(msg.contains("529"));
                assert!(msg.contains("overloaded"));
            }
            other => panic!("expected Provider, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn headers_include_anthropic_specifics() {
        let http = FakeHttpClient::new();
        http.expect(
            "https://api.anthropic.com/v1/messages",
            Response {
                status: 200,
                headers: HashMap::new(),
                body: anthropic_response("ok", "m"),
            },
        );
        let arc = Arc::new(http);
        let provider = Anthropic {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: "sk-test".into(),
            anthropic_version: DEFAULT_VERSION.into(),
            http: arc.clone(),
        };
        let _ = simple::<Anthropic<Arc<FakeHttpClient>>>("m", "x")
            .run_with(provider)
            .await;
        let req = &arc.recorded_requests()[0];
        assert_eq!(req.headers.get("x-api-key"), Some(&"sk-test".to_string()));
        assert_eq!(
            req.headers.get("anthropic-version"),
            Some(&DEFAULT_VERSION.to_string())
        );
    }

    #[tokio::test]
    async fn empty_content_array_is_invalid_response() {
        let http = FakeHttpClient::new();
        let body = serde_json::to_vec(&json!({
            "id": "x", "type": "message", "role": "assistant",
            "model": "m", "content": [], "stop_reason": "end_turn",
            "usage": {"input_tokens": 0, "output_tokens": 0}
        })).unwrap();
        http.expect(
            "https://api.anthropic.com/v1/messages",
            Response { status: 200, headers: HashMap::new(), body },
        );
        let exit = simple::<Anthropic<FakeHttpClient>>("m", "x")
            .run_with(Anthropic::new("k", http))
            .await;
        match exit.err() {
            Some(LlmError::InvalidResponse(msg)) => assert!(msg.contains("no text blocks")),
            other => panic!("expected InvalidResponse, got {other:?}"),
        }
    }
}
