//! LLM client trait + a rig-core-backed implementation.
//!
//! The agent's outer loop (`OBSERVE → PLAN → ACT → REFLECT`) calls
//! two LLM-backed steps:
//!
//! - **PLAN** — given the resolved scene context + the available
//!   tool descriptors, produce a shot list as a Markdown bullet
//!   block. The agent parses that text into one Plan event.
//! - **REFLECT** — given the most recent step's tool outcome +
//!   the previous plan, produce a short self-critique string.
//!
//! The trait keeps both calls under one seam so callers (the
//! agent, tests) depend on a stable surface. The implementation
//! in [`RigLlm`] wraps [`sagaline_providers::openai_compat::ChatModel`]
//! — i.e. one rig `GenericCompletionModel<OpenAICompletionsExt>`
//! shared across the agent's lifetime. Configuration (base URL,
//! default model id) is resolved by the caller via
//! `ProviderConfigSet::get(Capability::Chat, provider)`, then the
//! caller hands the constructed `ChatModel` to
//! [`RigLlm::new`].
//!
//! `LlmClient` is intentionally narrow — it does not try to be a
//! multi-provider abstraction. Provider selection lives in
//! `sagaline-providers`; this module only knows how to make chat
//! completions against an OpenAI-compatible endpoint.

use std::sync::Arc;

use async_trait::async_trait;
use rig_core::completion::{CompletionModel, Message};
use rig_core::providers::openai::completion::GenericCompletionModel;
use serde::Serialize;
use thiserror::Error;

use sagaline_core::ParsedEntity;

use crate::event::ResolvedContext;

/// Errors raised by the LLM client. Narrow on purpose — anything
/// beyond provider I/O is an upstream `CoreError`.
#[derive(Debug, Error)]
pub enum LlmError {
    #[error("LLM request failed: {0}")]
    Completion(#[from] rig_core::completion::CompletionError),

    #[error("LLM returned no assistant message")]
    Empty,
}

/// Input to the PLAN step. The agent assembles this once per
/// scene and hands it to the LLM.
#[derive(Debug, Clone, Serialize)]
pub struct PlanRequest {
    /// The Markdown body of the scene the agent is planning for.
    /// Free-form — the LLM is responsible for understanding it.
    pub scene_body: String,
    /// Resolved entity references (characters / environment /
    /// props) — already canonicalised to ids.
    pub resolved: ResolvedContext,
    /// The available tools (name + JSON Schema). The LLM uses
    /// these to pick which `ACT` calls make sense — even though
    /// the agent drives the actual call dispatch.
    pub tools: Vec<ToolSummary>,
}

/// One tool descriptor as exposed to the LLM in PLAN. The full
/// descriptor (with the JSON Schema) is what `ToolRegistry`
/// already produces; this is the wire-friendly subset.
#[derive(Debug, Clone, Serialize)]
pub struct ToolSummary {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Input to the REFLECT step. The agent hands the LLM enough of
/// the recent state for it to produce a useful critique; we
/// deliberately stay text-shaped so the prompt is auditable in
/// tests.
#[derive(Debug, Clone, Serialize)]
pub struct ReflectRequest {
    pub scene_id: String,
    pub plan: String,
    pub tool_name: String,
    pub tool_args: serde_json::Value,
    pub tool_result_summary: String,
    pub validation_ok: bool,
}

/// The LLM seam. Two calls; both async because rig's
/// `CompletionModel::completion` is async.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Produce the shot list for the current scene. Returns the
    /// raw assistant text (Markdown bullets); the agent wraps it
    /// in a `Plan` event.
    async fn complete_plan(&self, req: &PlanRequest) -> Result<String, LlmError>;

    /// Produce a self-critique for the most recent step. The
    /// agent wraps the returned string in a `Reflect` event.
    async fn complete_reflect(&self, req: &ReflectRequest) -> Result<String, LlmError>;
}

/// Rig-backed LLM client. Holds a single `ChatModel` (which is
/// cheap to clone — `Arc` under the hood) and dispatches both
/// PLAN and REFLECT to it with different system/user prompts.
pub struct RigLlm {
    model: Arc<GenericCompletionModel<rig_core::providers::openai::OpenAICompletionsExt>>,
}

impl RigLlm {
    pub fn new(
        model: Arc<GenericCompletionModel<rig_core::providers::openai::OpenAICompletionsExt>>,
    ) -> Self {
        Self { model }
    }
}

#[async_trait]
impl LlmClient for RigLlm {
    async fn complete_plan(&self, req: &PlanRequest) -> Result<String, LlmError> {
        let system = PLAN_SYSTEM;
        let user = serde_json::to_string(req).expect("PlanRequest is JSON-safe");
        let prompt = format!("{system}\n\n```json\n{user}\n```");
        let response = self
            .model
            .completion(self.model.completion_request(Message::user(&prompt)).build())
            .await?;
        extract_assistant_text(&response).ok_or(LlmError::Empty)
    }

    async fn complete_reflect(&self, req: &ReflectRequest) -> Result<String, LlmError> {
        let system = REFLECT_SYSTEM;
        let user = serde_json::to_string(req).expect("ReflectRequest is JSON-safe");
        let prompt = format!("{system}\n\n```json\n{user}\n```");
        let response = self
            .model
            .completion(self.model.completion_request(Message::user(&prompt)).build())
            .await?;
        extract_assistant_text(&response).ok_or(LlmError::Empty)
    }
}

/// Walk rig's `CompletionResponse::choice` and concatenate every
/// text content block. Tool calls (which we deliberately do not
/// advertise in PLAN/REFLECT) are skipped — they're never
/// expected to appear here.
fn extract_assistant_text(
    response: &rig_core::completion::CompletionResponse,
) -> Option<String> {
    use rig_core::completion::AssistantContent;
    let mut out = String::new();
    for content in &response.choice {
        if let AssistantContent::Text(t) = content {
            out.push_str(&t.text);
            out.push('\n');
        }
    }
    if out.trim().is_empty() {
        None
    } else {
        Some(out.trim().to_string())
    }
}

/// Build a `Vec<ToolSummary>` from a sagaline tool registry. We
/// pin the lifetime of the returned summaries to the descriptor
/// schema's owned values so the agent doesn't need to clone per
/// call.
pub fn tool_summaries(registry: &crate::tool::ToolRegistry) -> Vec<ToolSummary> {
    registry
        .descriptors()
        .into_iter()
        .map(|d| ToolSummary {
            name: d.name,
            description: d.description,
            parameters: serde_json::to_value(&d.parameters)
                .expect("RootSchema is JSON-safe"),
        })
        .collect()
}

/// Extract the Markdown body of a scene for the PLAN prompt.
/// Helper kept here so the agent doesn't need to know how to
/// phrase scene metadata for the LLM.
pub fn scene_body(scene: &ParsedEntity) -> String {
    let mut out = String::new();
    if let Some(title) = scene.frontmatter.get("title").and_then(|v| v.as_str()) {
        out.push_str(&format!("# {title}\n\n"));
    }
    out.push_str(scene.body.trim());
    out
}

const PLAN_SYSTEM: &str = "You are the PLAN step of a video-production agent. \
Given a scene's body and the entities it references, produce a short \
Markdown bullet list of shots. Each bullet should be one shot, with a \
3-7 word description, the camera framing (wide | medium | close_up | \
insert | pov), and a duration in seconds. Output ONLY the bullet list — \
no commentary, no code fences.";

const REFLECT_SYSTEM: &str = "You are the REFLECT step of a video-production agent. \
Given the most recent step's tool outcome and the scene's plan, produce \
one short sentence (≤25 words) noting whether the step met the plan or \
what should change. Output ONLY that sentence — no preamble.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_request_serializes_as_json() {
        let req = PlanRequest {
            scene_body: "Lin Mo enters the lab.".into(),
            resolved: ResolvedContext {
                characters: vec!["lin-mo".into()],
                environment: Some("laboratory".into()),
                props: vec!["energy-core".into()],
            },
            tools: vec![ToolSummary {
                name: "generate_image".into(),
                description: "Generate an image".into(),
                parameters: serde_json::json!({"type": "object"}),
            }],
        };
        let s = serde_json::to_string(&req).unwrap();
        assert!(s.contains("lin-mo"));
        assert!(s.contains("generate_image"));
    }

    #[test]
    fn reflect_request_serializes_as_json() {
        let req = ReflectRequest {
            scene_id: "scene_001".into(),
            plan: "- shot_004: Lin Mo close-up".into(),
            tool_name: "read_file".into(),
            tool_args: serde_json::json!({"path": "scenes/001-intro.md"}),
            tool_result_summary: "123 bytes".into(),
            validation_ok: true,
        };
        let s = serde_json::to_string(&req).unwrap();
        assert!(s.contains("read_file"));
        assert!(s.contains("scene_001"));
    }
}