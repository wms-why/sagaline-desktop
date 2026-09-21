//! `environment` domain tool.

use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use sagaline_store::{repo::EnvironmentRow, World};

use crate::tool::{Capability, Tool, ToolContext, ToolDescriptor, ToolError, ToolResult};
use crate::tools::domain;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateEnvironmentArgs {
    pub story_id: String,
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Serialize)]
pub struct CreateEnvironmentOutput {
    pub environment: EnvironmentRow,
}

pub struct CreateEnvironmentTool {
    world: Arc<World>,
}

impl CreateEnvironmentTool {
    pub fn new(world: Arc<World>) -> Self {
        Self { world }
    }
}

#[async_trait]
impl Tool for CreateEnvironmentTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<CreateEnvironmentArgs>(
            "create_environment",
            "Create a new Environment under a story. `slug` \
             must be unique within the story. Returns the \
             new environment row.",
            Capability::Mutate,
        )
    }

    async fn execute(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: CreateEnvironmentArgs = domain::parse_args("create_environment", args)?;
        let conn = self
            .world
            .conn()
            .map_err(|e| domain::map_store_err("create_environment", e))?;
        let id = uuid::Uuid::now_v7().to_string();
        let now = sagaline_store::time_util::now_iso();
        conn.execute(
            "INSERT INTO environments
                (id, story_id, slug, name, description, created_at, updated_at)
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
        .map_err(|e| domain::map_store_err("create_environment", e))?;
        let _ = ctx;
        let row = self
            .world
            .conn()
            .map_err(|e| domain::map_store_err("create_environment", e))?
            .query_row(
                "SELECT id, story_id, slug, name, description, created_at, updated_at \
                 FROM environments WHERE id = ?1",
                [&id],
                |r| {
                    Ok(EnvironmentRow {
                        id: r.get("id")?,
                        story_id: r.get("story_id")?,
                        slug: r.get("slug")?,
                        name: r.get("name")?,
                        description: r.get("description")?,
                        created_at: r.get("created_at")?,
                        updated_at: r.get("updated_at")?,
                    })
                },
            )
            .map_err(|e| domain::map_store_err("create_environment", e))?;
        domain::to_result(&CreateEnvironmentOutput { environment: row })
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
        let parsed: CreateEnvironmentArgs = domain::parse_args("create_environment", args)?;
        let row = self
            .world
            .environments()
            .create_in_tx(
                tx,
                sagaline_store::repo::NewEnvironment {
                    story_id: &parsed.story_id,
                    slug: &parsed.slug,
                    name: &parsed.name,
                    description: &parsed.description,
                },
            )
            .map_err(|e| domain::map_store_err("create_environment", e))?;
        domain::to_result(&CreateEnvironmentOutput { environment: row })
    }
}
