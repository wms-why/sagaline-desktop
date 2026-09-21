//! Read tools for the top-level `stories` entity.
//!
//! - `get_story` — fetch a single story by id or slug.
//! - `list_stories` — enumerate every story in the world.
//! - `search_story` — substring search over story title + summary
//!   (Phase 2 stub; the future FTS5-backed retrieval lands with
//!   the retrieval subsystem).

use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use sagaline_store::{repo::StoryRow, World};

use crate::tool::{Capability, Tool, ToolContext, ToolDescriptor, ToolError, ToolResult};
use crate::tools::domain;

// ---- get_story ------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetStoryArgs {
    /// Stable id (UUID v7) or slug. One of `id` or `slug` is required.
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GetStoryOutput {
    pub story: Option<StoryRow>,
}

pub struct GetStoryTool {
    world: Arc<World>,
}

impl GetStoryTool {
    pub fn new(world: Arc<World>) -> Self {
        Self { world }
    }
}

#[async_trait]
impl Tool for GetStoryTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<GetStoryArgs>(
            "get_story",
            "Fetch a single story by its UUID v7 id or its slug. \
             Returns `{story: {id, slug, title, summary, ...} | null}`. \
             Exactly one of `id` or `slug` is required.",
            Capability::Read,
        )
    }

    async fn execute(&self, _ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: GetStoryArgs = domain::parse_args("get_story", args)?;
        let row = if let Some(id) = parsed.id {
            self.world
                .stories()
                .get(&id)
                .map_err(|e| domain::map_store_err("get_story", e))?
        } else if let Some(slug) = parsed.slug {
            self.world
                .stories()
                .get_by_slug(&slug)
                .map_err(|e| domain::map_store_err("get_story", e))?
        } else {
            return Err(ToolError::BadArgs {
                name: "get_story".into(),
                message: "either `id` or `slug` is required".into(),
            });
        };
        domain::to_result(&GetStoryOutput { story: row })
    }
}

// ---- list_stories ---------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListStoriesArgs {
    /// Reserved for future pagination; ignored today.
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ListStoriesOutput {
    pub stories: Vec<StoryRow>,
}

pub struct ListStoriesTool {
    world: Arc<World>,
}

impl ListStoriesTool {
    pub fn new(world: Arc<World>) -> Self {
        Self { world }
    }
}

#[async_trait]
impl Tool for ListStoriesTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<ListStoriesArgs>(
            "list_stories",
            "Enumerate every story in the world. Returns \
             `{stories: [{id, slug, title, ...}]}` sorted by \
             creation time. `limit` caps the response size \
             (default: no cap).",
            Capability::Read,
        )
    }

    async fn execute(&self, _ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: ListStoriesArgs = domain::parse_args("list_stories", args)?;
        let mut stories = self
            .world
            .stories()
            .list()
            .map_err(|e| domain::map_store_err("list_stories", e))?;
        if let Some(n) = parsed.limit {
            stories.truncate(n as usize);
        }
        domain::to_result(&ListStoriesOutput { stories })
    }
}

// ---- search_story ---------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchStoryArgs {
    /// Case-insensitive substring matched against title + summary.
    pub query: String,
}

#[derive(Debug, Serialize)]
pub struct SearchStoryOutput {
    pub matches: Vec<StoryRow>,
}

pub struct SearchStoryTool {
    world: Arc<World>,
}

impl SearchStoryTool {
    pub fn new(world: Arc<World>) -> Self {
        Self { world }
    }
}

#[async_trait]
impl Tool for SearchStoryTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<SearchStoryArgs>(
            "search_story",
            "Substring search over story title + summary. \
             Returns `{matches: [{id, slug, title, ...}]}`. \
             Phase 2 stub; the future FTS5-backed retrieval \
             lands with the retrieval subsystem.",
            Capability::Read,
        )
    }

    async fn execute(&self, _ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: SearchStoryArgs = domain::parse_args("search_story", args)?;
        let needle = parsed.query.to_lowercase();
        let matches = self
            .world
            .stories()
            .list()
            .map_err(|e| domain::map_store_err("search_story", e))?
            .into_iter()
            .filter(|s| {
                s.title.to_lowercase().contains(&needle)
                    || s.summary.to_lowercase().contains(&needle)
            })
            .collect();
        domain::to_result(&SearchStoryOutput { matches })
    }
}
