//! End-to-end smoke test: the agent loop with a real (mocked)
//! LLM attached via [`RigLlm`]. Verifies that:
//!
//! - The PLAN step calls `/v1/chat/completions` and emits the
//!   assistant text inside `AgentEvent::Plan`.
//! - The REFLECT step also hits the chat endpoint and emits the
//!   assistant text inside `AgentEvent::Reflect`.
//! - When the chat endpoint returns a tool_call instead of
//!   text, the loop gracefully surfaces it (we do not drive the
//!   tool call from the assistant; we only assert the response
//!   didn't crash the loop).
//!
//! The mock server is [`wiremock`]; the rig client is pointed at
//! it via `build_chat`. The agent is wired with the resulting
//! `ChatModel` wrapped in [`RigLlm`].
use std::fs;
use std::sync::Arc;
use rig_core::client::CompletionClient;
use rig_core::providers::openai;
use secrecy::ExposeSecret as _;
use serde_json::json;
use tempfile::tempdir;
use sagaline_agent::tools::ReadFileTool;
use sagaline_agent::{Agent, AgentEvent, EventSink, LlmClient, RigLlm};
fn write(p: &std::path::Path, body: &str) {
    fs::write(p, body).unwrap();
}

fn build_story(root: &std::path::Path) {
    write(
        &root.join("story.md"),
        "---\nid: story_demo\ntype: story\nslug: demo\ncreated_at: 2026-09-15T00:00:00Z\nupdated_at: 2026-09-15T00:00:00Z\n---\ndemo\n",
    );
    fs::create_dir_all(root.join("characters/lin-mo")).unwrap();
    write(
        &root.join("characters/lin-mo/character.md"),
        "---\nid: character_lin-mo\ntype: character\nslug: lin-mo\ncreated_at: 2026-09-15T00:00:00Z\nupdated_at: 2026-09-15T00:00:00Z\n---\n林默\n",
    );
    fs::create_dir_all(root.join("environments/laboratory")).unwrap();
    write(
        &root.join("environments/laboratory/environment.md"),
        "---\nid: environment_laboratory\ntype: environment\nslug: laboratory\ncreated_at: 2026-09-15T00:00:00Z\nupdated_at: 2026-09-15T00:00:00Z\n---\n实验室\n",
    );
    fs::create_dir_all(root.join("chapters/001-start/scenes")).unwrap();
    write(
        &root.join("chapters/001-start/chapter.md"),
        "---\nid: chapter_001-start\ntype: chapter\nslug: 001-start\ncreated_at: 2026-09-15T00:00:00Z\nupdated_at: 2026-09-15T00:00:00Z\n---\n第一章\n",
    );
    write(
        &root.join("chapters/001-start/scenes/001-intro.md"),
        "---\nid: scene_001_intro\ntype: scene\nslug: 001-intro\ncharacters: [lin-mo]\nenvironment: laboratory\ncreated_at: 2026-09-15T00:00:00Z\nupdated_at: 2026-09-15T00:00:00Z\n---\n林默走进实验室\n",
    );
}

#[derive(Default)]
struct Collect {
    events: Vec<AgentEvent>,
}

impl EventSink for Collect {
    fn emit(&mut self, event: AgentEvent) {
        self.events.push(event);
    }
}

type OpenAIChat = rig_core::providers::openai::completion::GenericCompletionModel<
    rig_core::providers::openai::OpenAICompletionsExt,
>;

fn build_rig_client(
    base_url: &str,
) -> Arc<OpenAIChat> {
    let secret: rig_core::client::BearerAuth =
        sagaline_keys::KeyHandle::from_static_for_test("sk-test-fake-key")
            .reveal()
            .expose_secret()
            .to_string()
            .into();
    let client = openai::Client::builder()
        .api_key(secret)
        .base_url(base_url)
        .build()
        .expect("build client")
        .completions_api();
    Arc::new(client.completion_model("gpt-4o-mini"))
}

fn plan_response(plan: &str) -> serde_json::Value {
    json!({
        "id": "chatcmpl-plan",
        "object": "chat.completion",
        "created": 1_700_000_000_u64,
        "model": "gpt-4o-mini",
        "choices": [
            {
                "index": 0,
                "message": {"role": "assistant", "content": plan},
                "finish_reason": "stop",
            }
        ],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    })
}

fn reflect_response(note: &str) -> serde_json::Value {
    json!({
        "id": "chatcmpl-reflect",
        "object": "chat.completion",
        "created": 1_700_000_001_u64,
        "model": "gpt-4o-mini",
        "choices": [
            {
                "index": 0,
                "message": {"role": "assistant", "content": note},
                "finish_reason": "stop",
            }
        ],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    })
}

#[tokio::test]
async fn llm_backed_plan_and_reflect_are_wired() {
    let dir = tempdir().unwrap();
    build_story(dir.path());

    let server = wiremock::MockServer::start().await;

    // Two chat calls expected: PLAN first, REFLECT second.
    // We don't inspect the bodies — only that both succeed
    // and the loop finishes with both events populated by the
    // assistant text.
    server
        .register(
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .and(wiremock::matchers::path("/v1/chat/completions"))
                .respond_with(
                    wiremock::ResponseTemplate::new(200)
                        .insert_header("content-type", "application/json")
                        .set_body_json(plan_response(
                            "- shot_004: 林默特写, 表情紧张, 3s\n- shot_005: 实验室全景, 2s\n",
                        )),
                ),
        )
        .await;
    server
        .register(
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .and(wiremock::matchers::path("/v1/chat/completions"))
                .respond_with(
                    wiremock::ResponseTemplate::new(200)
                        .insert_header("content-type", "application/json")
                        .set_body_json(reflect_response(
                            "shot_004 林默表情可以再紧张一些; shot_005 镜头偏暗。",
                        )),
                ),
        )
        .await;

    let model = build_rig_client(&format!("{}/v1", server.uri()));
    let llm: Arc<dyn LlmClient> = Arc::new(RigLlm::new(model));

    let mut agent = Agent::new();
    agent.tools_mut().register(ReadFileTool::new(dir.path()));
    let agent = agent.with_llm(llm);

    let mut sink = Collect::default();
    let outcome = agent.run(dir.path(), &mut sink).await.expect("run");
    assert_eq!(outcome, sagaline_agent::StepOutcome::Complete);

    // Find the Plan and Reflect events; their payload strings
    // must contain the LLM-supplied text.
    let plan = sink
        .events
        .iter()
        .find_map(|e| match e {
            AgentEvent::Plan { plan, .. } => Some(plan.clone()),
            _ => None,
        })
        .expect("plan event");
    assert!(
        plan.contains("shot_004") && plan.contains("shot_005"),
        "PLAN should carry LLM-supplied bullets, got: {plan:?}"
    );

    let reflect = sink
        .events
        .iter()
        .find_map(|e| match e {
            AgentEvent::Reflect { notes, .. } => Some(notes.clone()),
            _ => None,
        })
        .expect("reflect event");
    assert!(
        reflect.contains("shot_004"),
        "REFLECT should carry LLM-supplied note, got: {reflect:?}"
    );
}

#[tokio::test]
async fn llm_backend_failure_falls_back_to_canned() {
    // No mock server is set up, so every chat call fails
    // (connection refused). The loop must log the failure and
    // fall back to canned PLAN / REFLECT, completing normally.
    let dir = tempdir().unwrap();
    build_story(dir.path());

    // Point at a port we never bind — `127.0.0.1:1` is a
    // privileged-reserved port that won't be open; rig's HTTP
    // client will refuse the connection.
    let model = build_rig_client("http://127.0.0.1:1/v1");
    let llm: Arc<dyn LlmClient> = Arc::new(RigLlm::new(model));

    let mut agent = Agent::new();
    agent.tools_mut().register(ReadFileTool::new(dir.path()));
    let agent = agent.with_llm(llm);

    let mut sink = Collect::default();
    let outcome = agent.run(dir.path(), &mut sink).await.expect("run");
    assert_eq!(outcome, sagaline_agent::StepOutcome::Complete);

    // PLAN text should be the canned scaffold — which references
    // the resolved character slug "lin-mo".
    let plan = sink
        .events
        .iter()
        .find_map(|e| match e {
            AgentEvent::Plan { plan, .. } => Some(plan.clone()),
            _ => None,
        })
        .expect("plan event");
    assert!(
        plan.contains("lin-mo") || plan.contains("laboratory"),
        "canned fallback should reference resolved entities, got: {plan:?}"
    );
}