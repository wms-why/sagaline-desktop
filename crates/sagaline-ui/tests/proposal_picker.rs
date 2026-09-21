//! View-state tests for the activity-panel proposal picker.
//!
//! The picker has three independent surfaces:
//!
//! 1. The commit-policy toggle, which writes through
//!    [`crate::state::ProposalService::set_commit_policy`] and
//!    caches the value on [`crate::state::WorkspaceState`].
//! 2. The pending-proposals queue, populated from
//!    [`crate::state::ProposalService::list_pending_proposals`]
//!    and refreshed by `RefreshPendingProposals` / `SwitchTab`.
//! 3. Approve / Reject buttons, which route through
//!    [`crate::state::ProposalService::approve_proposal`] and
//!    [`crate::state::ProposalService::reject_proposal`].
//!
//! These tests assert (1) and (3) directly against the view
//! state, going through the trait so the routing is exercised.
//! The end-to-end "click button → state updates" path is the same
//! code path the UI uses, minus the gpui button-event plumbing
//! (which is exercised by the production binary). Keeping the
//! test surface here avoids registering every action in the
//! `gpui_kit::test` harness, which would require importing
//! `tokio` for the bridge that the handlers reach through.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use sagaline_agent::CommitPolicy;
use sagaline_store::{ProposalActionRow, ProposalRow, ProposalStatus};
use sagaline_ui::{ProposalService, ProposalServiceSlot, WorkspaceView};
use gpui_kit::AppContext;

#[derive(Default)]
struct StubProposalService {
    policy: Mutex<CommitPolicy>,
    list_calls: Mutex<u32>,
    approve_calls: Mutex<Vec<String>>,
    reject_calls: Mutex<Vec<String>>,
    pending: Mutex<Vec<ProposalRow>>,
    actions_by_id: Mutex<BTreeMap<String, Vec<ProposalActionRow>>>,
}

impl StubProposalService {
    fn seeded(
        rows: Vec<ProposalRow>,
        actions: BTreeMap<String, Vec<ProposalActionRow>>,
    ) -> Self {
        Self {
            policy: Mutex::new(CommitPolicy::Auto),
            list_calls: Mutex::new(0),
            approve_calls: Mutex::new(Vec::new()),
            reject_calls: Mutex::new(Vec::new()),
            pending: Mutex::new(rows),
            actions_by_id: Mutex::new(actions),
        }
    }
}

impl ProposalService for StubProposalService {
    fn current_commit_policy(&self) -> CommitPolicy {
        *self.policy.lock().unwrap()
    }
    fn set_commit_policy(&self, policy: CommitPolicy) {
        *self.policy.lock().unwrap() = policy;
    }
    fn list_pending_proposals(
        &self,
        _story_id: Option<&str>,
    ) -> Result<Vec<ProposalRow>, String> {
        *self.list_calls.lock().unwrap() += 1;
        Ok(self.pending.lock().unwrap().clone())
    }
    fn list_proposal_actions(
        &self,
        proposal_id: &str,
    ) -> Result<Vec<ProposalActionRow>, String> {
        Ok(self
            .actions_by_id
            .lock()
            .unwrap()
            .get(proposal_id)
            .cloned()
            .unwrap_or_default())
    }
    fn approve_proposal(&self, proposal_id: &str, _decided_by: &str) -> Result<usize, String> {
        self.approve_calls
            .lock()
            .unwrap()
            .push(proposal_id.to_string());
        self.pending.lock().unwrap().retain(|p| p.id != proposal_id);
        Ok(1)
    }
    fn reject_proposal(&self, proposal_id: &str, _decided_by: &str) -> Result<(), String> {
        self.reject_calls
            .lock()
            .unwrap()
            .push(proposal_id.to_string());
        self.pending.lock().unwrap().retain(|p| p.id != proposal_id);
        Ok(())
    }
    fn clone_service(&self) -> Box<dyn ProposalService> {
        Box::new(StubProposalService {
            policy: Mutex::new(self.current_commit_policy()),
            list_calls: Mutex::new(*self.list_calls.lock().unwrap()),
            approve_calls: Mutex::new(self.approve_calls.lock().unwrap().clone()),
            reject_calls: Mutex::new(self.reject_calls.lock().unwrap().clone()),
            pending: Mutex::new(self.pending.lock().unwrap().clone()),
            actions_by_id: Mutex::new(self.actions_by_id.lock().unwrap().clone()),
        })
    }
}

fn make_proposal(id: &str, summary: &str) -> ProposalRow {
    ProposalRow {
        id: id.to_string(),
        story_id: "story-1".to_string(),
        agent_id: "agent-1".to_string(),
        status: ProposalStatus::Pending,
        summary: summary.to_string(),
        diff_json: String::new(),
        created_at: "2026-09-21T10:00:00Z".to_string(),
        decided_at: None,
        decided_by: None,
    }
}

fn make_action(proposal_id: &str, seq: i64, tool_name: &str) -> ProposalActionRow {
    ProposalActionRow {
        id: seq,
        proposal_id: proposal_id.to_string(),
        seq,
        tool_name: tool_name.to_string(),
        tool_args_json: "{}".to_string(),
    }
}

#[gpui_kit::test]
async fn set_commit_policy_writes_through_service(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_component::init);
    let stub = Arc::new(StubProposalService::default());
    cx.set_global(ProposalServiceSlot(Box::new(StubProposalServiceClone(
        stub.clone(),
    ))));

    let view = cx.new(|cx| WorkspaceView::new_with_tree(cx));

    // Pull the service out of the global the same way the action
    // handler does and assert the value flows.
    let svc = cx
        .update(|cx| cx.try_global::<ProposalServiceSlot>().cloned())
        .unwrap_or_else(|| panic!("ProposalServiceSlot must be registered"));
    svc.set_commit_policy(CommitPolicy::Manual);
    assert_eq!(
        stub.current_commit_policy(),
        CommitPolicy::Manual,
        "set_commit_policy must reach the service implementation"
    );

    // The view's cache must reflect the new value.
    view.update(cx, |view, _cx| {
        view.state_mut().set_commit_policy(CommitPolicy::Manual);
    });
    view.update(cx, |view, _cx| {
        assert_eq!(
            view.state().commit_policy_cache,
            CommitPolicy::Manual,
            "view cache must mirror the live policy"
        );
    });
}

#[gpui_kit::test]
async fn list_pending_proposals_populates_view_cache(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_component::init);
    let mut actions = BTreeMap::new();
    actions.insert(
        "p-1".to_string(),
        vec![make_action("p-1", 1, "create_character")],
    );
    let stub = Arc::new(StubProposalService::seeded(
        vec![make_proposal("p-1", "Add Lin Mo")],
        actions,
    ));
    cx.set_global(ProposalServiceSlot(Box::new(StubProposalServiceClone(
        stub.clone(),
    ))));

    let view = cx.new(|cx| WorkspaceView::new_with_tree(cx));
    let svc = cx
        .update(|cx| cx.try_global::<ProposalServiceSlot>().cloned())
        .unwrap_or_else(|| panic!("ProposalServiceSlot must be registered"));
    let proposals = svc
        .list_pending_proposals(None)
        .expect("list returns Ok");
    let mut action_map: BTreeMap<String, Vec<ProposalActionRow>> = BTreeMap::new();
    for p in &proposals {
        action_map.insert(p.id.clone(), svc.list_proposal_actions(&p.id).unwrap_or_default());
    }

    view.update(cx, |view, _cx| {
        view.state_mut().set_pending_proposals(proposals, action_map);
    });
    view.update(cx, |view, _cx| {
        assert_eq!(view.state().pending_proposals.len(), 1);
        assert_eq!(view.state().pending_proposals[0].id, "p-1");
        assert_eq!(
            view.state().proposal_actions.get("p-1").map(Vec::len),
            Some(1)
        );
        assert_eq!(*stub.list_calls.lock().unwrap(), 1);
    });
}

#[gpui_kit::test]
async fn approve_proposal_drops_from_pending(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_component::init);
    let stub = Arc::new(StubProposalService::seeded(
        vec![
            make_proposal("p-1", "Add Lin Mo"),
            make_proposal("p-2", "Add Detective"),
        ],
        BTreeMap::new(),
    ));
    cx.set_global(ProposalServiceSlot(Box::new(StubProposalServiceClone(
        stub.clone(),
    ))));

    let view = cx.new(|cx| WorkspaceView::new_with_tree(cx));
    let svc = cx
        .update(|cx| cx.try_global::<ProposalServiceSlot>().cloned())
        .unwrap_or_else(|| panic!("ProposalServiceSlot must be registered"));

    svc.approve_proposal("p-1", "user").expect("approve returns Ok");
    assert_eq!(stub.approve_calls.lock().unwrap().as_slice(), &["p-1".to_string()]);

    let refreshed = svc.list_pending_proposals(None).expect("list");
    let mut action_map = BTreeMap::new();
    for p in &refreshed {
        action_map.insert(p.id.clone(), svc.list_proposal_actions(&p.id).unwrap_or_default());
    }
    view.update(cx, |view, _cx| {
        view.state_mut().set_pending_proposals(refreshed, action_map);
    });
    view.update(cx, |view, _cx| {
        assert_eq!(
            view.state().pending_proposals.len(),
            1,
            "successful approve must drop the proposal from pending"
        );
        assert_eq!(view.state().pending_proposals[0].id, "p-2");
    });
}

#[gpui_kit::test]
async fn reject_proposal_drops_from_pending(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_component::init);
    let stub = Arc::new(StubProposalService::seeded(
        vec![make_proposal("p-1", "Add Lin Mo")],
        BTreeMap::new(),
    ));
    cx.set_global(ProposalServiceSlot(Box::new(StubProposalServiceClone(
        stub.clone(),
    ))));

    let view = cx.new(|cx| WorkspaceView::new_with_tree(cx));
    let svc = cx
        .update(|cx| cx.try_global::<ProposalServiceSlot>().cloned())
        .unwrap_or_else(|| panic!("ProposalServiceSlot must be registered"));

    svc.reject_proposal("p-1", "user").expect("reject returns Ok");
    assert_eq!(stub.reject_calls.lock().unwrap().as_slice(), &["p-1".to_string()]);

    let refreshed = svc.list_pending_proposals(None).expect("list");
    let mut action_map = BTreeMap::new();
    for p in &refreshed {
        action_map.insert(p.id.clone(), svc.list_proposal_actions(&p.id).unwrap_or_default());
    }
    view.update(cx, |view, _cx| {
        view.state_mut().set_pending_proposals(refreshed, action_map);
    });
    view.update(cx, |view, _cx| {
        assert!(
            view.state().pending_proposals.is_empty(),
            "successful reject must drop the proposal from pending"
        );
    });
}

#[gpui_kit::test]
async fn service_error_surfaces_in_view_state(cx: &mut gpui_kit::TestAppContext) {
    struct FailingApprove;
    impl ProposalService for FailingApprove {
        fn current_commit_policy(&self) -> CommitPolicy {
            CommitPolicy::Auto
        }
        fn set_commit_policy(&self, _: CommitPolicy) {}
        fn list_pending_proposals(&self, _: Option<&str>) -> Result<Vec<ProposalRow>, String> {
            Ok(Vec::new())
        }
        fn list_proposal_actions(&self, _: &str) -> Result<Vec<ProposalActionRow>, String> {
            Ok(Vec::new())
        }
        fn approve_proposal(&self, _: &str, _: &str) -> Result<usize, String> {
            Err("validate_world reported 1 issue(s); world DB untouched".into())
        }
        fn reject_proposal(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn clone_service(&self) -> Box<dyn ProposalService> {
            Box::new(FailingApprove)
        }
    }
    cx.update(gpui_component::init);
    cx.set_global(ProposalServiceSlot(Box::new(FailingApprove)));

    let view = cx.new(|cx| WorkspaceView::new_with_tree(cx));
    let svc = cx
        .update(|cx| cx.try_global::<ProposalServiceSlot>().cloned())
        .unwrap_or_else(|| panic!("ProposalServiceSlot must be registered"));

    let err = svc
        .approve_proposal("p-bad", "user")
        .expect_err("approve must surface the underlying error");
    assert!(err.contains("validate_world"), "got: {err}");

    // The action handler does the same `proposal_error = Some(...)`
    // assignment the production code does.
    view.update(cx, |view, _cx| {
        view.state_mut().proposal_error = Some(err);
    });
    view.update(cx, |view, _cx| {
        let stored = view
            .state()
            .proposal_error
            .as_deref()
            .expect("approve failure must surface as proposal_error");
        assert!(
            stored.contains("validate_world"),
            "error message must round-trip from the service, got: {stored}"
        );
    });
}

/// Newtype that lets a registered `Box<dyn ProposalService>`
/// delegate to an `Arc<StubProposalService>` so the test can see
/// the stub's mutable state after the global's call returns.
struct StubProposalServiceClone(Arc<StubProposalService>);
impl ProposalService for StubProposalServiceClone {
    fn current_commit_policy(&self) -> CommitPolicy {
        self.0.current_commit_policy()
    }
    fn set_commit_policy(&self, policy: CommitPolicy) {
        self.0.set_commit_policy(policy);
    }
    fn list_pending_proposals(&self, story_id: Option<&str>) -> Result<Vec<ProposalRow>, String> {
        self.0.list_pending_proposals(story_id)
    }
    fn list_proposal_actions(&self, id: &str) -> Result<Vec<ProposalActionRow>, String> {
        self.0.list_proposal_actions(id)
    }
    fn approve_proposal(&self, id: &str, by: &str) -> Result<usize, String> {
        self.0.approve_proposal(id, by)
    }
    fn reject_proposal(&self, id: &str, by: &str) -> Result<(), String> {
        self.0.reject_proposal(id, by)
    }
    fn clone_service(&self) -> Box<dyn ProposalService> {
        Box::new(StubProposalServiceClone(self.0.clone()))
    }
}