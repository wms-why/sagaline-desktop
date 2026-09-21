//! `prop` domain tool. Props are single-file entities (no
//! separate prop.md; Phase 2 keeps them slim).

use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use sagaline_store::World;

use crate::tool::{Capability, Tool, ToolContext, ToolDescriptor, ToolError, ToolResult};
use crate::tools::domain;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreatePropArgs {
    pub story_id: String,
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Serialize)]
pub struct CreatePropOutput {
    pub prop_id: String,
}

pub struct CreatePropTool {
    world: Arc<World>,
}

impl CreatePropTool {
    pub fn new(world: Arc<World>) -> Self {
        Self { world }
    }
}

#[async_trait]
impl Tool for CreatePropTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<CreatePropArgs>(
            "create_prop",
            "Create a new Prop under a story. `slug` must be \
             unique within the story. Returns the new prop id.",
            Capability::Mutate,
        )
    }

    async fn execute(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: CreatePropArgs = domain::parse_args("create_prop", args)?;
        let conn = self
            .world
            .conn()
            .map_err(|e| domain::map_store_err("create_prop", e))?;
        let id = uuid::Uuid::now_v7().to_string();
        let now = sagaline_store::time_util::now_iso();
        conn.execute(
            "INSERT INTO props (id, story_id, slug, name, description, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            rusqlite::params![
                id,
                parsed.story_id,
                parsed.slug,
                parsed.name,
                parsed.description,
                now,
            ],
        )
        .map_err(|e| domain::map_store_err("create_prop", e))?;
        let _ = ctx;
        domain::to_result(&CreatePropOutput { prop_id: id })
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
        let parsed: CreatePropArgs = domain::parse_args("create_prop", args)?;
        // Use the repo's _in_tx variant so we share the same
        // INSERT shape (the inline SQL in `execute` mirrors it
        // for the auto-commit path; both are kept identical).
        let row = self
            .world
            .props()
            .create_in_tx(
                tx,
                sagaline_store::repo::NewProp {
                    story_id: &parsed.story_id,
                    slug: &parsed.slug,
                    name: &parsed.name,
                    description: &parsed.description,
                },
            )
            .map_err(|e| domain::map_store_err("create_prop", e))?;
        domain::to_result(&CreatePropOutput { prop_id: row.id })
    }
}
