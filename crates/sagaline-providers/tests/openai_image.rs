//! Integration test: OpenAI gpt-image-1 image generation.
//!
//! Boots a wiremock server pointing at the OpenAI
//! `/images/generations` endpoint (rig prepends
//! `<base_url>/` to the path itself), registers a minimal
//! base64-encoded image payload, and asserts:
//!
//! - The wire request body carries `model`, `prompt`, and
//!   `size`.
//! - The bearer auth header carries the API key.
//! - The base64 response decodes back to the original bytes.
//! - `GenerationOutput::mime` is `"image/png"`.
//!
//! The decoded bytes don't have to be a valid PNG for this
//! test — the decode path is what we're verifying. (A real
//! fixture PNG would be one more assertion, but the
//! codec-fidelity side of the round-trip is the provider's
//! responsibility, not ours.) Mirrors
//! `tests/minimax_image.rs` for the native backend.
//!
//! ## What rig sends
//!
//! Rig's `OpenAIResponsesExt::image_generation_request_body`
//! emits `{ "model": ..., "prompt": ..., "size": "WxH" }` plus
//! anything the caller passed in `additional_params`. We
//! assert the first three keys land; we don't pin the byte-
//! exact body because rig may add or reorder fields between
//! versions.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use sagaline_providers::openai::OpenAiImage;
use sagaline_providers::{Capability, GenerationRequest, ImageGen, ModelAdapter};
use secrecy::SecretString;
use serde_json::json;

#[tokio::test]
async fn openai_image_round_trip() {
    let server = wiremock::MockServer::start().await;

    // 8 arbitrary bytes — the test only checks round-trip equality.
    let fixture: Vec<u8> = (0u8..8).collect();
    let b64 = B64.encode(&fixture);
    let response_body = json!({
        "created": 0,
        "data": [{ "b64_json": b64 }]
    });

    // Match POST + path + bearer header. Body-shape assertions
    // live in the `received_requests()` check below; we use
    // `body_partial_json` so rig can add extra fields without
    // breaking the test.
    let mock = wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/images/generations"))
        .and(wiremock::matchers::header(
            "authorization",
            "Bearer sk-test-fake-key",
        ))
        .and(wiremock::matchers::body_partial_json(json!({
            "model": "gpt-image-1",
            "prompt": "a cat",
            "size": "1024x1024",
        })))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(&response_body),
        );
    server.register(mock).await;

    let api_key = SecretString::new(Box::from(
        "sk-test-fake-key".to_string().into_boxed_str(),
    ));
    // base_url = mock server root. rig appends `/images/generations`.
    let backend = OpenAiImage::new(api_key, server.uri(), "gpt-image-1");

    assert_eq!(backend.provider_name(), "openai");
    assert_eq!(backend.capabilities(), &[Capability::Image]);
    assert_eq!(backend.id(), "gpt-image-1");

    // Empty `extra` — anything in `extra` would be merged into
    // the request body via rig's `additional_params`, and our
    // mock body matcher asserts only the three canonical fields.
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
    assert_eq!(req.url.path(), "/images/generations");

    let auth_header = req
        .headers
        .iter()
        .find(|(k, _)| k.as_str().eq_ignore_ascii_case("authorization"))
        .map(|(_, v)| v.to_str().unwrap_or("").to_string())
        .expect("authorization header present");
    assert_eq!(auth_header, "Bearer sk-test-fake-key");

    let body: serde_json::Value = serde_json::from_slice(&req.body).expect("json body");
    assert_eq!(body["model"], "gpt-image-1");
    assert_eq!(body["prompt"], "a cat");
    assert_eq!(body["size"], "1024x1024");
}

/// `extra.model_id` must override the construction-time model
/// without rebuilding the backend — that's the path the
/// `generate_image` tool uses.
#[tokio::test]
async fn extra_model_id_overrides_construction_default() {
    let server = wiremock::MockServer::start().await;

    let fixture: Vec<u8> = vec![0xDE, 0xAD, 0xBE, 0xEF];
    let b64 = B64.encode(&fixture);
    let response_body = json!({
        "created": 0,
        "data": [{ "b64_json": b64 }]
    });

    let mock = wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/images/generations"))
        .and(wiremock::matchers::body_partial_json(json!({
            "model": "gpt-image-1.5",
        })))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(&response_body),
        );
    server.register(mock).await;

    let api_key = SecretString::new(Box::from(
        "sk-test-fake-key".to_string().into_boxed_str(),
    ));
    // Construction-time model is "gpt-image-1", but the call
    // below overrides it to "gpt-image-1.5".
    let backend = OpenAiImage::new(api_key, server.uri(), "gpt-image-1");

    let extra = json!({ "model_id": "gpt-image-1.5" });
    let req = GenerationRequest {
        prompt: "anything",
        negative_prompt: None,
        reference_images: &[],
        seed: None,
        aspect_ratio: None,
        extra: &extra,
    };
    let out = backend.generate(req).await.expect("generate");

    assert_eq!(out.mime, "image/png");
    assert_eq!(out.bytes, fixture);

    let received = server.received_requests().await.expect("received");
    assert_eq!(received.len(), 1);
    let body: serde_json::Value =
        serde_json::from_slice(&received[0].body).expect("json body");
    assert_eq!(body["model"], "gpt-image-1.5");
    // `api_key` from `extra` must NOT land in the wire body.
    assert!(body.get("api_key").is_none());
}