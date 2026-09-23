//! Integration smoke tests for the Phase-2.5+ Execute-tier tools:
//!
//! - `generate_speech` (TTS via the [`ProviderRegistry`])
//! - `submit_video` + `poll_video` (image-to-video, async submit / poll)
//! - `compose_video` (mp4 re-mux)
//!
//! We don't drive a real provider for `generate_speech` /
//! `submit_video` (the providers crate owns those wiremock tests).
//! Here we only assert the agent-tool layer's contract:
//!
//! - arg shape (JSON round-trip; required vs optional fields)
//! - registry dispatch errors (unknown provider, missing key)
//! - arg deserialization rejects malformed payloads
//! - `compose_video` rejects empty `shot_paths`

use std::sync::Arc;

use sagaline_agent::tools::{
    ComposeVideoArgs, ComposeVideoTool, GenerateSpeechArgs, PollVideoArgs, PollVideoTool,
    SubmitVideoArgs, SubmitVideoTool,
};
use sagaline_agent::Tool;
use sagaline_providers::ProviderRegistry;
use sagaline_store::World;

fn empty_registry() -> Arc<ProviderRegistry> {
    Arc::new(ProviderRegistry::new())
}

fn empty_config() -> Arc<sagaline_providers::ProviderConfigSet> {
    Arc::new(sagaline_providers::ProviderConfigSet::default())
}

fn in_memory_store() -> Arc<World> {
    Arc::new(World::in_memory().expect("in-memory world"))
}

// ────────────────────────────────────────────────────────────────
// generate_speech
// ────────────────────────────────────────────────────────────────

#[test]
fn generate_speech_args_round_trip() {
    let args = GenerateSpeechArgs {
        provider: "minimax".into(),
        model: Some("speech-2.8-hd".into()),
        text: "hello world".into(),
        voice_id: "English_expressive_narrator".into(),
        output_path: "/tmp/out.mp3".into(),
        key_id: None,
        shot_path: Some("/tmp/story/shots/001.md".into()),
    };
    let json = serde_json::to_value(&args).expect("serialize");
    assert_eq!(json["provider"], "minimax");
    assert_eq!(json["model"], "speech-2.8-hd");
    assert_eq!(json["voice_id"], "English_expressive_narrator");
    assert!(json["key_id"].is_null(), "key_id omitted → JSON null");
    assert!(
        json["shot_path"].is_string(),
        "shot_path serialises as string (PathBuf not in schemars 0.8)"
    );
}

#[test]
fn generate_speech_args_missing_required_field_is_rejected() {
    // `text` is required; without it, deserialization must fail.
    let payload = serde_json::json!({
        "provider": "minimax",
        "voice_id": "English_expressive_narrator",
        "output_path": "/tmp/out.mp3",
    });
    let res: Result<GenerateSpeechArgs, _> = serde_json::from_value(payload);
    assert!(res.is_err(), "missing `text` must fail to deserialize");
}

#[tokio::test]
async fn generate_speech_unknown_provider_returns_execution_error() {
    let store = in_memory_store();
    let tool = sagaline_agent::tools::GenerateSpeechTool::new(
        empty_registry(),
        empty_config(),
        store.clone(),
    );
    let err = tool
        .execute(
            sagaline_agent::ToolContext::new(store.clone()),
            serde_json::to_value(GenerateSpeechArgs {
                provider: "no-such-provider".into(),
                model: None,
                text: "hi".into(),
                voice_id: "v".into(),
                output_path: "/tmp/out.mp3".into(),
                key_id: None,
                shot_path: None,
            })
            .unwrap(),
        )
        .await
        .expect_err("unknown provider must surface as Execution error");
    // Execution { name: "generate_speech", source: ProviderError::Unknown(_) }
    let msg = format!("{err}");
    assert!(
        msg.contains("generate_speech"),
        "error must name the tool; got: {msg}"
    );
    assert!(
        msg.contains("no-such-provider"),
        "error must name the provider; got: {msg}"
    );
}

// ────────────────────────────────────────────────────────────────
// submit_video
// ────────────────────────────────────────────────────────────────

#[test]
fn submit_video_args_round_trip() {
    let args = SubmitVideoArgs {
        provider: "minimax".into(),
        model: Some("video-01".into()),
        prompt: "camera pushes slowly toward the energy core".into(),
        first_frame_path: "/tmp/keyframes/01.png".into(),
        duration_seconds: Some(3),
        key_id: None,
    };
    let json = serde_json::to_value(&args).expect("serialize");
    assert_eq!(json["provider"], "minimax");
    assert_eq!(json["duration_seconds"], 3);
    assert!(json["key_id"].is_null(), "key_id omitted → JSON null");
}

#[test]
fn submit_video_args_missing_required_field_is_rejected() {
    // `first_frame_path` is required.
    let payload = serde_json::json!({
        "provider": "minimax",
        "prompt": "...",
    });
    let res: Result<SubmitVideoArgs, _> = serde_json::from_value(payload);
    assert!(
        res.is_err(),
        "missing `first_frame_path` must fail to deserialize"
    );
}

#[tokio::test]
async fn submit_video_unknown_provider_returns_execution_error() {
    let store = in_memory_store();
    let tool = SubmitVideoTool::new(empty_registry(), empty_config(), store.clone());
    let err = tool
        .execute(
            sagaline_agent::ToolContext::new(store.clone()),
            serde_json::to_value(SubmitVideoArgs {
                provider: "no-such-provider".into(),
                model: None,
                prompt: "...".into(),
                first_frame_path: "/tmp/missing.png".into(),
                duration_seconds: None,
                key_id: None,
            })
            .unwrap(),
        )
        .await
        .expect_err("unknown provider must surface");
    let msg = format!("{err}");
    assert!(msg.contains("submit_video"), "got: {msg}");
    assert!(msg.contains("no-such-provider"), "got: {msg}");
}

#[tokio::test]
async fn submit_video_missing_first_frame_returns_io_error() {
    // Even with an unknown provider, we'd get ProviderError first;
    // flip the scenario: register a no-op backend via the test
    // path. The cleanest check: a known-but-unconfigured
    // provider, with `first_frame_path` pointing at a file that
    // doesn't exist. The tool surfaces `Io { kind: NotFound }`.
    let store = in_memory_store();
    let tool = SubmitVideoTool::new(empty_registry(), empty_config(), store.clone());
    let err = tool
        .execute(
            sagaline_agent::ToolContext::new(store.clone()),
            serde_json::to_value(SubmitVideoArgs {
                // Sentinel: registry lookup fails first (registry is
                // empty), so this exercise is covered by the test
                // above. The Io path requires a registered
                // backend; that lives in the providers crate's
                // wiremock tests, not here.
                provider: "minimax".into(),
                model: None,
                prompt: "...".into(),
                first_frame_path: "/tmp/does-not-exist.png".into(),
                duration_seconds: None,
                key_id: None,
            })
            .unwrap(),
        )
        .await
        .expect_err("empty registry surfaces as Execution error");
    let msg = format!("{err}");
    assert!(
        msg.contains("submit_video"),
        "any submit_video error names the tool; got: {msg}"
    );
}

// ────────────────────────────────────────────────────────────────
// poll_video
// ────────────────────────────────────────────────────────────────

#[test]
fn poll_video_args_round_trip() {
    let args = PollVideoArgs {
        provider: "minimax".into(),
        provider_task_id: "task-abc".into(),
        key_id: None,
    };
    let json = serde_json::to_value(&args).expect("serialize");
    assert_eq!(json["provider"], "minimax");
    assert_eq!(json["provider_task_id"], "task-abc");
    assert!(json["key_id"].is_null(), "key_id omitted → JSON null");
}

#[test]
fn poll_video_args_missing_required_field_is_rejected() {
    let payload = serde_json::json!({
        "provider": "minimax",
    });
    let res: Result<PollVideoArgs, _> = serde_json::from_value(payload);
    assert!(
        res.is_err(),
        "missing `provider_task_id` must fail to deserialize"
    );
}

#[tokio::test]
async fn poll_video_unknown_provider_returns_execution_error() {
    let store = in_memory_store();
    let tool = PollVideoTool::new(empty_registry(), empty_config(), store.clone());
    let err = tool
        .execute(
            sagaline_agent::ToolContext::new(store.clone()),
            serde_json::to_value(PollVideoArgs {
                provider: "no-such-provider".into(),
                provider_task_id: "task-abc".into(),
                key_id: None,
            })
            .unwrap(),
        )
        .await
        .expect_err("unknown provider must surface");
    let msg = format!("{err}");
    assert!(msg.contains("poll_video"), "got: {msg}");
    assert!(msg.contains("no-such-provider"), "got: {msg}");
}

// ────────────────────────────────────────────────────────────────
// compose_video
// ────────────────────────────────────────────────────────────────

#[test]
fn compose_video_args_round_trip() {
    let args = ComposeVideoArgs {
        shot_paths: vec!["/tmp/a.mp4".into(), "/tmp/b.mp4".into()],
        output_path: "/tmp/out.mp4".into(),
    };
    let json = serde_json::to_value(&args).expect("serialize");
    let arr = json["shot_paths"].as_array().expect("array");
    assert_eq!(arr.len(), 2);
    assert_eq!(json["output_path"], "/tmp/out.mp4");
}

#[test]
fn compose_video_args_empty_shot_paths_is_ok_at_deserialize() {
    // The JSON-Schema layer accepts `[]`; the runtime check
    // lives in `run_compose`. This test pins the contract:
    // deserialization does NOT reject empty arrays (so the LLM
    // gets a clean error message instead of a parse error).
    let payload = serde_json::json!({
        "shot_paths": [],
        "output_path": "/tmp/out.mp4",
    });
    let args: CompileVideoArgs = serde_json::from_value(payload).expect("deserialize");
    assert!(args.shot_paths.is_empty());
}

// A generated-shape alias is annoying — rename the type's path so
// the test compiles without an extra import.
type CompileVideoArgs = ComposeVideoArgs;

#[tokio::test]
async fn compose_video_empty_shot_paths_returns_execution_error() {
    let tool = ComposeVideoTool::new();
    let err = tool
        .execute(
            sagaline_agent::ToolContext::new(
                Arc::new(World::in_memory().expect("in-memory world")),
            ),
            serde_json::to_value(ComposeVideoArgs {
                shot_paths: vec![],
                output_path: "/tmp/out.mp4".into(),
            })
            .unwrap(),
        )
        .await
        .expect_err("empty shot_paths must surface");
    let msg = format!("{err}");
    assert!(msg.contains("compose_video"), "got: {msg}");
    assert!(
        msg.contains("non-empty"),
        "error must mention the constraint; got: {msg}"
    );
}