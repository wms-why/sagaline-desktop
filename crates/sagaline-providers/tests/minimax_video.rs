//! Integration test: minimax image-to-video.
//!
//! Two-phase wiremock test:
//!
//! 1. `submit` → POST `/video/generations` with the first frame
//!    base64-encoded plus `model`, `prompt`, `duration_seconds`.
//!    Asserts the wire body + bearer auth and that the returned
//!    [`TaskHandle`] carries the provider's `task_id`.
//! 2. `poll` → GET `/tasks/{task_id}`. Two consecutive polls; the
//!    first returns `running`, the second `succeeded` with a
//!    `video_url`. Asserts the returned [`VideoStatus`] variant.

use sagaline_providers::minimax::MinimaxVideo;
use sagaline_providers::{
    Capability, ImageToVideo as _, ModelAdapter, ProviderRegistry, VideoRequest, VideoStatus,
};
use sagaline_store::KeyHandle;
use secrecy::ExposeSecret as _;

#[tokio::test]
async fn minimax_video_submit_then_poll() {
    let server = wiremock::MockServer::start().await;

    // Submit mock — the first frame is read from disk and base64-
    // encoded; the matcher only asserts the structured fields.
    let submit_mock = wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/video/generations"))
        .and(wiremock::matchers::header(
            "authorization",
            "Bearer sk-test-fake-key",
        ))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(serde_json::json!({ "task_id": "task-42" })),
        );
    server.register(submit_mock).await;

    // Polls mock — first call returns running, second returns
    // succeeded with a URL. We scope each mock to one request so
    // wiremock doesn't keep matching the first one forever.
    let poll_running = wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/tasks/task-42"))
        .and(wiremock::matchers::header(
            "authorization",
            "Bearer sk-test-fake-key",
        ))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(serde_json::json!({ "status": "running" })),
        )
        .up_to_n_times(1);
    server.register(poll_running).await;

    let poll_succeeded = wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/tasks/task-42"))
        .and(wiremock::matchers::header(
            "authorization",
            "Bearer sk-test-fake-key",
        ))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(serde_json::json!({
                    "status": "succeeded",
                    "video_url": "https://example.invalid/out.mp4",
                })),
        );
    server.register(poll_succeeded).await;

    // Build the backend.
    let key = KeyHandle::from_static_for_test("sk-test-fake-key");
    let api_key = secrecy::SecretString::new(Box::from(
        key.reveal().expose_secret().to_string().into_boxed_str(),
    ));
    let backend = MinimaxVideo::new(api_key, server.uri(), "video-01");

    assert_eq!(backend.provider_name(), "minimax");
    assert_eq!(backend.capabilities(), &[Capability::ImageToVideo]);
    assert_eq!(backend.id(), "video-01");

    // Stage a tiny first-frame file on disk (the backend reads it
    // synchronously). Content is arbitrary.
    let tmp = tempfile::tempdir().expect("tempdir");
    let frame_path = tmp.path().join("first_frame.png");
    std::fs::write(&frame_path, b"\x89PNG_FAKE").expect("write frame");

    let extra = serde_json::json!({});
    let req = VideoRequest {
        first_frame: &frame_path,
        prompt: "a slow zoom-out revealing a starlit harbor",
        duration_seconds: Some(5),
        extra: &extra,
    };

    let handle = backend.submit(req).await.expect("submit should succeed");
    assert_eq!(handle.provider, "minimax");
    assert_eq!(handle.provider_task_id, "task-42");
    assert_eq!(handle.model_id, "video-01");

    // First poll: still running.
    let status = backend.poll(&handle).await.expect("first poll should succeed");
    assert!(
        matches!(status, VideoStatus::Running),
        "first poll should be Running, got: {status:?}"
    );

    // Second poll: ready.
    let status = backend.poll(&handle).await.expect("second poll should succeed");
    assert!(
        matches!(status, VideoStatus::Ready { .. }),
        "second poll should be Ready, got: {status:?}"
    );
    if let VideoStatus::Ready { url } = status {
        assert_eq!(url, "https://example.invalid/out.mp4");
    }

    // Submit body assertions.
    let received = server.received_requests().await.expect("received");
    assert_eq!(received.len(), 3, "1 submit + 2 polls = 3 requests");

    let submit_req = &received[0];
    assert_eq!(submit_req.method, "POST");
    assert_eq!(submit_req.url.path(), "/video/generations");

    let submit_body: serde_json::Value =
        serde_json::from_slice(&submit_req.body).expect("submit body json");
    assert_eq!(submit_body["model"], "video-01");
    assert_eq!(
        submit_body["prompt"],
        "a slow zoom-out revealing a starlit harbor"
    );
    assert_eq!(submit_body["duration_seconds"], 5);
    // First frame is base64-encoded on the wire; the matcher only
    // asserts the structured fields above, so here we just check
    // the field exists and is non-empty.
    assert!(
        submit_body["first_frame_b64"].as_str().is_some_and(|s| !s.is_empty()),
        "first_frame_b64 must be a non-empty string, got: {:?}",
        submit_body["first_frame_b64"]
    );

    // Round-trip via the registry too.
    let mut registry = ProviderRegistry::new();
    registry.register_image_to_video(backend);
    let picked = registry
        .pick_image_to_video("minimax")
        .expect("registry lookup should succeed");
    assert_eq!(picked.provider_name(), "minimax");
}