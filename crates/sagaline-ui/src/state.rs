//! UI-side state for an open story.
//!
//! [`WorkspaceState`] is a plain data struct — it wraps a
//! [`sagaline_core::StoryGraph`] loaded from disk and tracks the
//! user's selection. The view layer owns a `WorkspaceState` and calls
//! `cx.notify()` after mutating it to trigger a redraw.
//!
//! [`KeyStoreSlot`] is the gpui global the binary installs to
//! expose the encrypted [`sagaline_keys::SagalineStore`] to the
//! BYOK key-management panel. It lives here (rather than in
//! `view.rs`) so both the view and the action handlers can reach
//! it without duplicating the slot type.
//!
//! Commands are sync: opening / reloading a story is one disk walk, and
//! editing a file is a direct write + reload. No background actor is
//! needed — unlike the previous SQL-era design, there is no shared
//! database connection to coordinate writes through.
use std::path::{Path, PathBuf};

use sagaline_core::{CoreError, EntityId, StoryGraph, StoryRoot};
use std::sync::Arc;
use gpui_kit::Global;
use sagaline_keys::SagalineStore;

/// gpui global carrying the [`SagalineStore`] the app shell
/// opened at startup. The view layer reads this from
/// [`App::global`] to render the BYOK panel; the binary sets it
/// once during `install_env`. When absent (e.g. in headless tests)
/// the panel renders a placeholder.
pub struct KeyStoreSlot(pub Arc<SagalineStore>);

impl Global for KeyStoreSlot {}

/// Top-level UI state. Owns the loaded story graph and the current
/// selection.
#[derive(Debug, Default)]
pub struct WorkspaceState {
    /// The story directory currently open, if any.
    pub root: Option<StoryRoot>,
    /// The cached graph for the open story. `None` when no story is open.
    pub graph: Option<StoryGraph>,
    /// Currently selected entity (left pane → right pane preview).
    pub selected: Option<EntityId>,
    /// Most recent load / reload error, surfaced in the UI banner.
    pub last_error: Option<String>,
}

impl WorkspaceState {
    /// Empty state — the UI on first launch before any story is opened.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load the story at `path` and adopt it. On failure, the previous
    /// state is preserved and the error is recorded.
    pub fn open_story(&mut self, path: PathBuf) {
        match try_load(&path) {
            Ok((root, graph)) => {
                self.root = Some(root);
                self.graph = Some(graph);
                self.selected = None;
                self.last_error = None;
            }
            Err(e) => {
                self.last_error = Some(format_load_error(&path, &e));
            }
        }
    }

    /// Re-walk the currently-open story directory and refresh the graph.
    /// No-op if no story is open.
    pub fn reload(&mut self) {
        let Some(root) = self.root.clone() else {
            return;
        };
        match StoryGraph::load(&root) {
            Ok(graph) => {
                self.graph = Some(graph);
                self.last_error = None;
            }
            Err(e) => {
                self.last_error = Some(format!("reload failed: {e}"));
            }
        }
    }

    /// Select an entity for preview in the right pane. `None` clears
    /// the preview.
    pub fn select(&mut self, id: Option<EntityId>) {
        self.selected = id;
    }

    pub fn graph(&self) -> Option<&StoryGraph> {
        self.graph.as_ref()
    }

    /// Borrow the currently-open story path, if any.
    pub fn root_path(&self) -> Option<&Path> {
        self.root.as_ref().map(|r| r.path())
    }

    /// Own the currently-open story path, if any.
    pub fn root_path_owned(&self) -> Option<PathBuf> {
        self.root_path().map(|p| p.to_path_buf())
    }
}

fn try_load(path: &Path) -> Result<(StoryRoot, StoryGraph), CoreError> {
    let root = StoryRoot::new(path)?;
    let graph = StoryGraph::load(&root)?;
    Ok((root, graph))
}

fn format_load_error(path: &Path, e: &CoreError) -> String {
    format!("failed to load {}: {e}", path.display())
}