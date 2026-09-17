//! The agent loop: OBSERVE → PLAN → ACT → REFLECT, per scene.
//!
//! PLAN and REFLECT dispatch to an optional [`LlmClient`] when one
//! is attached via [`Agent::with_llm`]; without one, they fall
//! back to the deterministic canned builders (used by tests and
//! by headless / no-key runs). The loop's shape and the
//! [`AgentEvent`]s it emits do not change between the two modes.

use sagaline_core::{CoreError, EntityType, ParsedEntity, StoryGraph, StoryRoot};
use std::path::Path;

use crate::event::{AgentEvent, EventSink};
use crate::prompt::resolve_context;
use crate::tool::{ToolError, ToolRegistry};

/// Knobs for [`Agent`]. Currently just a max-step guard so a misbehaving
/// PLAN can't loop forever in tests.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Hard cap on steps the loop will run for a single scene.
    /// Default: 8.
    pub max_steps: u32,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self { max_steps: 8 }
    }
}

/// What the loop did with the last scene.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    /// Scene processed; loop is done for this scene.
    Complete,
    /// Scene skipped (not a scene, or graph invalid).
    Skipped { reason: String },
    /// Hit [`AgentConfig::max_steps`] without finishing.
    MaxStepsReached,
}

/// The agent. Cheap to construct; holds a tool registry and a sink.
pub struct Agent {
    config: AgentConfig,
    tools: ToolRegistry,
    llm: Option<std::sync::Arc<dyn crate::llm::LlmClient>>,
}

impl Agent {
    /// Create an agent with default config and an empty tool registry.
    pub fn new() -> Self {
        Self::with_config(AgentConfig::default())
    }

    pub fn with_config(config: AgentConfig) -> Self {
        Self {
            config,
            tools: ToolRegistry::new(),
            llm: None,
        }
    }

    /// Attach an LLM client. With an LLM attached, the loop
    /// dispatches PLAN and REFLECT through it instead of the
    /// canned text builders. Without one, the loop falls back
    /// to the deterministic scaffold so existing tests /
    /// headless runs stay unchanged.
    pub fn with_llm(mut self, llm: std::sync::Arc<dyn crate::llm::LlmClient>) -> Self {
        self.llm = Some(llm);
        self
    }

    /// Borrow the tool registry so callers can `register(...)` tools
    /// before running.
    pub fn tools_mut(&mut self) -> &mut ToolRegistry {
        &mut self.tools
    }

    /// Run the agent loop on every scene in the story rooted at
    /// `story_path`. Emits one [`AgentEvent`] per step into `sink`.
    ///
    /// Async so tool calls (read_file / write_file / rig image-gen /
    /// chat completions) can perform I/O without blocking the
    /// executor. The event sequence emitted is identical to the
    /// pre-async version; the UI subscribes to the same stream.
    pub async fn run<P: AsRef<Path>>(
        &self,
        story_path: P,
        sink: &mut dyn EventSink,
    ) -> Result<StepOutcome, CoreError> {
        let root = StoryRoot::new(story_path.as_ref())?;
        let graph = StoryGraph::load(&root)?;

        // Reflect once up front. If the whole graph is invalid, bail
        // before doing any work — same logic a real LLM reflection
        // would apply, just cheaper.
        if let Err(errs) = graph.validate() {
            let notes = errs
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            sink.emit(AgentEvent::Done {
                step: 0,
                reason: format!("graph invalid: {notes}"),
            });
            return Ok(StepOutcome::Skipped {
                reason: format!("graph invalid: {notes}"),
            });
        }

        let scenes: Vec<&ParsedEntity> =
            graph.entities_by_type(EntityType::Scene);

        if scenes.is_empty() {
            sink.emit(AgentEvent::Done {
                step: 0,
                reason: "no scenes in story".to_string(),
            });
            return Ok(StepOutcome::Skipped {
                reason: "no scenes in story".to_string(),
            });
        }

        // For each scene: one full OBSERVE → PLAN → ACT → REFLECT pass.
        for (idx, scene) in scenes.iter().enumerate() {
            let step = (idx as u32) + 1;
            if step > self.config.max_steps {
                sink.emit(AgentEvent::Done {
                    step,
                    reason: format!("max_steps={} reached", self.config.max_steps),
                });
                return Ok(StepOutcome::MaxStepsReached);
            }
            // Step counter — first event of every step so the UI
            // can render a "step N" header before the details.
            sink.emit(AgentEvent::StepStart { step });
            // 1. OBSERVE
            let resolved = resolve_context(scene, &graph.entities);
            sink.emit(AgentEvent::Observe {
                step,
                scene_id: scene.id.to_string(),
                resolved: resolved.clone(),
            });

            // 2. PLAN — LLM-backed if an LLM is attached, else
            //    canned. Either way the produced text is a
            //    Markdown bullet list; the agent counts `- ` lines
            //    to populate `shots_planned`.
            let plan_text = match self.llm.as_ref() {
                Some(llm) => match llm
                    .complete_plan(&crate::llm::PlanRequest {
                        scene_body: crate::llm::scene_body(scene),
                        resolved: resolved.clone(),
                        tools: crate::llm::tool_summaries(&self.tools),
                    })
                    .await
                {
                    Ok(text) => text,
                    Err(e) => {
                        tracing::warn!(error = %e, "LLM plan failed; falling back to canned");
                        build_canned_plan(scene, &graph)
                    }
                },
                None => build_canned_plan(scene, &graph),
            };
            let shots_planned = plan_text.lines().filter(|l| l.starts_with("- ")).count() as u32;
            sink.emit(AgentEvent::Plan {
                step,
                plan: plan_text.clone(),
                shots_planned,
            });


            // 3. ACT — invoke the read_file tool against the scene path
            //    once, so the loop is observably exercising the tool
            //    registry end-to-end.
            let args = serde_json::json!({ "path": scene.path.to_string_lossy() });
            let result = match self.tools.get("read_file") {
                Some(tool) => match tool.execute(args.clone()).await {
                    Ok(v) => ToolOutcome::Ok(v),
                    Err(e) => ToolOutcome::Err(e),
                },
                None => ToolOutcome::Missing,
            };
            let summary = match &result {
                ToolOutcome::Ok(v) => format!(
                    "{} bytes",
                    v.get("bytes").and_then(|x| x.as_u64()).unwrap_or(0)
                ),
                ToolOutcome::Err(e) => format!("error: {e}"),
                ToolOutcome::Missing => "tool not registered".to_string(),
            };
            sink.emit(AgentEvent::Act {
                step,
                tool: "read_file".to_string(),
                args,
                result_summary: summary.clone(),
            });

            // 4. REFLECT — LLM-backed if an LLM is attached, else
            //    the canned verdict string. The validation_ok
            //    field stays driven by `StoryGraph::validate()`
            //    semantics regardless of LLM presence; we do not
            //    ask the LLM to second-guess the structural
            //    validator.
            let notes = match self.llm.as_ref() {
                Some(llm) => match llm
                    .complete_reflect(&crate::llm::ReflectRequest {
                        scene_id: scene.id.to_string(),
                        plan: plan_text.clone(),
                        tool_name: "read_file".to_string(),
                        tool_args: serde_json::json!({ "path": scene.path.to_string_lossy() }),
                        tool_result_summary: summary.clone(),
                        validation_ok: matches!(result, ToolOutcome::Ok(_)),
                    })
                    .await
                {
                    Ok(text) => text,
                    Err(e) => {
                        tracing::warn!(error = %e, "LLM reflect failed; falling back to canned");
                        if matches!(result, ToolOutcome::Ok(_)) {
                            "scene read back successfully; references resolve".to_string()
                        } else {
                            format!("tool issue: {summary}")
                        }
                    }
                },
                None => {
                    if matches!(result, ToolOutcome::Ok(_)) {
                        "scene read back successfully; references resolve".to_string()
                    } else {
                        format!("tool issue: {summary}")
                    }
                }
            };
            let validation_ok = matches!(result, ToolOutcome::Ok(_));
            sink.emit(AgentEvent::Reflect {
                step,
                validation_ok,
                notes,
            });
        }

        let last = scenes.len() as u32;
        sink.emit(AgentEvent::Done {
            step: last,
            reason: format!("processed {} scene(s)", scenes.len()),
        });
        Ok(StepOutcome::Complete)
    }
}

enum ToolOutcome {
    Ok(serde_json::Value),
    Err(ToolError),
    Missing,
}

impl Default for Agent {
    fn default() -> Self {
        Self::new()
    }
}

/// Canned plan: one "establishing" line for the environment, plus one
/// line per referenced character. Capped at 3 lines so the test output
/// stays small. A real phase will let the LLM produce this.
fn build_canned_plan(scene: &ParsedEntity, graph: &StoryGraph) -> String {
    let resolved = resolve_context(scene, &graph.entities);
    let mut lines: Vec<String> = Vec::new();

    if let Some(env) = &resolved.environment {
        lines.push(format!("- establishing shot: {env}"));
    }
    for c in resolved.characters.iter().take(3) {
        lines.push(format!("- character beat: {c}"));
    }
    if lines.is_empty() {
        lines.push("- ambient shot".to_string());
    }
    lines.join("\n")
}
