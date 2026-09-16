//! Integration test: MiniMax native image generation.
//!
//! Boots a wiremock server pointing at the MiniMax
//! `/image/generations` endpoint (URL appended by the backend
//! itself to the configured `base_url`), registers a minimal
//! base64-encoded image payload, and asserts:
//! - The wire request body shape (`model`, `prompt`, `n`,
//!   `response_format`).
//! - The bearer auth header carries the API key.
//! - The base64 response decodes back to the original bytes.
//! - `GenerationOutput::mime` is `"image/png"`.
//!
//! The decoded bytes don't have to be a valid PNG for this test —
//! the decode path is what we're verifying. (A real fixture PNG
//! would be one more assertion, but the codec-fidelity side of
//! the round-trip is the provider's responsibility, not ours.)

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use sagaline_keys::KeyHandle;
use sagaline_providers::minimax::MinimaxImage;
use sagaline_providers::{
    Capability, GenerationRequest, ImageGen, ModelAdapter, ProviderError,
};
use secrecy::ExposeSecret as _;
use serde_json::json;

#[tokio::test]
async fn minimax_image_round_trip() {
    let server = wiremock::MockServer::start().await;

    // 8 arbitrary bytes — the test only checks round-trip equality.
    let fixture: Vec<u8> = (0u8..8).collect();
    let b64 = B64.encode(&fixture);
    let response_body = json!({
        "data": [{ "b64_json": b64 }]
    });

    let mock = wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/image/generations"))
        .and(wiremock::matchers::header(
            "authorization",
            "Bearer sk-test-fake-key",
        ))
        .and(wiremock::matchers::body_partial_json(json!({
            "model": "image-01",
            "prompt": "a cat",
            "n": 1,
            "response_format": "b64_json",
        })))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(&response_body),
        );
    server.register(mock).await;

    let key = KeyHandle::from_static_for_test("sk-test-fake-key");
    let api_key = secrecy::SecretString::new(Box::from(
        key.reveal().expose_secret().to_string().into_boxed_str(),
    ));
    // base_url = mock server root. The backend appends
    // `/image/generations` itself.
    let backend = MinimaxImage::new(api_key, server.uri(), "image-01");

    assert_eq!(backend.provider_name(), "minimax");
    assert_eq!(backend.capabilities(), &[Capability::Image]);
    assert_eq!(backend.id(), "image-01");

    // Empty `extra` — anything in `extra` would be merged into the
    // request body, and our mock body matcher asserts only the four
    // canonical fields.
    let extra = json!({});
    let req = GenerationRequest {
        prompt: "a cat",
        negative_prompt: None,
        reference_images: &[],
        seed: None,
        aspect_ratio: None,
        extra: &extra,
    };
    let out = backend
        .generate(req)
        .await
        .expect("image generation should succeed");

    assert_eq!(out.mime, "image/png");
    assert_eq!(out.bytes, fixture, "decoded bytes round-trip");

    let received = server.received_requests().await.expect("received");
    assert_eq!(received.len(), 1);
    let req = &received[0];
    assert_eq!(req.method, "POST");
    assert_eq!(req.url.path(), "/image/generations");

    let auth_header = req
        .headers
        .iter()
        .find(|(k, _)| k.as_str().eq_ignore_ascii_case("authorization"))
        .map(|(_, v)| v.to_str().unwrap_or("").to_string())
        .expect("authorization header present");
    assert_eq!(auth_header, "Bearer sk-test-fake-key");

    let body: serde_json::Value = serde_json::from_slice(&req.body).expect("json body");
    assert_eq!(body["model"], "image-01");
    assert_eq!(body["prompt"], "a cat");
    assert_eq!(body["n"], 1);
    assert_eq!(body["response_format"], "b64_json");
}

#[allow(dead_code)]
fn _phantom_error(e: ProviderError) -> ProviderError {
    e
}
