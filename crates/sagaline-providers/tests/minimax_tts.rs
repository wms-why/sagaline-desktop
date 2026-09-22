//! Integration test: minimax native text-to-speech.
//!
//! Boots a wiremock server pointing at minimax's `/audio/speech`
//! endpoint and asserts:
//!
//! - The wire request body carries `model`, `text`, `voice_id`.
//! - The bearer auth header carries the API key.
//! - The binary audio/mpeg response is returned verbatim in
//!   `GenerationOutput::bytes` with `mime = "audio/mpeg"`.

use sagaline_providers::minimax::MinimaxTts;
use sagaline_providers::{
    Capability, ModelAdapter, ProviderRegistry, Tts as _,
};
use sagaline_store::KeyHandle;
use secrecy::ExposeSecret as _;
use serde_json::json;

#[tokio::test]
async fn minimax_tts_round_trip() {
    let server = wiremock::MockServer::start().await;

    // Fake mp3 payload — the bytes don't have to be a valid mp3;
    // we only assert the round-trip.
    let fixture: Vec<u8> = b"ID3fake-mp3-payload".to_vec();

    let mock = wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/audio/speech"))
        .and(wiremock::matchers::header(
            "authorization",
            "Bearer sk-test-fake-key",
        ))
        .and(wiremock::matchers::body_partial_json(json!({
            "model": "speech-2.8-hd",
            "text": "hello, world.",
            "voice_id": "English_expressive_narrator",
        })))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .insert_header("content-type", "audio/mpeg")
                .set_body_bytes(fixture.clone()),
        );
    server.register(mock).await;

    let key = KeyHandle::from_static_for_test("sk-test-fake-key");
    let api_key = secrecy::SecretString::new(Box::from(
        key.reveal().expose_secret().to_string().into_boxed_str(),
    ));
    let backend = MinimaxTts::new(api_key, server.uri(), "speech-2.8-hd");

    assert_eq!(backend.provider_name(), "minimax");
    assert_eq!(backend.capabilities(), &[Capability::Tts]);
    assert_eq!(backend.id(), "speech-2.8-hd");

    let out = backend
        .synthesize("hello, world.", "English_expressive_narrator")
        .await
        .expect("tts synthesize should succeed");

    assert_eq!(out.mime, "audio/mpeg");
    assert_eq!(
        out.bytes, fixture,
        "tts response bytes must be returned verbatim"
    );

    // Round-trip the registry so we know `register_tts` + `pick_tts`
    // route correctly for the production path.
    let mut registry = ProviderRegistry::new();
    registry.register_tts(backend);
    let picked = registry
        .pick_tts("minimax")
        .expect("registry lookup should succeed");
    assert_eq!(picked.provider_name(), "minimax");

    let received = server.received_requests().await.expect("received");
    assert_eq!(received.len(), 1);
    let req = &received[0];
    assert_eq!(req.method, "POST");
    assert_eq!(req.url.path(), "/audio/speech");

    let auth_header = req
        .headers
        .iter()
        .find(|(k, _)| k.as_str().eq_ignore_ascii_case("authorization"))
        .map(|(_, v)| v.to_str().unwrap_or("").to_string())
        .expect("authorization header present");
    assert_eq!(auth_header, "Bearer sk-test-fake-key");

    let body: serde_json::Value = serde_json::from_slice(&req.body).expect("json body");
    assert_eq!(body["model"], "speech-2.8-hd");
    assert_eq!(body["text"], "hello, world.");
    assert_eq!(body["voice_id"], "English_expressive_narrator");
}