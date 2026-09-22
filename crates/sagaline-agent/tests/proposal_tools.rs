//! Integration tests for the Phase 3 Proposal → Commit tools.
//!
//! Coverage:
//! - `propose_change` records actions but doesn't mutate the world
//! - `list_pending_proposals` sees the queued batch
//! - `approve_proposal` validates → replays → flips to `committed`
//! - `reject_proposal` flips pending → `rejected`
//! - The audit log (`agent_actions`) records every call
//! - Re-approving a non-pending proposal errors
//! - `by_capability()` groups the new tools

use std::sync::Arc;

use sagaline_agent::tools::{
    AddCharacterAgeTool, ApproveProposalTool, CreateChapterTool, CreateCharacterTool,
    CreateShotTool, ListPendingProposalsTool, ProposeChangeTool, RejectProposalTool,
};
use sagaline_agent::{Capability, Tool, ToolContext, ToolRegistry};
use sagaline_store::repo::NewStory;
use sagaline_store::{NewProposal, NewProposalAction, ProposalStatus, World};
use serde_json::json;

fn fresh() -> (Arc<World>, ToolContext) {
    let world = Arc::new(World::in_memory().expect("world"));
    let ctx = ToolContext::new(world.clone());
    (world, ctx)
}

fn fresh_registry() -> (Arc<World>, ToolContext, ToolRegistry) {
    let (w, c) = fresh();
    (w, c, ToolRegistry::new())
}

async fn run(tool: &dyn Tool, ctx: ToolContext, args: serde_json::Value) -> serde_json::Value {
    tool.execute(ctx, args).await.expect("tool call")
}

fn register_domain_tools(reg: &mut ToolRegistry, world: Arc<World>) {
    reg.register(CreateCharacterTool::new(world.clone()));
    reg.register(CreateChapterTool::new(world.clone()));
}

#[tokio::test]
async fn propose_records_actions_but_does_not_mutate_world() {
    let (world, ctx, mut reg) = fresh_registry();
    register_domain_tools(&mut reg, world.clone());

    let story = world
        .stories()
        .create(NewStory {
            slug: "s",
            title: "Story",
            summary: "",
        })
        .unwrap();

    let propose = ProposeChangeTool::new(world.clone());
    let out = run(
        &propose,
        ctx.clone(),
        json!({
            "story_id": story.id,
            "summary": "add Lin Mo",
            "actions": [
                {
                    "tool_name": "create_character",
                    "args": {
                        "story_id": story.id,
                        "slug": "lin-mo",
                        "name": "Lin Mo",
                        "bio": ""
                    }
                }
            ]
        }),
    )
    .await;

    let proposal_id = out["proposal_id"].as_str().unwrap().to_string();
    assert_eq!(out["actions_recorded"], 1);

    // 1. Proposal row exists with status=pending.
    let p = world.proposals().get(&proposal_id).unwrap().unwrap();
    assert_eq!(p.status, ProposalStatus::Pending);

    // 2. One queued action.
    let actions = world
        .proposal_actions()
        .list_by_proposal(&proposal_id)
        .unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].tool_name, "create_character");
    assert_eq!(actions[0].seq, 1);

    // 3. World was NOT mutated — no character exists yet.
    assert_eq!(world.characters()._count().unwrap(), 0);
}

#[tokio::test]
async fn list_pending_proposals_filters_by_story() {
    let (world, ctx, mut reg) = fresh_registry();
    register_domain_tools(&mut reg, world.clone());

    let story_a = world
        .stories()
        .create(NewStory {
            slug: "a",
            title: "A",
            summary: "",
        })
        .unwrap();
    let story_b = world
        .stories()
        .create(NewStory {
            slug: "b",
            title: "B",
            summary: "",
        })
        .unwrap();

    let propose = ProposeChangeTool::new(world.clone());
    run(
        &propose,
        ctx.clone(),
        json!({
            "story_id": story_a.id,
            "summary": "a1",
            "actions": [{"tool_name": "create_character", "args": {"story_id": story_a.id, "slug": "x", "name": "X", "bio": ""}}]
        }),
    )
    .await;
    run(
        &propose,
        ctx.clone(),
        json!({
            "story_id": story_b.id,
            "summary": "b1",
            "actions": [{"tool_name": "create_character", "args": {"story_id": story_b.id, "slug": "y", "name": "Y", "bio": ""}}]
        }),
    )
    .await;

    let list = ListPendingProposalsTool::new(world.clone());
    let all = run(&list, ctx.clone(), json!({})).await;
    assert_eq!(all["proposals"].as_array().unwrap().len(), 2);

    let by_story = run(&list, ctx.clone(), json!({"story_id": story_a.id})).await;
    assert_eq!(by_story["proposals"].as_array().unwrap().len(), 1);
    assert_eq!(by_story["proposals"][0]["story_id"], story_a.id);
}

#[tokio::test]
async fn approve_replays_actions_and_flips_status() {
    let (world, ctx, mut reg) = fresh_registry();
    register_domain_tools(&mut reg, world.clone());

    let story = world
        .stories()
        .create(NewStory {
            slug: "s",
            title: "Story",
            summary: "",
        })
        .unwrap();

    // 1. Propose: add a character + a chapter.
    let propose = ProposeChangeTool::new(world.clone());
    let out = run(
        &propose,
        ctx.clone(),
        json!({
            "story_id": story.id,
            "summary": "bootstrap",
            "actions": [
                {
                    "tool_name": "create_character",
                    "args": {"story_id": story.id, "slug": "lin-mo", "name": "Lin Mo", "bio": ""}
                },
                {
                    "tool_name": "create_chapter",
                    "args": {"story_id": story.id, "slug": "ch1", "ordinal": 1, "title": "Chapter 1", "synopsis": ""}
                }
            ]
        }),
    )
    .await;
    let proposal_id = out["proposal_id"].as_str().unwrap().to_string();
    assert_eq!(out["actions_recorded"], 2);

    // 2. Approve with a wired registry (so the replay can
    //    resolve the create_* tool names).
    let approve = ApproveProposalTool::new(world.clone()).with_registry(Arc::new(reg));
    let approve_out = run(
        &approve,
        ctx.clone(),
        json!({"proposal_id": proposal_id, "decided_by": "test-user"}),
    )
    .await;
    assert_eq!(approve_out["proposal_id"], proposal_id);
    assert_eq!(approve_out["actions_replayed"], 2);
    // 2 replays + 1 approve-call row = 3 audit rows.
    assert_eq!(approve_out["audit_rows"], 3);

    // 3. Proposal status is committed.
    let p = world.proposals().get(&proposal_id).unwrap().unwrap();
    assert_eq!(p.status, ProposalStatus::Committed);
    assert_eq!(p.decided_by.as_deref(), Some("test-user"));
    assert!(p.decided_at.is_some());

    // 4. World was actually mutated.
    assert_eq!(world.characters()._count().unwrap(), 1);

    // 5. Audit log has the propose + replay + approve rows
    //    (4 total: propose, replay create_character, replay
    //    create_chapter, approve).
    let rows = world.agent_actions().list_by_story(&story.id).unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].tool_name, "propose_change");
    assert_eq!(rows[1].tool_name, "create_character");
    assert_eq!(rows[2].tool_name, "create_chapter");
    assert_eq!(rows[3].tool_name, "approve_proposal");
    for r in &rows {
        assert_eq!(r.proposal_id.as_deref(), Some(proposal_id.as_str()));
    }
}

#[tokio::test]
async fn approve_rejects_non_pending() {
    let (world, ctx, mut reg) = fresh_registry();
    register_domain_tools(&mut reg, world.clone());

    let story = world
        .stories()
        .create(NewStory {
            slug: "s",
            title: "Story",
            summary: "",
        })
        .unwrap();

    let propose = ProposeChangeTool::new(world.clone());
    let out = run(
        &propose,
        ctx.clone(),
        json!({
            "story_id": story.id,
            "summary": "x",
            "actions": [{"tool_name": "create_character", "args": {"story_id": story.id, "slug": "x", "name": "X", "bio": ""}}]
        }),
    )
    .await;
    let proposal_id = out["proposal_id"].as_str().unwrap().to_string();

    // Reject first.
    let reject = RejectProposalTool::new(world.clone());
    let r = run(&reject, ctx.clone(), json!({"proposal_id": proposal_id})).await;
    assert_eq!(r["status"], "rejected");

    // Now try to approve.
    let approve = ApproveProposalTool::new(world.clone());
    let err = approve
        .execute(ctx.clone(), json!({"proposal_id": proposal_id}))
        .await
        .expect_err("expected approve to fail on rejected proposal");
    assert!(format!("{err}").contains("not `pending`"));
}

#[tokio::test]
async fn reject_flips_status_and_leaves_world_intact() {
    let (world, ctx, _reg) = fresh_registry();

    let story = world
        .stories()
        .create(NewStory {
            slug: "s",
            title: "Story",
            summary: "",
        })
        .unwrap();

    let propose = ProposeChangeTool::new(world.clone());
    let out = run(
        &propose,
        ctx.clone(),
        json!({
            "story_id": story.id,
            "summary": "x",
            "actions": [{"tool_name": "create_character", "args": {"story_id": story.id, "slug": "x", "name": "X", "bio": ""}}]
        }),
    )
    .await;
    let proposal_id = out["proposal_id"].as_str().unwrap().to_string();

    let reject = RejectProposalTool::new(world.clone());
    let r = run(
        &reject,
        ctx.clone(),
        json!({"proposal_id": proposal_id, "decided_by": "user-1"}),
    )
    .await;
    assert_eq!(r["status"], "rejected");

    let p = world.proposals().get(&proposal_id).unwrap().unwrap();
    assert_eq!(p.status, ProposalStatus::Rejected);
    assert_eq!(p.decided_by.as_deref(), Some("user-1"));

    // World untouched.
    assert_eq!(world.characters()._count().unwrap(), 0);

    // Audit row exists.
    let rows = world.agent_actions().list_by_story(&story.id).unwrap();
    let reject_row = rows
        .iter()
        .find(|r| r.tool_name == "reject_proposal")
        .unwrap();
    assert_eq!(
        reject_row.proposal_id.as_deref(),
        Some(proposal_id.as_str())
    );
}

#[tokio::test]
async fn approve_with_unknown_tool_in_action_errors() {
    let (world, ctx, mut reg) = fresh_registry();
    register_domain_tools(&mut reg, world.clone());

    let story = world
        .stories()
        .create(NewStory {
            slug: "s",
            title: "Story",
            summary: "",
        })
        .unwrap();

    // Manually insert a proposal whose action references a tool
    // that's NOT in the registry (don't go through propose_change,
    // which would reject it at the JSON-schema layer).
    let proposal = world
        .proposals()
        .create(NewProposal {
            story_id: &story.id,
            agent_id: &ctx.agent_id,
            summary: "bogus",
            diff_json: "",
        })
        .unwrap();
    world
        .proposal_actions()
        .record(NewProposalAction {
            proposal_id: &proposal.id,
            seq: 1,
            tool_name: "no_such_tool",
            tool_args_json: "{}",
        })
        .unwrap();

    let approve = ApproveProposalTool::new(world.clone()).with_registry(Arc::new(reg));
    let err = approve
        .execute(ctx.clone(), json!({"proposal_id": proposal.id}))
        .await
        .expect_err("expected unknown-tool error");
    assert!(format!("{err}").contains("no_such_tool"));

    // Status stays pending because the failure rolled back.
    let p = world.proposals().get(&proposal.id).unwrap().unwrap();
    assert_eq!(p.status, ProposalStatus::Pending);
}

#[tokio::test]
async fn by_capability_groups_new_tools() {
    let (world, _ctx, mut reg) = fresh_registry();
    register_domain_tools(&mut reg, world.clone());
    reg.register(ProposeChangeTool::new(world.clone()));
    reg.register(ApproveProposalTool::new(world.clone()));
    reg.register(ListPendingProposalsTool::new(world.clone()));
    reg.register(RejectProposalTool::new(world.clone()));

    let groups = reg.by_capability();
    let mutate = groups.get(&Capability::Mutate).unwrap();
    assert!(mutate.contains(&"propose_change".to_string()));
    assert!(mutate.contains(&"approve_proposal".to_string()));
    assert!(mutate.contains(&"reject_proposal".to_string()));

    let read = groups.get(&Capability::Read).unwrap();
    assert!(read.contains(&"list_pending_proposals".to_string()));
}

// ---- Phase 5: single-transaction atomicity -------------------------------

#[tokio::test]
async fn approve_atomic_rolls_back_on_mid_failure() {
    // Three valid in-tx actions, but action #2 has a slug that
    // collides with action #1. The `create_character` tool's
    // UNIQUE(slug) constraint fires mid-batch. Phase 5's
    // single-transaction wrapper must roll back action #1's
    // INSERT, leave the world DB at zero characters, and keep
    // the proposal `pending` (no `committed` flip, no
    // approve-call audit row).
    let (world, ctx, mut reg) = fresh_registry();
    register_domain_tools(&mut reg, world.clone());

    let story = world
        .stories()
        .create(NewStory {
            slug: "s",
            title: "Story",
            summary: "",
        })
        .unwrap();

    let propose = ProposeChangeTool::new(world.clone());
    let out = run(
        &propose,
        ctx.clone(),
        json!({
            "story_id": story.id,
            "summary": "atomic rollback test",
            "actions": [
                {
                    "tool_name": "create_character",
                    "args": {"story_id": story.id, "slug": "alpha", "name": "Alpha", "bio": ""}
                },
                {
                    "tool_name": "create_character",
                    "args": {"story_id": story.id, "slug": "alpha", "name": "Alpha Dup", "bio": ""}
                },
                {
                    "tool_name": "create_character",
                    "args": {"story_id": story.id, "slug": "beta", "name": "Beta", "bio": ""}
                }
            ]
        }),
    )
    .await;
    let proposal_id = out["proposal_id"].as_str().unwrap().to_string();

    let approve = ApproveProposalTool::new(world.clone()).with_registry(Arc::new(reg));
    let err = approve
        .execute(ctx.clone(), json!({"proposal_id": proposal_id}))
        .await
        .expect_err("expected approve to fail on duplicate slug");
    let msg = format!("{err}");
    assert!(
        msg.contains("create_character") && msg.contains("failed"),
        "expected in-batch failure message, got: {msg}"
    );

    // 1. World DB is unchanged: zero characters. The duplicate
    //    `alpha` did not leak through, and action #3's `beta`
    //    was rolled back too.
    assert_eq!(world.characters()._count().unwrap(), 0);
    assert_eq!(
        world.characters().list_for_story(&story.id).unwrap().len(),
        0
    );

    // 2. Proposal status stays `pending` — the failure rolled
    //    back the `set_status_in_tx` flip too.
    let p = world.proposals().get(&proposal_id).unwrap().unwrap();
    assert_eq!(p.status, ProposalStatus::Pending);
    assert!(p.decided_at.is_none());

    // 3. Audit log has only the propose row; the three replay
    //    audit rows and the approve-call row were all written
    //    inside the rolled-back tx, so they did not survive.
    let rows = world.agent_actions().list_by_story(&story.id).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].tool_name, "propose_change");
}

#[tokio::test]
async fn approve_atomic_commits_all_on_success() {
    // Three valid in-tx actions, no failure. Phase 5 must
    // commit all three replay rows + the approve-call row in
    // a single outer transaction.
    let (world, ctx, mut reg) = fresh_registry();
    register_domain_tools(&mut reg, world.clone());

    let story = world
        .stories()
        .create(NewStory {
            slug: "s",
            title: "Story",
            summary: "",
        })
        .unwrap();

    let propose = ProposeChangeTool::new(world.clone());
    let out = run(
        &propose,
        ctx.clone(),
        json!({
            "story_id": story.id,
            "summary": "atomic commit test",
            "actions": [
                {
                    "tool_name": "create_character",
                    "args": {"story_id": story.id, "slug": "alpha", "name": "Alpha", "bio": ""}
                },
                {
                    "tool_name": "create_character",
                    "args": {"story_id": story.id, "slug": "beta", "name": "Beta", "bio": ""}
                },
                {
                    "tool_name": "create_character",
                    "args": {"story_id": story.id, "slug": "gamma", "name": "Gamma", "bio": ""}
                }
            ]
        }),
    )
    .await;
    let proposal_id = out["proposal_id"].as_str().unwrap().to_string();

    let approve = ApproveProposalTool::new(world.clone()).with_registry(Arc::new(reg));
    let approve_out = run(
        &approve,
        ctx.clone(),
        json!({"proposal_id": proposal_id, "decided_by": "test-user"}),
    )
    .await;
    assert_eq!(approve_out["proposal_id"], proposal_id);
    assert_eq!(approve_out["actions_replayed"], 3);
    assert_eq!(approve_out["audit_rows"], 4); // 3 replays + 1 approve-call row

    // 1. All three characters landed.
    let chars = world.characters().list_for_story(&story.id).unwrap();
    assert_eq!(chars.len(), 3);
    let slugs: Vec<&str> = chars.iter().map(|c| c.slug.as_str()).collect();
    assert!(slugs.contains(&"alpha"));
    assert!(slugs.contains(&"beta"));
    assert!(slugs.contains(&"gamma"));

    // 2. Proposal moved to `committed`.
    let p = world.proposals().get(&proposal_id).unwrap().unwrap();
    assert_eq!(p.status, ProposalStatus::Committed);
    assert_eq!(p.decided_by.as_deref(), Some("test-user"));
    assert!(p.decided_at.is_some());

    // 3. Audit log: 4 rows. Order is the insertion order:
    //    propose_change, replay create_character alpha, replay
    //    beta, replay gamma, then approve_proposal. The replay
    //    rows must use the action's stored args (not the
    //    post-validation input).
    let rows = world.agent_actions().list_by_story(&story.id).unwrap();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].tool_name, "propose_change");
    assert_eq!(rows[1].tool_name, "create_character");
    assert_eq!(rows[2].tool_name, "create_character");
    assert_eq!(rows[3].tool_name, "create_character");
    let replay_rows: Vec<_> = rows[1..4].iter().collect();
    let mut replay_slugs: Vec<&str> = replay_rows
        .iter()
        .filter_map(|r| {
            serde_json::from_str::<serde_json::Value>(&r.args_json)
                .ok()
                .and_then(|v| {
                    v.get("slug")
                        .and_then(|s| s.as_str())
                        .map(|s| s.to_string())
                })
        })
        .map(|s| Box::leak(s.into_boxed_str()) as &str)
        .collect();
    replay_slugs.sort();
    assert_eq!(replay_slugs, vec!["alpha", "beta", "gamma"]);
}

// ---- Phase 5 follow-up: the remaining Mutate tools opt into execute_in_tx.
//
// `update_character`, `add_character_age`, `add_character_appearance`,
// `assign_character_to_scene`, `assign_environment_to_scene`, and `create_shot`
// each declared `supports_in_tx() = true` this phase. The tests below
// prove the in-tx path is actually wired up: approve_proposal routes
// them through `execute_in_tx` (so they participate in the single-
// transaction wrapper), and a mid-batch failure still rolls the
// batch back to the pre-approve world state.

#[tokio::test]
async fn approve_age_tool_rolls_back_on_duplicate_age() {
    // Three `add_character_age` actions for the same character;
    // actions #1 and #2 share `age = 10`. The UNIQUE(character_id,
    // age) constraint fires mid-batch. Phase 5 must roll action #1
    // back, leave zero character_ages rows, and keep the proposal
    // `pending`.
    let (world, ctx, mut reg) = fresh_registry();
    register_domain_tools(&mut reg, world.clone());
    reg.register(AddCharacterAgeTool::new(world.clone()));

    let story = world
        .stories()
        .create(NewStory {
            slug: "s",
            title: "Story",
            summary: "",
        })
        .unwrap();
    let character = world
        .characters()
        .create(sagaline_store::repo::NewCharacter {
            story_id: &story.id,
            slug: "lin",
            name: "Lin",
            occupation: None,
            bio: "",
        })
        .unwrap();

    let propose = ProposeChangeTool::new(world.clone());
    let out = run(
        &propose,
        ctx.clone(),
        json!({
            "story_id": story.id,
            "summary": "age duplicate test",
            "actions": [
                { "tool_name": "add_character_age", "args": {
                    "character_id": character.id, "age": 10, "note": "ten" } },
                { "tool_name": "add_character_age", "args": {
                    "character_id": character.id, "age": 10, "note": "ten-again" } },
                { "tool_name": "add_character_age", "args": {
                    "character_id": character.id, "age": 20, "note": "twenty" } }
            ]
        }),
    )
    .await;
    let proposal_id = out["proposal_id"].as_str().unwrap().to_string();

    let approve = ApproveProposalTool::new(world.clone()).with_registry(Arc::new(reg));
    let err = approve
        .execute(ctx.clone(), json!({"proposal_id": proposal_id}))
        .await
        .expect_err("expected approve to fail on duplicate age");
    let msg = format!("{err}");
    assert!(
        msg.contains("add_character_age") && msg.contains("failed"),
        "expected in-batch failure message, got: {msg}"
    );

    // World DB: zero character_ages rows. Action #1 (age=10) rolled
    // back; action #3 (age=20) never landed either.
    let conn = world.conn().unwrap();
    let n_ages: i64 = conn
        .query_row("SELECT COUNT(*) FROM character_ages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n_ages, 0, "duplicate-age rollback must leave 0 ages");

    // Proposal stays pending.
    let p = world.proposals().get(&proposal_id).unwrap().unwrap();
    assert_eq!(p.status, ProposalStatus::Pending);

    // Audit log: only the propose row survived.
    let rows = world.agent_actions().list_by_story(&story.id).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].tool_name, "propose_change");
}

#[tokio::test]
async fn approve_shot_tool_commits_via_execute_in_tx() {
    // Three valid `create_shot` actions under a fresh scene. None
    // fail, so the in-tx path must commit all three replay rows +
    // the approve-call audit row in a single transaction. This
    // proves `create_shot`'s `execute_in_tx` is actually wired up.
    let (world, ctx, mut reg) = fresh_registry();
    register_domain_tools(&mut reg, world.clone());
    reg.register(CreateShotTool::new(world.clone()));

    let story = world
        .stories()
        .create(NewStory {
            slug: "s",
            title: "Story",
            summary: "",
        })
        .unwrap();
    let chapter = world
        .scenes()
        .create_chapter(sagaline_store::repo::NewChapter {
            story_id: &story.id,
            slug: "ch1",
            ordinal: 1,
            title: "Chapter 1",
            synopsis: "",
        })
        .unwrap();
    let scene = world
        .scenes()
        .create_scene(sagaline_store::repo::NewScene {
            chapter_id: &chapter.id,
            slug: "sc1",
            ordinal: 1,
            title: "Scene 1",
            synopsis: "",
        })
        .unwrap();

    let propose = ProposeChangeTool::new(world.clone());
    let out = run(
        &propose,
        ctx.clone(),
        json!({
            "story_id": story.id,
            "summary": "three shots",
            "actions": [
                { "tool_name": "create_shot", "args": {
                    "scene_id": scene.id, "slug": "s1", "ordinal": 1,
                    "title": "Shot 1", "prompt": "" } },
                { "tool_name": "create_shot", "args": {
                    "scene_id": scene.id, "slug": "s2", "ordinal": 2,
                    "title": "Shot 2", "prompt": "" } },
                { "tool_name": "create_shot", "args": {
                    "scene_id": scene.id, "slug": "s3", "ordinal": 3,
                    "title": "Shot 3", "prompt": "" } }
            ]
        }),
    )
    .await;
    let proposal_id = out["proposal_id"].as_str().unwrap().to_string();

    let approve = ApproveProposalTool::new(world.clone()).with_registry(Arc::new(reg));
    let approve_out = approve
        .execute(ctx.clone(), json!({"proposal_id": proposal_id}))
        .await
        .expect("approve should commit all 3 shots");
    assert_eq!(approve_out["actions_replayed"], 3);
    assert_eq!(approve_out["audit_rows"], 4);

    let conn = world.conn().unwrap();
    let n_shots: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM shots WHERE scene_id = ?1",
            rusqlite::params![scene.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n_shots, 3, "all 3 shots must land in the world DB");

    // Audit log: 1 propose + 3 replay + 1 approve-call = 5 rows.
    let rows = world.agent_actions().list_by_story(&story.id).unwrap();
    assert_eq!(rows.len(), 5);
    let replay_names: Vec<&str> = rows
        .iter()
        .filter(|r| r.tool_name == "create_shot")
        .map(|r| r.tool_name.as_str())
        .collect();
    assert_eq!(replay_names.len(), 3);
}
