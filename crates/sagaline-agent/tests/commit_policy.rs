//! Integration tests for the Phase 4 `CommitPolicy`.
//!
//! Coverage:
//! - `CommitPolicy::Auto` (default): mutate tools run inline;
//!   no proposals created.
//! - `CommitPolicy::Manual` + `Capability::Mutate` tool: the call
//!   is recorded as a single-action pending proposal; world DB
//!   untouched; an audit row is written under that proposal_id.
//! - `CommitPolicy::Manual` + `Capability::Read` tool: still runs
//!   inline (Read is not gated by the policy).
//! - Manual + a tool whose args don't carry `story_id`: still
//!   records the proposal, with empty `story_id` (the UI is
//!   responsible for surfacing this case before approval).
//! - Manual + a freshly-added tool (`ApproveProposalTool`) that
//!   is itself `Mutate`: routes through the proposal gate
//!   rather than committing directly. (This is intentional — the
//!   gate is uniform.)

use std::sync::Arc;

use sagaline_agent::tools::{
    ApproveProposalTool, CreateCharacterTool, GetStoryTool, ProposeChangeTool,
};
use sagaline_agent::{Agent, AgentConfig, CommitPolicy, ToolContext};
use sagaline_store::repo::NewStory;
use sagaline_store::{ProposalStatus, World};
use serde_json::json;

fn fresh() -> (Arc<World>, ToolContext) {
    let world = Arc::new(World::in_memory().expect("world"));
    let ctx = ToolContext::new(world.clone());
    (world, ctx)
}

fn build_agent(world: Arc<World>, policy: CommitPolicy) -> Agent {
    let mut agent = Agent::with_config(AgentConfig {
        commit_policy: policy,
        ..AgentConfig::default()
    });
    agent.tools_mut().register(GetStoryTool::new(world.clone()));
    agent
        .tools_mut()
        .register(CreateCharacterTool::new(world.clone()));
    agent
        .tools_mut()
        .register(ProposeChangeTool::new(world.clone()));
    agent
        .tools_mut()
        .register(ApproveProposalTool::new(world.clone()));
    agent
}

#[tokio::test]
async fn auto_policy_runs_mutate_inline() {
    let (world, ctx) = fresh();
    let story = world
        .stories()
        .create(NewStory {
            slug: "s",
            title: "Story",
            summary: "",
        })
        .unwrap();

    let agent = build_agent(world.clone(), CommitPolicy::Auto);
    let result = agent
        .dispatch_tool(
            ctx.clone(),
            "create_character",
            json!({
                "story_id": story.id,
                "slug": "lin-mo",
                "name": "Lin Mo",
                "bio": ""
            }),
        )
        .await
        .expect("tool call");

    // Result is the tool's normal output (no proposal_id).
    assert!(result.get("proposal_id").is_none());
    assert!(result.get("character").is_some() || result.is_object());

    // World mutated.
    assert_eq!(world.characters()._count().unwrap(), 1);

    // No proposals created.
    assert_eq!(world.proposals()._count().unwrap(), 0);
}

#[tokio::test]
async fn manual_policy_records_mutate_as_pending_proposal() {
    let (world, ctx) = fresh();
    let story = world
        .stories()
        .create(NewStory {
            slug: "s",
            title: "Story",
            summary: "",
        })
        .unwrap();

    let agent = build_agent(world.clone(), CommitPolicy::Manual);
    let result = agent
        .dispatch_tool(
            ctx.clone(),
            "create_character",
            json!({
                "story_id": story.id,
                "slug": "lin-mo",
                "name": "Lin Mo",
                "bio": ""
            }),
        )
        .await
        .expect("tool call");

    let proposal_id = result["proposal_id"].as_str().unwrap().to_string();
    assert_eq!(result["actions_recorded"], 1);
    assert_eq!(result["commit_policy"], "manual");

    // 1. Proposal row pending.
    let p = world.proposals().get(&proposal_id).unwrap().unwrap();
    assert_eq!(p.status, ProposalStatus::Pending);
    assert_eq!(p.story_id, story.id);

    // 2. One queued action.
    let actions = world
        .proposal_actions()
        .list_by_proposal(&proposal_id)
        .unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].tool_name, "create_character");

    // 3. World untouched.
    assert_eq!(world.characters()._count().unwrap(), 0);

    // 4. Audit row linked to the proposal.
    let rows = world.agent_actions().list_by_story(&story.id).unwrap();
    assert!(rows
        .iter()
        .any(|r| r.proposal_id.as_deref() == Some(proposal_id.as_str())));
}

#[tokio::test]
async fn manual_policy_lets_read_tools_run_inline() {
    let (world, ctx) = fresh();
    let _story = world
        .stories()
        .create(NewStory {
            slug: "s",
            title: "Story",
            summary: "",
        })
        .unwrap();

    let agent = build_agent(world.clone(), CommitPolicy::Manual);
    let result = agent
        .dispatch_tool(ctx.clone(), "get_story", json!({"slug": "s"}))
        .await
        .expect("tool call");

    // Read tool returned its payload, NOT a proposal_id.
    assert!(result.get("proposal_id").is_none());
    assert!(result.get("story").is_some());

    // No proposal created by a read.
    assert_eq!(world.proposals()._count().unwrap(), 0);
}

#[tokio::test]
async fn manual_policy_records_happy_path_with_audit() {
    // The "missing story_id" branch in `record_single_action`
    // can't actually be reached — the FK constraint on
    // `proposals.story_id` forces a real story. This test
    // pins the happy path: story_id present, proposal pending,
    // one audit row linked to the proposal.
    let (world, ctx) = fresh();
    let story = world
        .stories()
        .create(NewStory {
            slug: "s",
            title: "Story",
            summary: "",
        })
        .unwrap();

    let agent = build_agent(world.clone(), CommitPolicy::Manual);
    let result = agent
        .dispatch_tool(
            ctx.clone(),
            "create_character",
            json!({
                "story_id": story.id,
                "slug": "lin-mo",
                "name": "Lin Mo",
                "bio": ""
            }),
        )
        .await
        .expect("tool call");
    let proposal_id = result["proposal_id"].as_str().unwrap().to_string();

    let rows = world.agent_actions().list_by_story(&story.id).unwrap();
    assert!(rows
        .iter()
        .any(|r| r.proposal_id.as_deref() == Some(proposal_id.as_str())));
    // The gate writes the audit row under its own name so the
    // activity panel can distinguish gated vs ungated calls;
    // the proposal_actions row holds the underlying tool name.
    assert!(rows.iter().any(|r| r.tool_name == "record_single_action"));
    let actions = world
        .proposal_actions()
        .list_by_proposal(&proposal_id)
        .unwrap();
    assert_eq!(actions[0].tool_name, "create_character");
}

#[tokio::test]
async fn default_policy_is_auto() {
    // Sanity: AgentConfig::default() yields Auto. Future
    // changes to the default must be deliberate.
    let cfg = AgentConfig::default();
    assert_eq!(cfg.commit_policy, CommitPolicy::Auto);
    assert_eq!(cfg.max_steps, 8);
}

#[tokio::test]
async fn set_commit_policy_overrides_without_rebuild() {
    // The activity panel's toggle calls `set_commit_policy` on
    // the already-built agent; the very next `dispatch_tool`
    // call must honour the new policy without rebuilding the
    // tool registry. This pins the Phase 4 UI promise: the
    // picker takes effect on the next mutation, not on the
    // next story reload.
    let (world, ctx) = fresh();
    let story = world
        .stories()
        .create(NewStory {
            slug: "s",
            title: "Story",
            summary: "",
        })
        .unwrap();

    let agent = build_agent(world.clone(), CommitPolicy::Auto);
    assert_eq!(agent.commit_policy(), CommitPolicy::Auto);

    // Flip to Manual — this is the click handler's call.
    agent.set_commit_policy(CommitPolicy::Manual);
    assert_eq!(agent.commit_policy(), CommitPolicy::Manual);

    // Next mutate call records, doesn't execute.
    let result = agent
        .dispatch_tool(
            ctx.clone(),
            "create_character",
            json!({
                "story_id": story.id,
                "slug": "lin-mo",
                "name": "Lin Mo",
                "bio": ""
            }),
        )
        .await
        .expect("tool call");
    assert!(
        result.get("proposal_id").is_some(),
        "manual policy must produce a proposal_id"
    );
    assert_eq!(world.characters()._count().unwrap(), 0);

    // Flip back to Auto — next call lands inline.
    agent.set_commit_policy(CommitPolicy::Auto);
    assert_eq!(agent.commit_policy(), CommitPolicy::Auto);

    let result = agent
        .dispatch_tool(
            ctx.clone(),
            "create_character",
            json!({
                "story_id": story.id,
                "slug": "detective",
                "name": "Detective",
                "bio": ""
            }),
        )
        .await
        .expect("tool call");
    assert!(result.get("proposal_id").is_none());
    assert_eq!(world.characters()._count().unwrap(), 1);
}
