//! Integration test: OpenAI-compatible chat completions.
//!
//! Boots a wiremock server, points a rig OpenAI-compatible client
//! at it, sends one completion, and asserts:
//! - The wire shape is OpenAI chat completions
//!   (`POST /v1/chat/completions`).
//! - The bearer auth header carries the API key we supplied.
//! - The request body carries the messages + model.
//! - The response parses back through rig into a usable
//!   `CompletionResponse`.

use rig_core::client::CompletionClient;
use rig_core::completion::{AssistantContent, CompletionModel, Message};
use rig_core::providers::openai;
use rig_core::providers::openai::completion::GenericCompletionModel;
use sagaline_keys::KeyHandle;
use secrecy::ExposeSecret as _;
use serde_json::json;
use std::sync::Arc;

type OpenAIChat = GenericCompletionModel<openai::OpenAICompletionsExt>;

#[tokio::test]
async fn openai_compat_chat_completions_round_trip() {
    let server = wiremock::MockServer::start().await;

    let response_body = json!({
        "id": "chatcmpl-test-001",
        "object": "chat.completion",
        "created": 1700000000u64,
        "model": "gpt-4o-mini",
        "choices": [
            {
                "index": 0,
                "message": {"role": "assistant", "content": "pong"},
                "finish_reason": "stop",
            }
        ],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    });

    // rig appends `chat/completions` to whatever base_url we give.
    // base_url here is "<mock>/v1", so the final path is
    // "/v1/chat/completions" — that's what we match.
    let mock = wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/v1/chat/completions"))
        .and(wiremock::matchers::header(
            "authorization",
            "Bearer sk-test-fake-key",
        ))
        .and(wiremock::matchers::body_partial_json(json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "ping"}],
        })))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(&response_body),
        );
    server.register(mock).await;

    let key = KeyHandle::from_static_for_test("sk-test-fake-key");
    let secret_string = key.reveal().expose_secret().to_string();
    let bearer: rig_core::client::BearerAuth = secret_string.into();
    let client = openai::Client::builder()
        .api_key(bearer)
        .base_url(format!("{}/v1", server.uri()))
        .build()
        .expect("build client")
        .completions_api();

    let model: Arc<OpenAIChat> = Arc::new(client.completion_model("gpt-4o-mini"));

    let request = model.completion_request(Message::user("ping")).build();
    let response = model
        .completion(request)
        .await
        .unwrap_or_else(|e| panic!("completion failed: {e}; mock URL = {}", server.uri()));

    let assistant_text: String = response
        .choice
        .iter()
        .filter_map(|c| match c {
            AssistantContent::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");
    assert_eq!(assistant_text, "pong");

    let received = server.received_requests().await.expect("received");
    assert_eq!(received.len(), 1, "expected one request, got {}", received.len());
    let req = &received[0];
    assert_eq!(req.method, "POST");
    assert_eq!(req.url.path(), "/v1/chat/completions");

    let auth_header = req
        .headers
        .iter()
        .find(|(k, _)| k.as_str().eq_ignore_ascii_case("authorization"))
        .map(|(_, v)| v.to_str().unwrap_or("").to_string())
        .expect("authorization header present");
    assert_eq!(auth_header, "Bearer sk-test-fake-key");

    let body: serde_json::Value = serde_json::from_slice(&req.body).expect("json body");
    assert_eq!(body["model"], "gpt-4o-mini");
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"], "ping");
}
