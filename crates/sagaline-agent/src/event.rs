//! The agent's event stream — what the UI subscribes to for the
//! live chain-of-thought panel.
//!
//! Every step the agent takes emits one [`AgentEvent`]. The
//! [`EventSink`] trait is the broadcast surface; the default
//! implementation just collects events into a `Vec` for tests.

use serde::Serialize;
use serde_json::Value;

/// One beat of the agent's chain-of-thought.
///
/// Variants mirror the loop: OBSERVE → PLAN → ACT → REFLECT → next.
/// `step` increments monotonically so the UI can render "step 3 / 12"
/// without re-counting.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentEvent {
    /// Step counter at the start of a new step. Always the first event
    /// of a step; lets the UI show a "step N" header before details.
    StepStart { step: u32 },

    /// The agent loaded the current scene and resolved its references.
    Observe {
        step: u32,
        scene_id: String,
        resolved: ResolvedContext,
    },

    /// The agent decomposed the scene into a plan (shot list etc.).
    Plan {
        step: u32,
        plan: String,
        shots_planned: u32,
    },

    /// The agent invoked a tool and got a result back.
    Act {
        step: u32,
        tool: String,
        args: Value,
        result_summary: String,
    },

    /// The agent reflected on the step (validation + self-critique).
    Reflect {
        step: u32,
        validation_ok: bool,
        notes: String,
    },

    /// The loop finished. `reason` is human-readable; consumers
    /// should not parse it.
    Done { step: u32, reason: String },
}

/// The slice of the story the agent has loaded for the current step.
/// Kept as owned `String`s so events are `Clone` and trivially
/// serializable to the UI.
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedContext {
    pub characters: Vec<String>,
    pub environment: Option<String>,
    pub props: Vec<String>,
}

impl ResolvedContext {
    pub fn is_empty(&self) -> bool {
        self.characters.is_empty() && self.environment.is_none() && self.props.is_empty()
    }
}

/// Anything that can receive agent events. The default implementation is
/// `VecCollector`; the future gpui implementation will forward events
/// to a UI channel.
pub trait EventSink {
    fn emit(&mut self, event: AgentEvent);
}

/// In-memory sink: appends events to a `Vec`. Used by tests and by the
/// CLI / headless mode.
#[derive(Default)]
pub struct VecCollector {
    pub events: Vec<AgentEvent>,
}

impl EventSink for VecCollector {
    fn emit(&mut self, event: AgentEvent) {
        self.events.push(event);
    }
}
