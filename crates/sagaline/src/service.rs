//! `StoryService` adapter from `AppEnv`.
//!
//! The UI crate never depends on this binary crate, so the
//! `StoryServiceSlot` global the binary installs is the bridge:
//! the UI looks up `Box<dyn StoryService>` (through the
//! `StoryServiceSlot` newtype) and calls through it.
//!
//! [`AppEnvProposalService`] is the sibling the activity panel's
//! proposal picker talks through. It owns the live
//! [`CommitPolicy`] toggle (mirrored into `SAGALINE_COMMIT_POLICY`
//! for restart durability) and dispatches `approve_proposal` /
//! `reject_proposal` against the agent's registered tool
//! registry — the same registry `build_agent` constructed, so the
//! replay loop + audit log run identically to the tool-call path.

use std::path::PathBuf;
use std::sync::Arc;

use sagaline_agent::{CommitPolicy, ToolContext};
use sagaline_core::{ProjectLocation, Story, StorySummary};
use sagaline_store::{ProposalActionRepo, ProposalRepo, ProposalRow, ProposalStatus};
use sagaline_ui::{ProposalService, StoryService};

use crate::env::AppEnv;

/// Thin adapter that implements [`StoryService`] for an
/// `Arc<AppEnv>`. Cheap to clone — the underlying `AppEnv` is
/// already `Arc`-shared, and `set_project_location` is the only
/// mutating call (it goes through `AppEnv::set_project_location`
/// which is internally synchronised).
#[derive(Clone)]
pub struct AppEnvStoryService(pub Arc<AppEnv>);

impl AppEnvStoryService {
    pub fn new(env: Arc<AppEnv>) -> Self {
        Self(env)
    }
}

impl StoryService for AppEnvStoryService {
    fn create_story(&self, title: &str) -> Result<Arc<Story>, String> {
        let store = self
            .0
            .story_store()
            .ok_or_else(|| "Pick a project location first (⌘ ,).".to_string())?;
        // The store is `Arc<dyn StoryStore>`; `create` is sync.
        // The UI wraps the call in `spawn_blocking`, so we just
        // call through here.
        store.create(title).map_err(|e| e.to_string())
    }

    fn project_location_display(&self) -> Option<String> {
        self.0.story_location().map(|p| p.display())
    }

    fn set_project_location(&self, path: PathBuf) -> Result<(), String> {
        self.0
            .set_project_location(ProjectLocation::new(path))
            .map_err(|e| e.to_string())
    }

    fn recent_stories(&self) -> Vec<StorySummary> {
        self.0
            .story_store()
            .and_then(|s| s.list().ok())
            .unwrap_or_default()
    }

    fn clone_service(&self) -> Box<dyn StoryService> {
        Box::new(self.clone())
    }
}

/// Thin adapter that implements [`ProposalService`] for an
/// `Arc<AppEnv>`. The activity panel's picker reaches it
/// through the [`sagaline_ui::ProposalServiceSlot`] gpui global.
#[derive(Clone)]
pub struct AppEnvProposalService(pub Arc<AppEnv>);

impl AppEnvProposalService {
    pub fn new(env: Arc<AppEnv>) -> Self {
        Self(env)
    }
}

impl ProposalService for AppEnvProposalService {
    fn current_commit_policy(&self) -> CommitPolicy {
        self.0.commit_policy()
    }

    fn set_commit_policy(&self, policy: CommitPolicy) {
        // `AppEnv::set_commit_policy` also pushes the new value
        // into the running agent (if any) so its next
        // `dispatch_tool` call honours it, and mirrors the value
        // into `SAGALINE_COMMIT_POLICY` for restart durability.
        self.0.set_commit_policy(policy);
    }

    fn list_pending_proposals(
        &self,
        story_id: Option<&str>,
    ) -> Result<Vec<ProposalRow>, String> {
        let repo = ProposalRepo::new(&self.0.store);
        repo.list(story_id, Some(ProposalStatus::Pending))
            .map_err(|e| e.to_string())
    }

    fn list_proposal_actions(
        &self,
        proposal_id: &str,
    ) -> Result<Vec<sagaline_store::ProposalActionRow>, String> {
        let repo = ProposalActionRepo::new(&self.0.store);
        repo.list_by_proposal(proposal_id).map_err(|e| e.to_string())
    }

    fn approve_proposal(&self, proposal_id: &str, decided_by: &str) -> Result<usize, String> {
        let agent = self
            .0
            .agent()
            .ok_or_else(|| "agent not built yet — open a story first".to_string())?;
        let registry = agent.tools();
        let tool = registry
            .get("approve_proposal")
            .ok_or_else(|| "approve_proposal tool not registered".to_string())?;
        let ctx = ToolContext::new(self.0.store.clone());
        let args = serde_json::json!({
            "proposal_id": proposal_id,
            "decided_by": decided_by,
        });
        // `Tool::execute` is async (async_trait produces a
        // `Send + 'static` future). The UI dispatches this
        // method on a Tokio worker thread (via
        // `bridge.spawn_blocking`), which is not itself a
        // Tokio-runtime context — so we drive the future with
        // `futures::executor::block_on`, a pure synchronous
        // executor that does not interfere with any Tokio
        // runtime. The `Send` bound on async_trait's future
        // output is what makes this safe.
        let result = futures::executor::block_on(async move {
            tool.execute(ctx, args).await
        })
        .map_err(|e| format!("approve_proposal failed: {e}"))?;
        let actions_replayed = result
            .get("actions_replayed")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        Ok(actions_replayed)
    }

    fn reject_proposal(&self, proposal_id: &str, decided_by: &str) -> Result<(), String> {
        let agent = self
            .0
            .agent()
            .ok_or_else(|| "agent not built yet — open a story first".to_string())?;
        let registry = agent.tools();
        let tool = registry
            .get("reject_proposal")
            .ok_or_else(|| "reject_proposal tool not registered".to_string())?;
        let ctx = ToolContext::new(self.0.store.clone());
        let args = serde_json::json!({
            "proposal_id": proposal_id,
            "decided_by": decided_by,
        });
        futures::executor::block_on(async move { tool.execute(ctx, args).await })
            .map_err(|e| format!("reject_proposal failed: {e}"))?;
        Ok(())
    }

    fn clone_service(&self) -> Box<dyn ProposalService> {
        Box::new(self.clone())
    }
}
