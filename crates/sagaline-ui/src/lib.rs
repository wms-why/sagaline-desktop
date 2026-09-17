//! Sagaline desktop UI — gpui-kit views over the [`sagaline_core`] story graph.
//!
//! Public surface:
//!
//! - [`WorkspaceState`] — plain Rust state that holds an opened story and
//!   the user's selection.
//! - [`WorkspaceView`] — top-level gpui-kit view. Constructed with
//!   [`WorkspaceView::new_with_tree`] to render the file tree on the left.
//! - [`register_actions`] — bind the UI's actions
//!   ([`actions::OpenStory`], [`actions::ReloadStory`],
//!   [`actions::SwitchTab`], [`actions::ImportProviderKeyFromFile`],
//!   [`actions::DeleteProviderKey`]) to handlers.
//! - [`actions`] — the action types.
//! - [`activity::AgentEventLog`] — the gpui global the app shell pushes
//!   agent events into; the activity panel reads it.
//! - [`KeyStoreSlot`] — the gpui global carrying the encrypted
//!   [`sagaline_keys::SagalineStore`]; the BYOK panel reads it.
#![deny(unsafe_code)]

pub mod actions;
pub mod activity;
pub mod state;
pub mod view;
pub use view::{register_actions, StoryOpened, WorkspaceView};
pub use actions::{DeleteProviderKey, ImportProviderKeyFromFile, OpenStory, ReloadStory, SwitchTab};
pub use activity::{format_event, render_activity, AgentEventLog};
pub use state::WorkspaceState;
pub use state::KeyStoreSlot;

