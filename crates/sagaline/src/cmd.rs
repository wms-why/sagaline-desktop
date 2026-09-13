//! Background writer task.
//!
//! Owns the shared SQLite connection and a `mpsc` receiver. The UI dispatches
//! commands onto the sender side via `WorkspaceState::dispatch`; this task
//! consumes them one at a time and replies via the per-command `oneshot`.
//!
//! This module is the seam every later CRUD (add chapter, add character,
//! etc.) will reuse — extend the `match` here when a new `sagaline_core::cmd::Cmd`
//! variant is added.

use std::sync::{Arc, Mutex};

use futures::channel::mpsc::UnboundedReceiver;
use futures::StreamExt;
use sagaline_core::{cmd::Cmd, db, model::Story};
use sagaline_core::rusqlite;

/// Consume `Cmd` values from `rx` until the channel closes. Each command is
/// processed sequentially on this task — SQLite serializes writes naturally,
/// so we don't need additional locking beyond the `Mutex` around the
/// `Connection`.
pub async fn run_writer(
    conn: Arc<Mutex<rusqlite::Connection>>,
    mut rx: UnboundedReceiver<Cmd>,
) {
    while let Some(cmd) = rx.next().await {
        match cmd {
            Cmd::CreateStory { title, reply } => {
                let result: anyhow::Result<Story> = (|| {
                    let conn = conn.lock().expect("connection mutex poisoned");
                    db::create_story(&conn, &title).map_err(Into::into)
                })();
                let _ = reply.send(result);
            }
        }
    }
}