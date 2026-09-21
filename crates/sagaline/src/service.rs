//! `StoryService` adapter from `AppEnv`.
//!
//! The UI crate never depends on this binary crate, so the
//! `StoryServiceSlot` global the binary installs is the bridge:
//! the UI looks up `Box<dyn StoryService>` (through the
//! `StoryServiceSlot` newtype) and calls through it.

use std::path::PathBuf;
use std::sync::Arc;

use sagaline_core::{ProjectLocation, Story, StorySummary};
use sagaline_ui::StoryService;

use crate::env::AppEnv;

/// Thin adapter that implements [`StoryService`] for an
/// `Arc<AppEnv>`. Cheap to clone — the underlying `AppEnv` is
/// already `Arc`-shared, and `set_project_location` is the only
/// mutating call (it goes through `AppEnv::set_project_location`
/// which is internally synchronised).
#[derive(Clone)]
pub struct AppEnvStoryService(pub Arc<AppEnv>);

impl AppEnvStoryService {
    pub fn new(env: Arc<AppEnv>) -> Self {
        Self(env)
    }
}

impl StoryService for AppEnvStoryService {
    fn create_story(&self, title: &str) -> Result<Arc<Story>, String> {
        let store = self
            .0
            .story_store()
            .ok_or_else(|| "Pick a project location first (⌘ ,).".to_string())?;
        // The store is `Arc<dyn StoryStore>`; `create` is sync.
        // The UI wraps the call in `spawn_blocking`, so we just
        // call through here.
        store.create(title).map_err(|e| e.to_string())
    }

    fn project_location_display(&self) -> Option<String> {
        self.0.story_location().map(|p| p.display())
    }

    fn set_project_location(&self, path: PathBuf) -> Result<(), String> {
        self.0
            .set_project_location(ProjectLocation::new(path))
            .map_err(|e| e.to_string())
    }

    fn recent_stories(&self) -> Vec<StorySummary> {
        self.0
            .story_store()
            .and_then(|s| s.list().ok())
            .unwrap_or_default()
    }

    fn clone_service(&self) -> Box<dyn StoryService> {
        Box::new(self.clone())
    }
}
