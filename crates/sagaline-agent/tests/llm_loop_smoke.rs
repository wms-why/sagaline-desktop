//! End-to-end smoke test: the agent loop with a real (mocked)
//! LLM attached via [`RigLlm`].
//!
//! Phase 2.5: the loop drives the SQLite world DB; the
//! `ReadFileTool` registration is gone — only `validate_world`
//! remains, but it's enough to drive PLAN + REFLECT through
//! the LLM and assert both events carry the assistant text.

use std::sync::Arc;

use rig_core::client::CompletionClient;
use rig_core::providers::openai;
use sagaline_agent::tools::ValidateWorldTool;
use sagaline_agent::{Agent, AgentConfig, AgentEvent, EventSink, LlmClient, RigLlm};
use sagaline_store::repo::{NewChapter, NewScene, NewStory};
use sagaline_store::World;
use secrecy::ExposeSecret as _;
use serde_json::json;

fn build_world() -> (Arc<World>, String) {
    let world = Arc::new(World::in_memory().expect("in-memory world"));
    let story = world
        .stories()
        .create(NewStory {
            slug: "demo",
            title: "Demo",
            summary: "",
        })
        .unwrap();
    let chapter = world
        .scenes()
        .create_chapter(NewChapter {
            story_id: &story.id,
            slug: "001-start",
            ordinal: 1,
            title: "Start",
            synopsis: "",
        })
        .unwrap();
    world
        .scenes()
        .create_scene(NewScene {
            chapter_id: &chapter.id,
            slug: "001-intro",
            ordinal: 1,
            title: "Intro",
            synopsis: "Lin Mo walks into the lab.",
        })
        .unwrap();
    (world, story.id)
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

fn build_rig_client(base_url: &str) -> Arc<OpenAIChat> {
    let secret: rig_core::client::BearerAuth =
        sagaline_store::KeyHandle::from_static_for_test("sk-test-fake-key")
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
    let (world, story_id) = build_world();

    let server = wiremock::MockServer::start().await;
    server
        .register(
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .and(wiremock::matchers::path("/v1/chat/completions"))
                .respond_with(
                    wiremock::ResponseTemplate::new(200)
                        .insert_header("content-type", "application/json")
                        .set_body_json(plan_response(
                            "- shot_004: Lin Mo close-up, tense, 3s\n- shot_005: lab wide, 2s\n",
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
                            "shot_004 Lin Mo should look more tense; shot_005 too dim.",
                        )),
                ),
        )
        .await;

    let model = build_rig_client(&format!("{}/v1", server.uri()));
    let llm: Arc<dyn LlmClient> = Arc::new(RigLlm::new(model));

    let mut agent = Agent::with_config(AgentConfig::default());
    agent
        .tools_mut()
        .register(ValidateWorldTool::new(world.clone()));
    let agent = agent.with_llm(llm);

    let mut sink = Collect::default();
    let outcome = agent.run(world, &story_id, &mut sink).await.expect("run");
    assert_eq!(outcome, sagaline_agent::StepOutcome::Complete);

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
    let (world, story_id) = build_world();

    let model = build_rig_client("http://127.0.0.1:1/v1");
    let llm: Arc<dyn LlmClient> = Arc::new(RigLlm::new(model));

    let mut agent = Agent::with_config(AgentConfig::default());
    agent
        .tools_mut()
        .register(ValidateWorldTool::new(world.clone()));
    let agent = agent.with_llm(llm);

    let mut sink = Collect::default();
    let outcome = agent.run(world, &story_id, &mut sink).await.expect("run");
    assert_eq!(outcome, sagaline_agent::StepOutcome::Complete);

    // Canned fallback produces a plan line; without a
    // resolved environment the canned plan uses the scene
    // slug, which is enough to assert "something was emitted".
    let plan = sink
        .events
        .iter()
        .find_map(|e| match e {
            AgentEvent::Plan { plan, .. } => Some(plan.clone()),
            _ => None,
        })
        .expect("plan event");
    assert!(
        !plan.trim().is_empty(),
        "canned fallback should emit at least one plan line"
    );
}
