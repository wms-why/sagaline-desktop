//! Proposal → Commit workflow tools.
//!
//! - `propose_change` — bundle a list of tool calls into a new
//!   pending proposal. Rows land in `proposal_actions` but the
//!   world DB is NOT mutated yet.
//! - `list_pending_proposals` — read the pending queue.
//! - `approve_proposal` — validate, replay each action against the
//!   world DB, flip the proposal to `committed`, and write the
//!   audit log. If validation fails the world DB is untouched.
//! - `reject_proposal` — flip a pending proposal to `rejected`.
//!
//! ## Atomicity caveats (Phase 5)
//!
//! `approve_proposal` opens ONE outer SQLite transaction that
//! wraps the pre-flight `validate_world_in_tx` check, every
//! replay (for tools that implement `Tool::execute_in_tx`),
//! every replay audit row, the proposal status flip to
//! `committed`, and the approve-call audit row. If anything
//! inside the loop fails (or `commit()` itself fails), the
//! transaction is dropped without commit and the world DB
//! stays at its pre-approve state. The proposal stays
//! `pending`; the user fixes the failing action and re-runs.
//!
//! Tools that opt into `execute_in_tx` participate in this
//! guarantee end-to-end. Tools that don't still call their
//! per-tool `execute`, which opens its own connection -- that
//! connection writes world-DB rows independently and CANNOT be
//! rolled back when the outer tx aborts. The approve audit row
//! for those tools still records correctly, but the world DB
//! may end up partially applied. The supported-tools list grows
//! as each domain tool opts in.

use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use sagaline_store::{
    validate_world_in_tx, AgentActionRepo, NewAgentAction, NewProposalAction, ProposalActionRepo,
    ProposalRepo, ProposalStatus,
};

use crate::tool::{
    Capability, Tool, ToolContext, ToolDescriptor, ToolError, ToolRegistry, ToolResult,
};
use crate::tools::domain;

// ---- propose_change -----------------------------------------------------

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ProposedActionArgs {
    /// The tool to invoke. Must be a name registered in the
    /// agent's [`ToolRegistry`] (case-sensitive). Domain tools
    /// (`create_*`, `update_*`, `add_*`, `assign_*`) are the
    /// canonical replay targets; `Read` tools are accepted but
    /// waste proposal slots.
    pub tool_name: String,
    /// The JSON object this tool would receive. Schema is the
    /// tool's own descriptor; we don't re-validate it here.
    pub args: Value,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ProposeChangeArgs {
    pub story_id: String,
    /// Short human-readable label, surfaced in the UI's pending
    /// list. e.g. "Add Lin Mo as detective in scene 3".
    pub summary: String,
    #[serde(default)]
    pub diff_json: Option<String>,
    pub actions: Vec<ProposedActionArgs>,
}

#[derive(Debug, Serialize)]
pub struct ProposeChangeOutput {
    pub proposal_id: String,
    pub actions_recorded: usize,
}

pub struct ProposeChangeTool {
    world: Arc<sagaline_store::World>,
}

impl ProposeChangeTool {
    pub fn new(world: Arc<sagaline_store::World>) -> Self {
        Self { world }
    }
}

#[async_trait]
impl Tool for ProposeChangeTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<ProposeChangeArgs>(
            "propose_change",
            "Bundle a list of tool calls into a new pending \
             proposal. The tool calls land in `proposal_actions` \
             but the world DB is NOT mutated yet — call \
             `approve_proposal` to run them, or `reject_proposal` \
             to discard. Returns `{proposal_id, \
             actions_recorded}`. The owning agent_id is taken \
             from `ToolContext::agent_id`.",
            Capability::Mutate,
        )
    }

    async fn execute(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: ProposeChangeArgs = domain::parse_args("propose_change", args)?;

        if parsed.actions.is_empty() {
            return Err(ToolError::BadArgs {
                name: "propose_change".into(),
                message: "`actions` must contain at least one entry".into(),
            });
        }

        let mut conn = self
            .world
            .conn()
            .map_err(|e| domain::map_store_err("propose_change", e))?;
        let tx = conn
            .transaction()
            .map_err(|e| domain::map_store_err("propose_change", e))?;

        // 1. Insert the proposal.
        let proposals = ProposalRepo::new(&self.world);
        let proposal_id = proposals.new_id();
        tx.execute(
            "INSERT INTO proposals
                (id, story_id, agent_id, status, summary, diff_json,
                 created_at, decided_at, decided_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL)",
            rusqlite::params![
                proposal_id,
                parsed.story_id,
                ctx.agent_id,
                ProposalStatus::Pending.as_str(),
                parsed.summary,
                parsed.diff_json.as_deref().unwrap_or(""),
                sagaline_store::time_util::now_iso(),
            ],
        )
        .map_err(|e| domain::map_store_err("propose_change", e))?;

        // 2. Insert each action. Serialise the args once so the
        // replay can `serde_json::from_str` them back into
        // `serde_json::Value`.
        let actions_repo = ProposalActionRepo::new(&self.world);
        let mut n_actions = 0usize;
        for (idx, action) in parsed.actions.iter().enumerate() {
            let args_json =
                serde_json::to_string(&action.args).map_err(|e| ToolError::Execution {
                    name: "propose_change".into(),
                    source: Box::new(e),
                })?;
            actions_repo
                .record_in_tx(
                    &tx,
                    NewProposalAction {
                        proposal_id: &proposal_id,
                        seq: (idx as i64) + 1,
                        tool_name: &action.tool_name,
                        tool_args_json: &args_json,
                    },
                )
                .map_err(|e| domain::map_store_err("propose_change", e))?;
            n_actions += 1;
        }

        tx.commit()
            .map_err(|e| domain::map_store_err("propose_change", e))?;

        // 3. Record an audit row for the propose call itself
        // (with `proposal_id` set).
        let audit = AgentActionRepo::new(&self.world);
        let _ = audit
            .record(NewAgentAction {
                story_id: &parsed.story_id,
                agent_id: &ctx.agent_id,
                tool_name: "propose_change",
                args_json: &serde_json::to_string(&parsed).unwrap_or_default(),
                result_summary: Some(&format!(
                    "queued proposal `{proposal_id}` with {n_actions} actions"
                )),
                proposal_id: Some(&proposal_id),
            })
            .map_err(|e| domain::map_store_err("propose_change", e))?;

        domain::to_result(&ProposeChangeOutput {
            proposal_id,
            actions_recorded: n_actions,
        })
    }
}

// ---- list_pending_proposals --------------------------------------------

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ListPendingProposalsArgs {
    #[serde(default)]
    pub story_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListPendingProposalsOutput {
    pub proposals: Vec<sagaline_store::ProposalRow>,
}

pub struct ListPendingProposalsTool {
    world: Arc<sagaline_store::World>,
}

impl ListPendingProposalsTool {
    pub fn new(world: Arc<sagaline_store::World>) -> Self {
        Self { world }
    }
}

#[async_trait]
impl Tool for ListPendingProposalsTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<ListPendingProposalsArgs>(
            "list_pending_proposals",
            "List every proposal in `pending` status, newest \
             first. Optional `story_id` restricts to one story. \
             Returns `{proposals: [{id, story_id, agent_id, \
             status, summary, diff_json, created_at, \
             decided_at, decided_by}]}`. `decided_at` / \
             `decided_by` are `null` for pending proposals.",
            Capability::Read,
        )
    }

    async fn execute(&self, _ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: ListPendingProposalsArgs = domain::parse_args("list_pending_proposals", args)?;
        let proposals = ProposalRepo::new(&self.world)
            .list(parsed.story_id.as_deref(), Some(ProposalStatus::Pending))
            .map_err(|e| domain::map_store_err("list_pending_proposals", e))?;
        domain::to_result(&ListPendingProposalsOutput { proposals })
    }
}

// ---- approve_proposal --------------------------------------------------

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ApproveProposalArgs {
    pub proposal_id: String,
    /// Optional; defaults to `"user"`. The actor that decided
    /// the proposal (used by the audit log and surfaced in the
    /// UI).
    #[serde(default)]
    pub decided_by: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ApproveProposalOutput {
    pub proposal_id: String,
    pub actions_replayed: usize,
    pub audit_rows: usize,
}

pub struct ApproveProposalTool {
    world: Arc<sagaline_store::World>,
    /// The tool registry the approve tool looks up action
    /// targets in. Set via [`Self::with_registry`] after
    /// construction so the binary can wire its own registry
    /// (the test path uses a fresh registry holding the domain
    /// tools).
    registry: Arc<ToolRegistry>,
}

impl ApproveProposalTool {
    pub fn new(world: Arc<sagaline_store::World>) -> Self {
        Self {
            world,
            registry: Arc::new(ToolRegistry::new()),
        }
    }
}

#[async_trait]
impl Tool for ApproveProposalTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<ApproveProposalArgs>(
            "approve_proposal",
            "Run every action in a pending proposal against the \
             world DB. Opens one outer SQLite transaction that \
             wraps the pre-flight `validate_world` check, every \
             action replay, the per-replay audit row, the \
             proposal's `committed` flip, and the approve-call \
             audit row. If anything inside fails the whole \
             transaction rolls back — the world DB stays at its \
             pre-approve state and the proposal stays `pending`. \
             Tools that opt into `Tool::execute_in_tx` \
             (`create_character`, `create_chapter`, \
             `create_environment`, `create_prop`, `create_scene`) \
             write inside this tx; tools that haven't opted in \
             still fall back to per-tool auto-commit, which means \
             a partial-failure leaves the world DB in whatever \
             state the successful replays reached (Phase 5 \
             caveat). Returns `{proposal_id, actions_replayed, \
             audit_rows}`.",
            Capability::Mutate,
        )
    }

    async fn execute(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: ApproveProposalArgs = domain::parse_args("approve_proposal", args)?;

        let proposals = ProposalRepo::new(&self.world);
        let actions_repo = ProposalActionRepo::new(&self.world);
        let audit = AgentActionRepo::new(&self.world);

        // 1. Load the proposal. Must be `pending`.
        let proposal = proposals
            .get(&parsed.proposal_id)
            .map_err(|e| domain::map_store_err("approve_proposal", e))?
            .ok_or_else(|| ToolError::Execution {
                name: "approve_proposal".into(),
                source: Box::new(std::io::Error::other(format!(
                    "proposal `{}` not found",
                    parsed.proposal_id
                ))),
            })?;
        if proposal.status != ProposalStatus::Pending {
            return Err(ToolError::BadArgs {
                name: "approve_proposal".into(),
                message: format!(
                    "proposal `{}` is `{}` (not `pending`); only pending proposals can be approved",
                    parsed.proposal_id,
                    proposal.status.as_str()
                ),
            });
        }

        // 2. Load the queued actions in seq order.
        let actions = actions_repo
            .list_by_proposal(&parsed.proposal_id)
            .map_err(|e| domain::map_store_err("approve_proposal", e))?;
        if actions.is_empty() {
            return Err(ToolError::BadArgs {
                name: "approve_proposal".into(),
                message: format!("proposal `{}` has no queued actions", parsed.proposal_id),
            });
        }

        let decided_by = parsed.decided_by.as_deref().unwrap_or("user").to_string();

        // 3. Pre-parse every action's stored args JSON, look up
        //    every tool in the registry, and split the actions
        //    into two phases:
        //      - `async_actions`: tools that don't implement
        //        `execute_in_tx` (must run via `execute`, async).
        //      - `in_tx_actions`: tools that DO implement
        //        `execute_in_tx` (sync, run inside the outer tx).
        //
        //    Why split? `rusqlite::Transaction` is `!Send`, and
        //    `tool.execute(...)` is async — holding a tx across
        //    that `.await` would force the future to be `!Send`,
        //    which the `Tool` trait forbids. We run async replays
        //    first (their writes cannot be rolled back; this is
        //    the Phase 5 caveat), then open ONE outer tx for the
        //    rest. The in-tx replays + status flip + approve-call
        //    audit row all land inside that outer tx; any failure
        //    drops the tx without commit and the world DB stays
        //    at its pre-approve state.
        // Pre-parse every action's stored args JSON and look up
        // every tool in the registry. Split into two passes based
        // on `supports_in_tx`:
        //   - `async_actions`: tools without `execute_in_tx` (run
        //     via `tool.execute(...)`, which is async).
        //   - `in_tx_actions`: tools with `execute_in_tx` (sync,
        //     run inside the outer tx).
        //
        // Why split? `rusqlite::Transaction` is `!Send`, and
        // `tool.execute(...)` is async — holding a tx across that
        // `.await` would force the future to be `!Send`, which
        // the `Tool` trait forbids. We run async replays first
        // (their writes cannot be rolled back; Phase 5 caveat),
        // then open ONE outer tx for the rest. The in-tx replays
        // + status flip + approve-call audit row all land inside
        // that outer tx; any failure drops the tx without commit
        // and the world DB stays at its pre-approve state.
        let mut async_actions: Vec<(usize, Value)> = Vec::new();
        let mut in_tx_actions: Vec<(usize, Value)> = Vec::new();
        for (idx, action) in actions.iter().enumerate() {
            let tool_value: Value =
                serde_json::from_str(&action.tool_args_json).map_err(|e| ToolError::Execution {
                    name: "approve_proposal".into(),
                    source: Box::new(std::io::Error::other(format!(
                        "proposal action #{} `{}`: stored args JSON is malformed: {}",
                        action.seq, action.tool_name, e
                    ))),
                })?;
            let tool =
                self.registry
                    .get(&action.tool_name)
                    .ok_or_else(|| ToolError::UnknownTool {
                        name: action.tool_name.clone(),
                    })?;
            if tool.supports_in_tx() {
                in_tx_actions.push((idx, tool_value));
            } else {
                async_actions.push((idx, tool_value));
            }
        }

        let mut n_replayed = 0usize;

        // Phase 1: async replays. Each tool opens its own
        // connection and commits independently. We record the
        // audit row in the same connection (auto-commit) so the
        // world write + audit row are paired per replay.
        for (idx, tool_value) in &async_actions {
            let action = &actions[*idx];
            let replay_ctx = ToolContext {
                world: self.world.clone(),
                agent_id: ctx.agent_id.clone(),
                proposal_id: Some(parsed.proposal_id.clone()),
            };
            let tool = self
                .registry
                .get(&action.tool_name)
                .expect("tool looked up in pre-parse step is still registered");
            let result = tool
                .execute(replay_ctx, tool_value.clone())
                .await
                .map_err(|e| ToolError::Execution {
                    name: "approve_proposal".into(),
                    source: Box::new(std::io::Error::other(format!(
                        "proposal action #{} `{}` failed: {}",
                        action.seq, action.tool_name, e
                    ))),
                })?;
            let summary = result
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("ok");
            audit
                .record(NewAgentAction {
                    story_id: &proposal.story_id,
                    agent_id: &ctx.agent_id,
                    tool_name: &action.tool_name,
                    args_json: &action.tool_args_json,
                    result_summary: Some(summary),
                    proposal_id: Some(&parsed.proposal_id),
                })
                .map_err(|e| domain::map_store_err("approve_proposal", e))?;
            n_replayed += 1;
        }

        // Phase 2: open ONE outer tx. Validate, replay in-tx
        // actions, flip status, record approve audit.
        let mut conn = self
            .world
            .conn()
            .map_err(|e| domain::map_store_err("approve_proposal", e))?;
        let tx = conn
            .transaction()
            .map_err(|e| domain::map_store_err("approve_proposal", e))?;

        let issues = validate_world_in_tx(&tx, Some(&proposal.story_id))
            .map_err(|e| domain::map_store_err("approve_proposal", e))?;
        if !issues.is_empty() {
            return Err(ToolError::Execution {
                name: "approve_proposal".into(),
                source: Box::new(std::io::Error::other(format!(
                    "validate_world reported {} issue(s); world DB untouched",
                    issues.len()
                ))),
            });
        }

        for (idx, tool_value) in &in_tx_actions {
            let action = &actions[*idx];
            let replay_ctx = ToolContext {
                world: self.world.clone(),
                agent_id: ctx.agent_id.clone(),
                proposal_id: Some(parsed.proposal_id.clone()),
            };
            let tool = self
                .registry
                .get(&action.tool_name)
                .expect("tool looked up in pre-parse step is still registered");
            let result = tool
                .execute_in_tx(&replay_ctx, &tx, tool_value.clone())
                .map_err(|e| ToolError::Execution {
                    name: "approve_proposal".into(),
                    source: Box::new(std::io::Error::other(format!(
                        "proposal action #{} `{}` failed: {}",
                        action.seq, action.tool_name, e
                    ))),
                })?;
            let summary = result
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("ok");
            audit
                .record_in_tx(
                    &tx,
                    NewAgentAction {
                        story_id: &proposal.story_id,
                        agent_id: &ctx.agent_id,
                        tool_name: &action.tool_name,
                        args_json: &action.tool_args_json,
                        result_summary: Some(summary),
                        proposal_id: Some(&parsed.proposal_id),
                    },
                )
                .map_err(|e| domain::map_store_err("approve_proposal", e))?;
            n_replayed += 1;
        }

        proposals
            .set_status_in_tx(
                &tx,
                &parsed.proposal_id,
                ProposalStatus::Committed,
                &decided_by,
            )
            .map_err(|e| domain::map_store_err("approve_proposal", e))?;

        audit
            .record_in_tx(
                &tx,
                NewAgentAction {
                    story_id: &proposal.story_id,
                    agent_id: &ctx.agent_id,
                    tool_name: "approve_proposal",
                    args_json: &serde_json::to_string(&parsed).unwrap_or_default(),
                    result_summary: Some(&format!(
                        "committed proposal `{}` ({} actions replayed)",
                        parsed.proposal_id, n_replayed
                    )),
                    proposal_id: Some(&parsed.proposal_id),
                },
            )
            .map_err(|e| domain::map_store_err("approve_proposal", e))?;

        tx.commit()
            .map_err(|e| domain::map_store_err("approve_proposal", e))?;

        let n_audit = n_replayed + 1;
        domain::to_result(&ApproveProposalOutput {
            proposal_id: parsed.proposal_id,
            actions_replayed: n_replayed,
            audit_rows: n_audit,
        })
    }
}

// We need a `registry` field on ApproveProposalTool that the
// other tools don't have. Restructure: keep it as a separate
// field; it's set via `with_registry` after construction. The
// `#[async_trait]` impl above uses `self.registry`; we add the
// field here.
impl ApproveProposalTool {
    pub fn with_registry(mut self, registry: Arc<ToolRegistry>) -> Self {
        self.registry = registry;
        self
    }
}

// ---- reject_proposal ---------------------------------------------------

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RejectProposalArgs {
    pub proposal_id: String,
    #[serde(default)]
    pub decided_by: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RejectProposalOutput {
    pub proposal_id: String,
    pub status: ProposalStatus,
}

pub struct RejectProposalTool {
    world: Arc<sagaline_store::World>,
}

impl RejectProposalTool {
    pub fn new(world: Arc<sagaline_store::World>) -> Self {
        Self { world }
    }
}

#[async_trait]
impl Tool for RejectProposalTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<RejectProposalArgs>(
            "reject_proposal",
            "Mark a pending proposal as `rejected`. The rows in \
             `proposal_actions` stay around for audit; the \
             world DB is NOT mutated. Only `pending` proposals \
             can be rejected; an already-committed or \
             already-rejected proposal returns an error. \
             Returns `{proposal_id, status}`.",
            Capability::Mutate,
        )
    }

    async fn execute(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: RejectProposalArgs = domain::parse_args("reject_proposal", args)?;
        let proposals = ProposalRepo::new(&self.world);
        let audit = AgentActionRepo::new(&self.world);

        let proposal = proposals
            .get(&parsed.proposal_id)
            .map_err(|e| domain::map_store_err("reject_proposal", e))?
            .ok_or_else(|| ToolError::Execution {
                name: "reject_proposal".into(),
                source: Box::new(std::io::Error::other(format!(
                    "proposal `{}` not found",
                    parsed.proposal_id
                ) as String)),
            })?;
        if proposal.status != ProposalStatus::Pending {
            return Err(ToolError::BadArgs {
                name: "reject_proposal".into(),
                message: format!(
                    "proposal `{}` is `{}` (not `pending`)",
                    parsed.proposal_id,
                    proposal.status.as_str()
                ),
            });
        }

        let decided_by = parsed.decided_by.as_deref().unwrap_or("user").to_string();
        let updated = proposals
            .set_status(&parsed.proposal_id, ProposalStatus::Rejected, &decided_by)
            .map_err(|e| domain::map_store_err("reject_proposal", e))?;
        if !updated {
            return Err(ToolError::Execution {
                name: "reject_proposal".into(),
                source: Box::new(std::io::Error::other(
                    "set_status affected 0 rows; concurrent change?".to_string(),
                )),
            });
        }

        let _ = audit
            .record(NewAgentAction {
                story_id: &proposal.story_id,
                agent_id: &ctx.agent_id,
                tool_name: "reject_proposal",
                args_json: &serde_json::to_string(&parsed).unwrap_or_default(),
                result_summary: Some(&format!("rejected proposal `{}`", parsed.proposal_id)),
                proposal_id: Some(&parsed.proposal_id),
            })
            .map_err(|e| domain::map_store_err("reject_proposal", e))?;

        domain::to_result(&RejectProposalOutput {
            proposal_id: parsed.proposal_id,
            status: ProposalStatus::Rejected,
        })
    }
}

/// Record a single tool call as a pending proposal. Used by
/// `Agent::dispatch_tool` when `commit_policy = Manual`. The
/// tool's side-effects do NOT happen — the world DB is
/// untouched; the user calls `approve_proposal` to land the
/// change.
///
/// `story_id` may be `None` when the tool's args don't carry
/// one (e.g. `propose_change` itself, or top-level utilities).
/// In that case the proposal row's `story_id` is empty string
/// and the agent's `approve_proposal` will fail until the
/// user attaches a story_id — this is intentional, surface it
/// in the UI rather than silently picking a story.
pub fn record_single_action(
    world: &sagaline_store::World,
    agent_id: &str,
    story_id: Option<&str>,
    summary: &str,
    tool_name: &str,
    args: &Value,
) -> Result<String, sagaline_store::StoreError> {
    use sagaline_store::NewAgentAction;

    let mut conn = world.conn()?;
    let tx = conn.transaction()?;

    let proposal_id = world.proposals().new_id();
    let sid = story_id.unwrap_or("");
    let now = sagaline_store::time_util::now_iso();
    tx.execute(
        "INSERT INTO proposals
            (id, story_id, agent_id, status, summary, diff_json,
             created_at, decided_at, decided_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL)",
        rusqlite::params![
            proposal_id,
            sid,
            agent_id,
            ProposalStatus::Pending.as_str(),
            summary,
            "",
            now,
        ],
    )?;
    let args_json = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
    tx.execute(
        "INSERT INTO proposal_actions (proposal_id, seq, tool_name, tool_args)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![proposal_id, 1i64, tool_name, args_json],
    )?;
    tx.commit()?;

    // Audit row outside the tx (separate connection write;
    // matches how the tool itself records its own rows).
    if !sid.is_empty() {
        let _ = world.agent_actions().record(NewAgentAction {
            story_id: sid,
            agent_id,
            tool_name: "record_single_action",
            args_json: &serde_json::to_string(&serde_json::json!({
                "proposal_id": proposal_id,
                "tool_name": tool_name,
            }))
            .unwrap_or_default(),
            result_summary: Some(&format!("queued `{tool_name}` as proposal `{proposal_id}`")),
            proposal_id: Some(&proposal_id),
        })?;
    }

    Ok(proposal_id)
}
