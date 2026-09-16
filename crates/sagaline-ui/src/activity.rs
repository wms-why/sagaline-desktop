//! Chain-of-thought activity panel.
//!
//! The right pane of the workspace has two tabs: **Preview** (the
//! currently-selected entity's YAML front matter + Markdown body) and
//! **Activity** (a live scrollback of the agent's `AgentEvent`
//! stream).
//!
//! Events are appended by the app shell's `ChannelSink` forwarder; the
//! UI reads them through the gpui global [`AgentEventLog`]. The
//! [`render_activity`] function does the actual rendering.

use gpui_base::v_flex;
use gpui_kit::*;
use sagaline_agent::AgentEvent;

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

/// Render the activity panel. Cheap; safe to call from `Render`.
pub fn render_activity(log: &AgentEventLog) -> gpui_kit::Div {
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
