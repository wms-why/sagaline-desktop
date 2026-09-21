//! The agent loop: OBSERVE → PLAN → ACT → REFLECT, per scene.
//!
//! PLAN and REFLECT dispatch to an optional [`LlmClient`] when one
//! is attached via [`Agent::with_llm`]; without one, they fall
//! back to the deterministic canned builders (used by tests and
//! by headless / no-key runs). The loop's shape and the
//! [`AgentEvent`]s it emits do not change between the two modes.
//!
//! ## Phase 2.5
//!
//! The loop now drives the SQLite [`sagaline_store::World`]
//! instead of the Markdown-era [`sagaline_core::StoryGraph`].
//! Scene iteration comes from `world.scenes().list_scenes_for_story`,
//! and the ACT step calls `validate_world` (a Read tool) so the
//! per-step event has a meaningful `result_summary`. The legacy
//! filesystem tools (`read_file` / `write_file` / `list_dir` /
//! `find` / `validate_story`) are deleted.

use sagaline_core::CoreError;
use sagaline_store::repo::SceneRow;
use sagaline_store::World;

use async_stream::try_stream;
use futures::Stream;
use futures::StreamExt;

use crate::event::{AgentEvent, EventSink};
use crate::tool::{ToolError, ToolRegistry};
use std::sync::{Arc, RwLock};

/// How the loop dispatches `Capability::Mutate` tool calls.
///
/// - `Auto` — today's behavior: the tool runs inline and the
///   world DB is mutated immediately.
/// - `Manual` — every `Mutate` call is routed through a
///   `propose_change`-style record: a single-action pending
///   proposal is created in `proposals` + `proposal_actions`,
///   and the tool's actual side-effects wait for a manual
///   `approve_proposal` (Phase 3). Read / Execute tools are
///   not affected — they always run inline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitPolicy {
    /// Mutations land immediately. Default for headless /
    /// canned-mode runs.
    Auto,
    /// Mutations are recorded as pending proposals; nothing
    /// touches the world DB until `approve_proposal` is called.
    /// Used by the GUI agent so the user can review / batch.
    Manual,
}

impl Default for CommitPolicy {
    fn default() -> Self {
        CommitPolicy::Auto
    }
}

/// Knobs for [`Agent`].
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Hard cap on steps the loop will run for a single story.
    /// Default: 8.
    pub max_steps: u32,
    /// How `Capability::Mutate` tool calls are dispatched. See
    /// [`CommitPolicy`]. Default: `Auto`.
    pub commit_policy: CommitPolicy,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_steps: 8,
            commit_policy: CommitPolicy::Auto,
        }
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
    /// Live [`CommitPolicy`]. The UI's activity-panel picker
    /// calls [`Self::set_commit_policy`] to flip this; the next
    /// [`Self::dispatch_tool`] call inside the same agent loop
    /// honours the new value without rebuilding the agent. Seeded
    /// from [`AgentConfig::commit_policy`] at construction.
    commit_policy: Arc<RwLock<CommitPolicy>>,
    /// `Arc` so [`Self::run_stream`] can move a clone into its
    /// `Send + 'static` stream body. Registration must happen
    /// before the `Agent` is shared — `Arc::get_mut` returns
    /// `None` once anyone else holds a clone.
    tools: std::sync::Arc<ToolRegistry>,
    llm: Option<std::sync::Arc<dyn crate::llm::LlmClient>>,
}

impl Agent {
    /// Create an agent with default config and an empty tool registry.
    pub fn new() -> Self {
        Self::with_config(AgentConfig::default())
    }

    pub fn with_config(config: AgentConfig) -> Self {
        Self {
            config: config.clone(),
            commit_policy: Arc::new(RwLock::new(config.commit_policy)),
            tools: std::sync::Arc::new(ToolRegistry::new()),
            llm: None,
        }
    }

    /// Current [`CommitPolicy`]. Cheap; reads the live override.
    pub fn commit_policy(&self) -> CommitPolicy {
        *self.commit_policy.read().expect("commit_policy lock poisoned")
    }

    /// Flip the live [`CommitPolicy`]. Takes effect on the next
    /// [`Self::dispatch_tool`] call inside the same agent loop
    /// — does not retroactively change pending proposals, and
    /// does NOT touch the world DB or any in-flight tool call.
    /// The activity panel's `Offerular` toggle dispatches this
    /// through [`crate::ProposalService`] when the user clicks
    /// Auto / Manual.
    pub fn set_commit_policy(&self, policy: CommitPolicy) {
        *self.commit_policy.write().expect("commit_policy lock poisoned") = policy;
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
    /// before running. Panics if the `Agent` has been shared (i.e.
    /// `Arc::get_mut` returns `None`) — registration must happen
    /// before [`Agent`] is wrapped in an `Arc` for the gpui
    /// pipeline.
    pub fn tools_mut(&mut self) -> &mut ToolRegistry {
        std::sync::Arc::get_mut(&mut self.tools).expect("Agent::tools_mut: registry already shared")
    }

    /// Shared handle to the tool registry. Cheap (bump refcount).
    /// Used by [`crate::ProposalService`] to look up
    /// `approve_proposal` / `reject_proposal` and dispatch them
    /// through the same registry the agent loop uses.
    pub fn tools(&self) -> Arc<ToolRegistry> {
        self.tools.clone()
    }

    /// Dispatch a single tool call through this agent, honoring
    /// [`AgentConfig::commit_policy`].
    ///
    /// - `Auto` policy (default): any registered tool runs
    ///   inline via [`Tool::execute`]. Same as calling the tool
    ///   directly through [`Self::tools_mut`].
    /// - `Manual` policy + [`Capability::Mutate`] tool: the call
    ///   is **not** executed. Instead a single-action pending
    ///   proposal is created and recorded in `proposal_actions`;
    ///   the world DB is untouched. The returned JSON carries
    ///   `{proposal_id, actions_recorded: 1}` so the caller can
    ///   `approve_proposal` it later.
    /// - `Manual` policy + `Read` / `Execute` tool: runs inline.
    ///
    /// Phase 2.5 rewires the loop's internal `tool.execute(...)`
    /// call site to use this method so the loop also benefits.
    /// For now the binary calls it directly.
    pub async fn dispatch_tool(
        &self,
        ctx: crate::tool::ToolContext,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, crate::tool::ToolError> {
        use crate::tool::Capability;

        let tool =
            self.tools
                .get(tool_name)
                .ok_or_else(|| crate::tool::ToolError::UnknownTool {
                    name: tool_name.to_string(),
                })?;

        if matches!(self.commit_policy(), CommitPolicy::Manual)
            && tool.descriptor().capability == Capability::Mutate
        {
            // Record-only path. The recorded args (without
            // `story_id` / `summary`) are the same JSON object
            // the LLM supplied. We pull `story_id` out of the
            // common arg shapes (story_id / chapter_id for scene
            // tools; for tools without story_id we fall back to
            // "unscoped" and let the user fix the proposal
            // before approving).
            let story_id = args
                .get("story_id")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let summary = format!("auto-recorded `{tool_name}` from Manual policy");

            let proposal_id = crate::tools::proposal::record_single_action(
                &ctx.world,
                &ctx.agent_id,
                story_id.as_deref(),
                &summary,
                tool_name,
                &args,
            )
            .map_err(|e| crate::tool::ToolError::Execution {
                name: tool_name.to_string(),
                source: Box::new(e),
            })?;

            return serde_json::to_value(serde_json::json!({
                "proposal_id": proposal_id,
                "actions_recorded": 1,
                "commit_policy": "manual",
            }))
            .map_err(|e| crate::tool::ToolError::Execution {
                name: tool_name.to_string(),
                source: Box::new(e),
            });
        }

        tool.execute(ctx, args).await
    }

    /// Run the agent loop on every scene in `story_id` (the
    /// SQLite world DB), returning a [`Stream`] of
    /// [`AgentEvent`]s.
    ///
    /// This is the canonical API. Tool calls (`validate_world`,
    /// `get_story`, future rig chat / image adapters) do real
    /// I/O via `tokio::fs` / reqwest, so the returned stream
    /// **must be polled on a Tokio runtime** — the `sagaline`
    /// binary does this through `sagaline_bridge::TokioBridge`.
    /// Tests can drive it on `#[tokio::test]`, or on any
    /// `tokio::test`-spawned runtime.
    ///
    /// Stream shape:
    /// - For each scene: `StepStart` -> `Observe` -> `Plan` ->
    ///   `Act` -> `Reflect`.
    /// - One final `Done { reason }` per run. `reason` is
    ///   human-readable; do not parse it.
    ///
    /// Top-level load failures still yield exactly one `Done`
    /// event so consumers never have to special-case
    /// pre-flight errors.
    pub fn run_stream(
        &self,
        world: Arc<World>,
        story_id: &str,
    ) -> impl Stream<Item = AgentEvent> + Send + 'static {
        let config = self.config.clone();
        let tools = self.tools.clone();
        let llm = self.llm.clone();
        let story_id_owned = story_id.to_string();

        try_stream! {
                let ctx = crate::tool::ToolContext::new(world.clone());

                let story = match world.stories().get(&story_id_owned) {
                    Ok(Some(s)) => s,
                    Ok(None) => {
                        yield AgentEvent::Done {
                            step: 0,
                            reason: format!(
                                "load failed: story `{story_id_owned}` not found"
                            ),
                        };
                        return;
                    }
                    Err(e) => {
                        yield AgentEvent::Done {
                            step: 0,
                            reason: format!("load failed: {e}"),
                        };
                        return;
                    }
                };

                let scenes = match world.scenes().list_scenes_for_story(&story.id) {
                    Ok(s) => s,
                    Err(e) => {
                        yield AgentEvent::Done {
                            step: 0,
                            reason: format!("scenes query failed: {e}"),
                        };
                        return;
                    }
                };

                if scenes.is_empty() {
                    yield AgentEvent::Done {
                        step: 0,
                        reason: "no scenes in story".to_string(),
                    };
                    return;
                }

                for (idx, scene) in scenes.iter().enumerate() {
                    let step = (idx as u32) + 1;
                    if step > config.max_steps {
                        yield AgentEvent::Done {
                            step,
                            reason: format!(
                                "max_steps={} reached",
                                config.max_steps
                            ),
                        };
                        return;
                    }
                    yield AgentEvent::StepStart { step };

                    let character_ids = world
                        .scenes()
                        .character_ids_for_scene(&scene.id)
                        .unwrap_or_default();
                    let environment_ids = world
                        .scenes()
                        .environment_ids_for_scene(&scene.id)
                        .unwrap_or_default();
                    let resolved = crate::event::ResolvedContext {
                        characters: character_ids.clone(),
                        environment: environment_ids.first().cloned(),
                        props: vec![],
                    };
                    yield AgentEvent::Observe {
                        step,
                        scene_id: scene.id.clone(),
                        resolved: resolved.clone(),
                    };

                    let plan_text = match llm.as_ref() {
                        Some(llm) => match llm
                            .complete_plan(&crate::llm::PlanRequest {
                                scene_body: crate::llm::scene_body(scene),
                                resolved: resolved.clone(),
                                tools: crate::llm::tool_summaries(&tools),
                            })
                            .await
                        {
                            Ok(text) => text,
                            Err(e) => {
                                tracing::warn!(error = %e, "LLM plan failed; falling back to canned");
                                build_canned_plan(scene, &character_ids, environment_ids.first())
                            }
                        },
                        None => build_canned_plan(scene, &character_ids, environment_ids.first()),
                    };
                    let shots_planned = plan_text
                        .lines()
                        .filter(|l| l.starts_with("- "))
                        .count() as u32;
                    yield AgentEvent::Plan {
                        step,
                        plan: plan_text.clone(),
                        shots_planned,
                    };

                    // ACT: call `validate_world` filtered to the
                    // current story. Replaces the legacy
                    // `read_file` step — `validate_world` is a
                    // Read tool so it bypasses the proposal gate
                    // under Manual policy.
                    let act_args = serde_json::json!({ "story_id": story.id });
                    let result = match tools.get("validate_world") {
                        Some(tool) => match tool.execute(ctx.clone(), act_args.clone()).await {
                            Ok(v) => ToolOutcome::Ok(v),
                            Err(e) => ToolOutcome::Err(e),
                        },
                        None => ToolOutcome::Missing,
                    };
                    let summary = match &result {
                        ToolOutcome::Ok(v) => {
                            let ok = v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false);
                            let n = v
                                .get("errors")
                                .and_then(|x| x.as_array())
                                .map(|a| a.len())
                                .unwrap_or(0);
                            if ok {
                                "ok".to_string()
                            } else {
                                format!("{n} validation issue(s)")
                            }
                        }
                        ToolOutcome::Err(e) => format!("error: {e}"),
                        ToolOutcome::Missing => "tool not registered".to_string(),
                    };
                    yield AgentEvent::Act {
                        step,
                        tool: "validate_world".to_string(),
                        args: act_args,
                        result_summary: summary.clone(),
                    };

                    let notes = match llm.as_ref() {
                        Some(llm) => match llm
                            .complete_reflect(&crate::llm::ReflectRequest {
                                scene_id: scene.id.clone(),
                                plan: plan_text.clone(),
                                tool_name: "validate_world".to_string(),
                                tool_args: serde_json::json!({ "story_id": story.id }),
                                tool_result_summary: summary.clone(),
                                validation_ok: matches!(&result, ToolOutcome::Ok(v) if v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false)),
                            })
                            .await
                        {
                            Ok(text) => text,
                            Err(e) => {
                                tracing::warn!(error = %e, "LLM reflect failed; falling back to canned");
                                if matches!(&result, ToolOutcome::Ok(v) if v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false)) {
                                    "world validation passed; references resolve".to_string()
                                } else {
                                    format!("validation issue: {summary}")
                                }
                            }
                        },
                        None => {
                            if matches!(&result, ToolOutcome::Ok(v) if v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false)) {
                                "world validation passed; references resolve".to_string()
                            } else {
                                format!("validation issue: {summary}")
                            }
                        }
                    };
                    let validation_ok = matches!(&result, ToolOutcome::Ok(v) if v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false));
                    yield AgentEvent::Reflect {
                        step,
                        validation_ok,
                        notes,
                    };
                }

                let last = scenes.len() as u32;
                yield AgentEvent::Done {
                    step: last,
                    reason: format!("processed {} scene(s)", scenes.len()),
                };
            }
            // `try_stream!` yields `Result<T, E>`; map errors
            // into a `Done` event so the public stream is
            // `Item = AgentEvent`. The top-level load failures
            // above already short-circuit on `Err`, so this
            // branch only fires if a future tool gains a `?`.
            .map(|r: Result<AgentEvent, CoreError>| match r {
                Ok(ev) => ev,
                Err(e) => AgentEvent::Done {
                    step: 0,
                    reason: format!("agent error: {e}"),
                },
            })
    }

    /// Backwards-compatible sink-based API. Drains
    /// [`Self::run_stream`] into `sink` and derives a
    /// [`StepOutcome`] from the terminal `Done` event's reason.
    /// Phase 2.5: caller supplies the world + story id.
    ///
    /// New callers should prefer [`Self::run_stream`].
    pub async fn run(
        &self,
        world: Arc<World>,
        story_id: &str,
        sink: &mut dyn EventSink,
    ) -> Result<StepOutcome, CoreError> {
        let mut stream = Box::pin(self.run_stream(world, story_id));
        let mut outcome = StepOutcome::Complete;
        while let Some(event) = stream.next().await {
            if let AgentEvent::Done { ref reason, .. } = event {
                outcome = outcome_from_done_reason(reason);
            }
            sink.emit(event);
        }
        Ok(outcome)
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
fn build_canned_plan(
    scene: &SceneRow,
    character_ids: &[String],
    environment_id: Option<&String>,
) -> String {
    let mut lines: Vec<String> = Vec::new();

    if let Some(env) = environment_id {
        lines.push(format!("- establishing shot: {env}"));
    } else {
        lines.push(format!("- establishing shot: {}", scene.slug));
    }
    for c in character_ids.iter().take(3) {
        lines.push(format!("- character beat: {c}"));
    }
    if lines.is_empty() {
        lines.push("- ambient shot".to_string());
    }
    lines.join("\n")
}

/// Map a terminal `Done` event's `reason` back to a
/// [`StepOutcome`]. Strings must stay in sync with the prefixes
/// used in [`Agent::run_stream`].
fn outcome_from_done_reason(reason: &str) -> StepOutcome {
    if reason.starts_with("graph invalid")
        || reason.starts_with("no scenes in story")
        || reason.starts_with("load failed")
        || reason.starts_with("scenes query failed")
    {
        StepOutcome::Skipped {
            reason: reason.to_string(),
        }
    } else if reason.starts_with("max_steps") {
        StepOutcome::MaxStepsReached
    } else {
        StepOutcome::Complete
    }
}
