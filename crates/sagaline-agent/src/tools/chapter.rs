//! `chapter` domain tools.

use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use sagaline_store::{repo::ChapterRow, World};

use crate::tool::{Capability, Tool, ToolContext, ToolDescriptor, ToolError, ToolResult};
use crate::tools::domain;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateChapterArgs {
    pub story_id: String,
    pub slug: String,
    pub ordinal: i64,
    pub title: String,
    #[serde(default)]
    pub synopsis: String,
}

#[derive(Debug, Serialize)]
pub struct CreateChapterOutput {
    pub chapter: ChapterRow,
}

pub struct CreateChapterTool {
    world: Arc<World>,
}

impl CreateChapterTool {
    pub fn new(world: Arc<World>) -> Self {
        Self { world }
    }
}

#[async_trait]
impl Tool for CreateChapterTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<CreateChapterArgs>(
            "create_chapter",
            "Create a new chapter under a story. `slug` must be \
             unique within the story; `ordinal` must be unique \
             too. Returns the new chapter row.",
            Capability::Mutate,
        )
    }

    async fn execute(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: CreateChapterArgs = domain::parse_args("create_chapter", args)?;
        let row = self
            .world
            .scenes()
            .create_chapter(sagaline_store::repo::NewChapter {
                story_id: &parsed.story_id,
                slug: &parsed.slug,
                ordinal: parsed.ordinal,
                title: &parsed.title,
                synopsis: &parsed.synopsis,
            })
            .map_err(|e| domain::map_store_err("create_chapter", e))?;
        let _ = ctx;
        domain::to_result(&CreateChapterOutput { chapter: row })
    }

    fn supports_in_tx(&self) -> bool {
        true
    }

    fn execute_in_tx(
        &self,
        _ctx: &ToolContext,
        tx: &rusqlite::Transaction<'_>,
        args: Value,
    ) -> Result<ToolResult, ToolError> {
        let parsed: CreateChapterArgs = domain::parse_args("create_chapter", args)?;
        let row = self
            .world
            .scenes()
            .create_chapter_in_tx(
                tx,
                sagaline_store::repo::NewChapter {
                    story_id: &parsed.story_id,
                    slug: &parsed.slug,
                    ordinal: parsed.ordinal,
                    title: &parsed.title,
                    synopsis: &parsed.synopsis,
                },
            )
            .map_err(|e| domain::map_store_err("create_chapter", e))?;
        domain::to_result(&CreateChapterOutput { chapter: row })
    }
}
