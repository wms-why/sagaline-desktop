//! Sagaline desktop binary — app shell.
//!
//! Re-exports the public surface of `sagaline-ui` for convenience. The
//! actual `main` function lives in `main.rs` and is intentionally
//! minimal — a real top-level `App` entity will land in the next phase.

pub use sagaline_ui::{WorkspaceState, WorkspaceView};