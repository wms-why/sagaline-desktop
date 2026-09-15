//! Sagaline desktop UI — gpui-kit views over the [`sagaline_core`] story graph.
//!
//! Public surface:
//!
//! - [`WorkspaceState`] — plain Rust state that holds an opened story and
//!   the user's selection.
//! - [`WorkspaceView`] — top-level gpui-kit view. Constructed with
//!   [`WorkspaceView::new_with_tree`] to render the file tree on the left.
//! - [`register_actions`] — bind the [`actions::OpenStory`] and
//!   [`actions::ReloadStory`] handlers.
//! - [`actions`] — [`actions::OpenStory`] and [`actions::ReloadStory`].

#![deny(unsafe_code)]

pub mod actions;
pub mod state;
pub mod view;

pub use actions::{OpenStory, ReloadStory};
pub use state::WorkspaceState;
pub use view::{register_actions, WorkspaceView};