//! Sagaline desktop binary — thin shell that boots the gpui window and
//! runs the background command writer.
//!
//! The writer lives here (not in `sagaline-ui`) because it owns the shared
//! `Arc<Mutex<rusqlite::Connection>>` and the `mpsc` receiver that the UI
//! dispatches into via `WorkspaceState::dispatch`.

pub mod cmd;