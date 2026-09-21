//! Chain-of-thought activity panel.
//!
//! The right pane of the workspace has three tabs: **Preview**
//! (the currently-selected entity), **Activity** (this module),
//! and **Keys** (BYOK provider keys).
//!
//! The Activity tab is a vertical stack:
//!
//! 1. **Picker header** — `[ Auto | Manual ]` toggle (writes the
//!    live [`sagaline_agent::CommitPolicy`] via
//!    [`crate::state::ProposalService`]) and a `[Refresh]`
//!    button (re-fetches the pending-proposals queue).
//! 2. **Event log** — the existing `AgentEventLog` scrollback.
//! 3. **Pending proposals** — one card per `ProposalRow`, with
//!    `Approve` / `Reject` buttons dispatching through the same
//!    [`crate::state::ProposalService`].
//!
//! The data sources are split: events arrive via the gpui global
//! [`AgentEventLog`] (pushed by the binary's `run_agent`
//! forwarder); proposals are cached on
//! [`crate::state::WorkspaceState`] and refreshed on demand. The
//! render function never hits the world DB directly — every read
//! goes through the service trait so the test harness can drive
//! it with an in-memory stub.

use gpui_base::{h_flex, v_flex};
use gpui_base::StyledExt;
use gpui_component::ActiveTheme;
use gpui_kit::*;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use sagaline_agent::{AgentEvent, CommitPolicy};
use sagaline_store::{ProposalActionRow, ProposalRow};

use crate::state::WorkspaceState;

/// Append-only log of agent events. Stored as a gpui global so the
/// forwarder task in the app shell can push events without holding
/// the workspace's `Entity` handle.
#[derive(Debug, Default, Clone)]
pub struct AgentEventLog {
    pub events: Vec<AgentEvent>,
}

impl Global for AgentEventLog {}

/// Convert a single [`AgentEvent`] into a short human-readable line.
/// The exact format is allowed to change without notice; this is the
/// function the activity panel renders. Tests don't pin wording.
pub fn format_event(event: &AgentEvent) -> String {
    match event {
        AgentEvent::StepStart { step } => {
            format!("── step {step} ──")
        }
        AgentEvent::Observe { step, scene_id, resolved } => {
            let mut parts: Vec<String> = Vec::new();
            if !resolved.characters.is_empty() {
                parts.push(format!("chars: {}", resolved.characters.join(", ")));
            }
            if let Some(env) = &resolved.environment {
                parts.push(format!("env: {env}"));
            }
            if !resolved.props.is_empty() {
                parts.push(format!("props: {}", resolved.props.join(", ")));
            }
            if parts.is_empty() {
                parts.push("(no references)".to_string());
            }
            format!("observe · step {step} · {scene_id} · {}", parts.join("; "))
        }
        AgentEvent::Plan { step, plan, shots_planned } => {
            let first_line = plan.lines().next().unwrap_or("");
            format!(
                "plan · step {step} · {shots_planned} shot(s) · {}",
                first_line
            )
        }
        AgentEvent::Act { step, tool, result_summary, .. } => {
            format!("act · step {step} · {tool} · {result_summary}")
        }
        AgentEvent::Reflect { step, validation_ok, notes } => {
            let mark = if *validation_ok { "✓" } else { "✗" };
            format!("reflect · step {step} {mark} · {notes}")
        }
        AgentEvent::Done { step, reason } => {
            format!("done · step {step} · {reason}")
        }
    }
}

/// Map the wire-format `policy` index (the same convention as
/// [`crate::actions::SwitchTab`] uses for tabs) back to the typed
/// [`CommitPolicy`]. Out-of-range values fall back to `Auto` —
/// defensive against malformed action payloads.
pub fn commit_policy_from_index(i: u32) -> CommitPolicy {
    match i {
        1 => CommitPolicy::Manual,
        _ => CommitPolicy::Auto,
    }
}

/// Inverse of [`commit_policy_from_index`]. Stable wire-format
/// index; the same numeric convention as [`SwitchTab`].
pub fn commit_policy_index(p: CommitPolicy) -> u32 {
    match p {
        CommitPolicy::Auto => 0,
        CommitPolicy::Manual => 1,
    }
}

/// Render the picker header: the Auto/Manual toggle and the
/// Refresh button. The toggle dispatches `SetCommitPolicy { policy }`;
/// the Refresh button dispatches `RefreshPendingProposals`.
pub fn render_activity_picker(state: &WorkspaceState, cx: &App) -> gpui_kit::Div {
    let current = state.commit_policy_cache;
    let muted = cx.theme().muted_foreground;

    let mut toggle = h_flex().gap_1();
    toggle = toggle.child(
        Button::new("policy-auto")
            .primary()
            .label("Auto")
            .on_click(|_, _, cx| {
                cx.dispatch_action(&crate::actions::SetCommitPolicy { policy: 0 });
            }),
    );
    toggle = toggle.child(
        Button::new("policy-manual")
            .primary()
            .label("Manual")
            .on_click(|_, _, cx| {
                cx.dispatch_action(&crate::actions::SetCommitPolicy { policy: 1 });
            }),
    );
    // The "active" variant surfaces via opacity / weight
    // rather than a separate ButtonVariants API; the simplest
    // signal is to render a small marker between the buttons
    // so the user sees the current value without needing a
    // design-system variant the facade doesn't expose.
    let active_marker = match current {
        CommitPolicy::Auto => "▸ Auto",
        CommitPolicy::Manual => "▸ Manual",
    };
    let active_marker = div().text_xs().text_color(muted).child(active_marker);

    let refresh = Button::new("refresh-proposals")
        .ghost()
        .label("Refresh")
        .on_click(|_, _, cx| {
            cx.dispatch_action(&crate::actions::RefreshPendingProposals);
        });

    h_flex()
        .w_full()
        .justify_between()
        .gap_2()
        .child(h_flex().gap_2().items_center().child(toggle).child(active_marker))
        .child(refresh)
}

/// One-line summary of a proposal card's queued actions. Shown
/// on each card so the user can see at a glance what would land.
fn format_actions_summary(actions: &[ProposalActionRow]) -> String {
    if actions.is_empty() {
        return "no actions queued".to_string();
    }
    let mut names: Vec<&str> = actions.iter().map(|a| a.tool_name.as_str()).collect();
    names.dedup();
    if names.len() == 1 {
        format!("1 action: {}", names[0])
    } else {
        format!("{} actions: {}", actions.len(), names.join(", "))
    }
}

/// Render a single proposal card. The Approve / Reject buttons
/// dispatch through the action system; the action handler in
/// `view::register_actions` calls the binary's
/// `ProposalService`.
fn render_proposal_card(
    proposal: &ProposalRow,
    actions: &[ProposalActionRow],
    cx: &App,
) -> gpui_kit::Div {
    let muted = cx.theme().muted_foreground;
    let id = proposal.id.clone();
    let id_for_reject = id.clone();

    let mut card = v_flex().gap_1().p_2().border_1().rounded_md().w_full();

    // Summary + timestamp.
    card = card.child(
        h_flex()
            .w_full()
            .justify_between()
            .gap_2()
            .child(div().text_sm().font_semibold().child(proposal.summary.clone()))
            .child(div().text_xs().text_color(muted).child(proposal.created_at.clone())),
    );

    // Story / agent id metadata.
    let meta = if proposal.story_id.is_empty() {
        format!("agent: {}", proposal.agent_id)
    } else {
        format!("story: {} · agent: {}", short_id(&proposal.story_id), proposal.agent_id)
    };
    card = card.child(div().text_xs().text_color(muted).child(meta));

    // Actions queued inside the proposal.
    card = card.child(
        div()
            .text_xs()
            .font_family("monospace")
            .child(format_actions_summary(actions)),
    );

    // Action buttons row. The button ids are static — the
    // proposal id is captured in the click closure, so we
    // don't need dynamic ids to route the dispatch. (gpui's
    // `Button::new` requires a `&'static str`-convertible id;
    // building one from a runtime UUID would force an
    // allocation that's only useful for a11y / DOM id
    // uniqueness, which the closure pattern already covers.)
    card = card.child(
        h_flex()
            .w_full()
            .justify_end()
            .gap_2()
            .child(
                Button::new("proposal-reject")
                    .ghost()
                    .label("Reject")
                    .on_click(move |_, _, cx| {
                        cx.dispatch_action(&crate::actions::RejectProposal {
                            proposal_id: id_for_reject.clone(),
                        });
                    }),
            )
            .child(
                Button::new("proposal-approve")
                    .primary()
                    .label("Approve")
                    .on_click(move |_, _, cx| {
                        cx.dispatch_action(&crate::actions::ApproveProposal {
                            proposal_id: id.clone(),
                        });
                    }),
            ),
    );

    card
}

/// Render the pending-proposals queue section. Empty / error
/// states render a short message instead of an empty box.
pub fn render_proposals_section(state: &WorkspaceState, cx: &App) -> gpui_kit::Div {
    let muted = cx.theme().muted_foreground;
    let danger = cx.theme().danger;
    let n = state.pending_proposals.len();

    let header = div()
        .text_sm()
        .font_semibold()
        .child(if n == 0 {
            "Pending proposals (0)".to_string()
        } else {
            format!("Pending proposals ({n})")
        });

    let mut col = v_flex().gap_2().p_2().size_full();

    if let Some(err) = &state.proposal_error {
        col = col.child(
            div()
                .text_xs()
                .text_color(danger)
                .child(format!("⚠ {err}")),
        );
    }

    col = col.child(header);

    if n == 0 {
        col = col.child(
            div()
                .text_xs()
                .text_color(muted)
                .child("No pending proposals. Mutations will queue here when commit policy is Manual."),
        );
        return col;
    }

    for proposal in &state.pending_proposals {
        let actions = state
            .proposal_actions
            .get(&proposal.id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        col = col.child(render_proposal_card(proposal, actions, cx));
    }

    col
}

/// Render the event-log portion of the activity panel. Same
/// format as before — kept as its own helper so the activity
/// panel can split "header / log / proposals" cleanly.
pub fn render_event_log(log: &AgentEventLog) -> gpui_kit::Div {
    let mut col = v_flex().gap_1().p_2().size_full();

    if log.events.is_empty() {
        col = col.child(
            div()
                .text_sm()
                .child("No agent activity yet. Open a story to start."),
        );
        return col;
    }

    for event in &log.events {
        let line = format_event(event);
        col = col.child(
            div()
                .text_xs()
                .font_family("monospace")
                .child(line),
        );
    }

    col
}

/// Render the full activity panel: picker header + event log +
/// proposals section. Cheap; safe to call from `Render`.
pub fn render_activity(state: &WorkspaceState, log: Option<&AgentEventLog>, cx: &App) -> gpui_kit::Div {
    let log = match log {
        Some(l) => l,
        None => {
            return v_flex()
                .gap_2()
                .p_2()
                .child(
                    div()
                        .text_sm()
                        .child("Activity log not installed (binary mode)."),
                );
        }
    };

    let picker = render_activity_picker(state, cx);
    let event_log = render_event_log(log);
    let proposals = render_proposals_section(state, cx);

    let muted = cx.theme().muted_foreground;
    v_flex()
        .size_full()
        .gap_0()
        .child(picker)
        .child(div().border_b_1().text_xs().text_color(muted).child(""))
        .child(event_log)
        .child(div().border_b_1().text_xs().text_color(muted).child(""))
        .child(proposals)
}

/// Compact display of a UUID so the card metadata stays on one
/// line. First 8 hex chars are more than enough to disambiguate
/// inside a single user's pending queue.
fn short_id(id: &str) -> String {
    let head: String = id.chars().take(8).collect();
    if id.len() > head.len() {
        format!("{head}…")
    } else {
        head
    }
}

#[cfg(test)]
mod tests {
    use super::{commit_policy_from_index, commit_policy_index, format_actions_summary};
    use sagaline_agent::CommitPolicy;
    use sagaline_store::ProposalActionRow;

    #[test]
    fn commit_policy_index_round_trip() {
        assert_eq!(
            commit_policy_from_index(commit_policy_index(CommitPolicy::Auto)),
            CommitPolicy::Auto
        );
        assert_eq!(
            commit_policy_from_index(commit_policy_index(CommitPolicy::Manual)),
            CommitPolicy::Manual
        );
    }

    #[test]
    fn commit_policy_index_out_of_range_defaults_to_auto() {
        assert_eq!(commit_policy_from_index(99), CommitPolicy::Auto);
    }

    #[test]
    fn format_actions_summary_handles_empty() {
        assert_eq!(format_actions_summary(&[]), "no actions queued");
    }

    #[test]
    fn format_actions_summary_dedupes_names() {
        let rows = vec![
            ProposalActionRow {
                id: 1,
                proposal_id: "p".into(),
                seq: 1,
                tool_name: "create_character".into(),
                tool_args_json: "{}".into(),
            },
            ProposalActionRow {
                id: 2,
                proposal_id: "p".into(),
                seq: 2,
                tool_name: "create_character".into(),
                tool_args_json: "{}".into(),
            },
            ProposalActionRow {
                id: 3,
                proposal_id: "p".into(),
                seq: 3,
                tool_name: "assign_character_to_scene".into(),
                tool_args_json: "{}".into(),
            },
        ];
        assert_eq!(
            format_actions_summary(&rows),
            "3 actions: create_character, assign_character_to_scene"
        );
    }
}