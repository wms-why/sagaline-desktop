//! UI-side state for an open story.
//!
//! [`WorkspaceState`] is a plain data struct — it wraps an opened
//! [`sagaline_core::Story`] plus the in-memory [`StoryGraph`] the
//! agent / preview panel consume, and tracks the user's selection.
//! The view layer owns a `WorkspaceState` and calls `cx.notify()`
//! after mutating it to trigger a redraw.
//!
//! [`KeyStoreSlot`] is the gpui global the binary installs to
//! expose the encrypted [`sagaline_store::World`] to the BYOK
//! key-management panel. It lives here (rather than in
//! `view.rs`) so both the view and the action handlers can reach
//! it without duplicating the slot type.
//!
//! Commands are sync: opening / reloading a story is one disk walk, and
//! editing a file is a direct write + reload. No background actor is
//! needed.

use std::sync::Arc;

use gpui_kit::Global;
use sagaline_core::{CoreError, EntityId, ProjectLocation, Story, StoryGraph, StorySummary};
use sagaline_store::World;

/// gpui global carrying the [`World`] the app shell opened at
/// startup. The view layer reads this from [`App::global`] to
/// render the BYOK panel; the binary sets it once during
/// `install_env`. When absent (e.g. in headless tests) the panel
/// renders a placeholder.
pub struct KeyStoreSlot(pub Arc<World>);

impl Global for KeyStoreSlot {}

/// Top-level UI state. Owns the loaded story (as a storage-agnostic
/// handle + the in-memory graph the agent reasons about) and the
/// user's selection. The story path is hidden behind [`Story`].
#[derive(Debug, Default)]
pub struct WorkspaceState {
    /// The story currently open, if any. Holds the title and id the
    /// UI shows; the on-disk path is hidden behind `Story::handle`.
    pub story: Option<Arc<Story>>,
    /// The cached graph for the open story. `None` when no story is open.
    pub graph: Option<StoryGraph>,
    /// Currently selected entity (left pane → right pane preview).
    pub selected: Option<EntityId>,
    /// Most recent load / reload error, surfaced in the UI banner.
    pub last_error: Option<String>,
    /// User's preferred project location. `None` until they pick one
    /// in the Project Settings modal. Owned by `AppEnv`; this is a
    /// snapshot the UI uses to render the onboarding / new-story
    /// dialog hints.
    pub project_location: Option<ProjectLocation>,
    /// Recent stories from the configured project location. Capped
    /// at 25, ordered most-recent-first. Refreshed when the project
    /// location changes or after a create / open.
    pub recent_stories: Vec<StorySummary>,
}

impl WorkspaceState {
    /// Empty state — the UI on first launch before any story is opened.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adopt a freshly-loaded [`Story`]. Loads the in-memory graph
    /// from the story's on-disk path. On failure the previous
    /// state is preserved and the error is recorded.
    pub fn open_story(&mut self, story: Arc<Story>) {
        let path = story.handle().path().to_path_buf();
        match try_load(&path) {
            Ok(graph) => {
                self.story = Some(story);
                self.graph = Some(graph);
                self.selected = None;
                self.last_error = None;
            }
            Err(e) => {
                self.last_error = Some(format_load_error(&path, &e));
            }
        }
    }

    /// Re-walk the currently-open story and refresh the graph.
    /// No-op if no story is open.
    pub fn reload(&mut self) {
        let Some(story) = self.story.clone() else {
            return;
        };
        let path = story.handle().path();
        match try_load(path) {
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

    pub fn story(&self) -> Option<&Arc<Story>> {
        self.story.as_ref()
    }

    /// Update the project location preference in the snapshot. The
    /// `AppEnv` is the source of truth; the UI keeps a copy so
    /// `cx.notify()` redraws the onboarding / dialog hints.
    pub fn set_project_location(&mut self, loc: ProjectLocation) {
        self.project_location = Some(loc);
    }

    /// Replace the recent-stories list.
    pub fn set_recent_stories(&mut self, list: Vec<StorySummary>) {
        self.recent_stories = list;
    }

    /// Display-form of the project location for the new-story hint.
    /// `None` when the user hasn't set one yet.
    pub fn project_location_summary(&self) -> Option<String> {
        self.project_location.as_ref().map(|p| p.display())
    }
}


/// gpui global the binary registers so the UI can ask for a
/// `Arc<Story>` to be created (or to look up the project
/// location) without depending on the binary crate. The binary
/// implements this against `AppEnv::story_store()` + the
/// `ProjectLocation` pref; the UI calls it through the global.
pub trait StoryService: Send + Sync + 'static {
    /// Create a story with the given title. Returns the new
    /// `Arc<Story>` on success, or a human-readable error.
    fn create_story(&self, title: &str) -> Result<std::sync::Arc<sagaline_core::Story>, String>;
    /// Current project location, if the user has set one.
    fn project_location_display(&self) -> Option<String>;
    /// Persist a new project location; refreshes the recent
    /// stories list inside the service.
    fn set_project_location(
        &self,
        path: std::path::PathBuf,
    ) -> Result<(), String>;
    /// The recent-stories list. Updated by `set_project_location`
    /// and `create_story`; the UI reads it to render the recent
    /// list and refreshes after writes.
    fn recent_stories(&self) -> Vec<sagaline_core::StorySummary>;
    /// Clone the service into a `Box<dyn StoryService>`. The
    /// concrete impl is required to be `Clone`.
    fn clone_service(&self) -> Box<dyn StoryService>;
}

/// Concrete newtype the binary registers as a gpui global. Wraps
/// the `Box<dyn StoryService>` so `try_global` can return a
/// `&StoryServiceSlot` and the inner service is clone-able.
pub struct StoryServiceSlot(pub Box<dyn StoryService>);

impl Clone for StoryServiceSlot {
    fn clone(&self) -> Self {
        Self(self.0.clone_service())
    }
}

impl gpui_kit::Global for StoryServiceSlot {}

impl std::ops::Deref for StoryServiceSlot {
    type Target = dyn StoryService;
    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}


fn try_load(path: &std::path::Path) -> Result<StoryGraph, CoreError> {
    let root = sagaline_core::StoryRoot::new(path.to_path_buf())?;
    StoryGraph::load(&root)
}

fn format_load_error(path: &std::path::Path, e: &CoreError) -> String {
    format!("failed to load {}: {e}", path.display())
}

