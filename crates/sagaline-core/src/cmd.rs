//! Commands the UI dispatches to the background writer task.
//!
//! The desktop UI runs in `cx.spawn` tasks on the gpui runtime; long-running
//! or blocking work (SQLite writes, model-adapter calls) is routed through an
//! `mpsc` channel to a single writer so the UI thread never blocks. Each
//! command carries a `oneshot::Sender` so the caller can await its result
//! without coupling to the writer's internal state.
//!
//! This is the seam every future CRUD (add chapter, add character, etc.) will
//! reuse — append a new variant, handle it in `sagaline::cmd::run_writer`.

use futures::channel::oneshot;

use crate::model::Story;

/// A request from the UI that the writer task will fulfill.
pub enum Cmd {
    /// Insert a new `Story` row with the given title and reply with the
    /// resulting entity (including its assigned `StoryId`).
    CreateStory {
        title: String,
        reply: oneshot::Sender<anyhow::Result<Story>>,
    },
}