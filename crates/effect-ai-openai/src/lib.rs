//! OpenAI provider for [`effect_ai`].
//!
//! Implements [`effect_ai::LlmProvider`] by POSTing to
//! `/v1/chat/completions` over a user-supplied [`effect_http::HttpClient`].
//! The HTTP layer is pluggable so you can fake it in tests without
//! standing up a network call.
//!
//! ```no_run
//! use effect_ai::*;
//! use effect_ai_openai::OpenAi;
//! use effect_http::LiveHttpClient;
//!
//! # #[tokio::main] async fn main() {
//! let provider = OpenAi::new(
//!     std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY"),
//!     LiveHttpClient::new(),
//! );
//!
//! let answer = simple::<OpenAi<LiveHttpClient>>("gpt-4o-mini", "What's 2+2?")
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

pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

pub struct OpenAi<H: HttpClient> {
    pub base_url: String,
    pub api_key: String,
    pub http: H,
}

impl<H: HttpClient> OpenAi<H> {
    /// Construct with the default base URL (`https://api.openai.com/v1`).
    pub fn new(api_key: impl Into<String>, http: H) -> Self {
        OpenAi {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: api_key.into(),
            http,
        }
    }

    /// Override the base URL — useful for OpenAI-compatible providers
    /// (Azure OpenAI deployments, local LLM servers, etc.).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }
}

impl<H: HttpClient> LlmProvider for OpenAi<H> {
    fn complete(
        &self,
        request: ChatRequest,
    ) -> AsyncResult<Result<ChatResponse, LlmError>> {
        let url = format!("{}/chat/completions", self.base_url);
        let auth = format!("Bearer {}", self.api_key);

        // Build OpenAI request JSON.
        let mut body = json!({
            "model": request.model,
            "messages": request.messages.iter().map(message_to_json).collect::<Vec<_>>(),
        });
        if let Some(t) = request.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(m) = request.max_tokens {
            body["max_tokens"] = json!(m);
        }

        let body_bytes = match serde_json::to_vec(&body) {
            Ok(v) => v,
            Err(e) => {
                return Box::pin(async move {
                    Err(LlmError::InvalidRequest(format!("encode failed: {e}")))
                });
            }
        };

        let http_req = HttpRequest {
            method: Method::Post,
            url,
            headers: [
                ("Authorization".to_string(), auth),
                ("Content-Type".to_string(), "application/json".to_string()),
            ]
            .into_iter()
            .collect(),
            body: Some(body_bytes),
        };

        // Borrow http via the trait through &self; we need to bridge
        // to a 'static future. The Live impl clones its inner client.
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

fn message_to_json(m: &ChatMessage) -> Value {
    json!({
        "role": match m.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        },
        "content": m.content,
    })
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

    // Pull choices[0].message.content.
    let content = value
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| {
            LlmError::InvalidResponse(
                "missing choices[0].message.content".into(),
            )
        })?
        .to_string();

    let model = value
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("unknown")
        .to_string();

    let usage = value.get("usage").map(|u| TokenUsage {
        input: u.get("prompt_tokens").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
        output: u
            .get("completion_tokens")
            .and_then(|n| n.as_u64())
            .unwrap_or(0) as u32,
    });

    let finish_reason = value
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("finish_reason"))
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

    fn openai_response(content: &str, model: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "model": model,
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": content},
                "finish_reason": "stop",
            }],
            "usage": {
                "prompt_tokens": 9,
                "completion_tokens": 12,
                "total_tokens": 21,
            },
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn happy_path_returns_assistant_content() {
        let http = FakeHttpClient::new();
        http.expect(
            "https://api.openai.com/v1/chat/completions",
            Response {
                status: 200,
                headers: HashMap::new(),
                body: openai_response("Paris", "gpt-4o-mini"),
            },
        );
        let provider = OpenAi::new("test-key", http);

        let exit = simple::<OpenAi<FakeHttpClient>>("gpt-4o-mini", "Capital of France?")
            .run_with(provider)
            .await;
        assert_eq!(exit.ok(), Some("Paris".to_string()));
    }

    #[tokio::test]
    async fn full_chat_request_returns_response_with_usage() {
        let http = FakeHttpClient::new();
        http.expect(
            "https://api.openai.com/v1/chat/completions",
            Response {
                status: 200,
                headers: HashMap::new(),
                body: openai_response("Hi!", "gpt-4o-mini-2024-07-18"),
            },
        );
        let provider = OpenAi::new("test-key", http);
        let request = ChatRequest::new("gpt-4o-mini", vec![ChatMessage::user("Hi")])
            .temperature(0.5)
            .max_tokens(50);
        let exit = complete::<OpenAi<FakeHttpClient>>(request)
            .run_with(provider)
            .await;
        let resp = exit.ok().unwrap();
        assert_eq!(resp.content, "Hi!");
        assert_eq!(resp.model, "gpt-4o-mini-2024-07-18");
        let usage = resp.usage.unwrap();
        assert_eq!(usage.input, 9);
        assert_eq!(usage.output, 12);
        assert_eq!(usage.total(), 21);
        assert_eq!(resp.finish_reason.as_deref(), Some("stop"));
    }

    #[tokio::test]
    async fn auth_error_becomes_invalid_request() {
        let http = FakeHttpClient::new();
        http.expect(
            "https://api.openai.com/v1/chat/completions",
            Response {
                status: 401,
                headers: HashMap::new(),
                body: b"".to_vec(),
            },
        );
        let exit = simple::<OpenAi<FakeHttpClient>>("gpt-4o-mini", "hi")
            .run_with(OpenAi::new("bad", http))
            .await;
        match exit.err() {
            Some(LlmError::InvalidRequest(msg)) => assert!(msg.contains("401")),
            other => panic!("expected InvalidRequest, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rate_limit_surfaces_retry_after() {
        let http = FakeHttpClient::new();
        let mut headers = HashMap::new();
        headers.insert("retry-after".to_string(), "30".to_string());
        http.expect(
            "https://api.openai.com/v1/chat/completions",
            Response {
                status: 429,
                headers,
                body: b"".to_vec(),
            },
        );
        let exit = simple::<OpenAi<FakeHttpClient>>("gpt-4o-mini", "hi")
            .run_with(OpenAi::new("k", http))
            .await;
        match exit.err() {
            Some(LlmError::RateLimit { retry_after_seconds }) => {
                assert_eq!(retry_after_seconds, Some(30));
            }
            other => panic!("expected RateLimit, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn server_error_surfaces_as_provider_error_with_body() {
        let http = FakeHttpClient::new();
        http.expect(
            "https://api.openai.com/v1/chat/completions",
            Response {
                status: 500,
                headers: HashMap::new(),
                body: b"upstream down".to_vec(),
            },
        );
        let exit = simple::<OpenAi<FakeHttpClient>>("gpt-4o-mini", "hi")
            .run_with(OpenAi::new("k", http))
            .await;
        match exit.err() {
            Some(LlmError::Provider(msg)) => {
                assert!(msg.contains("500"));
                assert!(msg.contains("upstream down"));
            }
            other => panic!("expected Provider, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn malformed_json_surfaces_as_invalid_response() {
        let http = FakeHttpClient::new();
        http.expect(
            "https://api.openai.com/v1/chat/completions",
            Response {
                status: 200,
                headers: HashMap::new(),
                body: b"not json".to_vec(),
            },
        );
        let exit = simple::<OpenAi<FakeHttpClient>>("gpt-4o-mini", "hi")
            .run_with(OpenAi::new("k", http))
            .await;
        assert!(matches!(exit.err(), Some(LlmError::InvalidResponse(_))));
    }

    #[tokio::test]
    async fn missing_choices_surfaces_as_invalid_response() {
        let http = FakeHttpClient::new();
        http.expect(
            "https://api.openai.com/v1/chat/completions",
            Response {
                status: 200,
                headers: HashMap::new(),
                body: br#"{"id":"x","model":"y","choices":[]}"#.to_vec(),
            },
        );
        let exit = simple::<OpenAi<FakeHttpClient>>("gpt-4o-mini", "hi")
            .run_with(OpenAi::new("k", http))
            .await;
        match exit.err() {
            Some(LlmError::InvalidResponse(msg)) => assert!(msg.contains("choices")),
            other => panic!("expected InvalidResponse, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn with_base_url_overrides_endpoint() {
        let http = FakeHttpClient::new();
        http.expect(
            "http://localhost:8000/chat/completions",
            Response {
                status: 200,
                headers: HashMap::new(),
                body: openai_response("local", "llama"),
            },
        );
        let provider =
            OpenAi::new("anything", http).with_base_url("http://localhost:8000");

        let exit = simple::<OpenAi<FakeHttpClient>>("llama", "hi")
            .run_with(provider)
            .await;
        assert_eq!(exit.ok(), Some("local".to_string()));
    }

    #[tokio::test]
    async fn messages_serialize_with_correct_roles() {
        use std::sync::Arc;
        let http = FakeHttpClient::new();
        http.expect(
            "https://api.openai.com/v1/chat/completions",
            Response {
                status: 200,
                headers: HashMap::new(),
                body: openai_response("ok", "m"),
            },
        );
        let arc = Arc::new(http);
        let provider = OpenAi {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: "k".into(),
            http: arc.clone(),
        };
        // The struct holds Arc<FakeHttpClient>; FakeHttpClient is HttpClient so Arc<H>: HttpClient too.
        // Actually Arc<H> isn't automatically HttpClient — let me just use Arc::clone for recording.
        let _ = chat::<OpenAi<Arc<FakeHttpClient>>>(
            "m",
            vec![
                ChatMessage::system("system msg"),
                ChatMessage::user("user msg"),
                ChatMessage::assistant("assistant msg"),
            ],
        )
        .run_with(provider)
        .await;
        let recorded = arc.recorded_requests();
        assert_eq!(recorded.len(), 1);
        let body_value: Value =
            serde_json::from_slice(recorded[0].body.as_ref().unwrap()).unwrap();
        let messages = body_value["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[2]["role"], "assistant");
        assert_eq!(messages[0]["content"], "system msg");
    }
}
