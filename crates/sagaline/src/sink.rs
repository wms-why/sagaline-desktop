//! `EventSink` implementation that ferries agent events to a
//! `tokio::sync::mpsc` channel.
//!
//! The agent loop is `async` and runs on a tokio task; the gpui UI
//! is bound to a single-threaded executor. We bridge the two with a
//! channel: the agent's [`ChannelSink`] pushes events; a separate
//! `cx.spawn`-managed task drains the receiver and pushes onto the
//! [`AgentEventLog`] global, calling `cx.notify()` so the workspace
//! re-renders.

use sagaline_agent::{AgentEvent, EventSink};
use tokio::sync::mpsc;

/// `EventSink` that posts each event into a tokio mpsc channel.
///
/// Cheap to clone and to construct. `send` failures (channel closed)
/// are swallowed — the loop's contract is best-effort observation.
#[derive(Clone)]
pub struct ChannelSink {
    pub tx: mpsc::UnboundedSender<AgentEvent>,
}

impl EventSink for ChannelSink {
    fn emit(&mut self, event: AgentEvent) {
        // The forwarder is alive for the lifetime of the run; if the
        // channel is closed, the agent's run is over and the event
        // is moot.
        let _ = self.tx.send(event);
    }
}
