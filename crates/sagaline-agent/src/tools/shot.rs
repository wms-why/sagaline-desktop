//! `shot` domain tool. Phase 2 ships just `create_shot` — the
//! reference-assignment helpers land once the asset subsystem is
//! in.

use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use sagaline_store::World;

use crate::tool::{Capability, Tool, ToolContext, ToolDescriptor, ToolError, ToolResult};
use crate::tools::domain;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateShotArgs {
    pub scene_id: String,
    pub slug: String,
    pub ordinal: i64,
    pub title: String,
    #[serde(default)]
    pub duration_sec: Option<f64>,
    #[serde(default)]
    pub prompt: String,
}

#[derive(Debug, Serialize)]
pub struct CreateShotOutput {
    pub shot_id: String,
}

pub struct CreateShotTool {
    world: Arc<World>,
}

impl CreateShotTool {
    pub fn new(world: Arc<World>) -> Self {
        Self { world }
    }
}

#[async_trait]
impl Tool for CreateShotTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<CreateShotArgs>(
            "create_shot",
            "Create a new Shot under a scene. `slug` and \
             `ordinal` are unique within the scene. `prompt` \
             is the text-to-video / text-to-image prompt the \
             generation tools will use; `duration_sec` is the \
             target video length (None for image shots).",
            Capability::Mutate,
        )
    }

    async fn execute(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: CreateShotArgs = domain::parse_args("create_shot", args)?;
        let conn = self
            .world
            .conn()
            .map_err(|e| domain::map_store_err("create_shot", e))?;
        let id = uuid::Uuid::now_v7().to_string();
        let now = sagaline_store::time_util::now_iso();
        conn.execute(
            "INSERT INTO shots
                (id, scene_id, slug, ordinal, title, duration_sec, prompt,
                 created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            rusqlite::params![
                id,
                parsed.scene_id,
                parsed.slug,
                parsed.ordinal,
                parsed.title,
                parsed.duration_sec,
                parsed.prompt,
                now,
            ],
        )
        .map_err(|e| domain::map_store_err("create_shot", e))?;
        let _ = ctx;
        domain::to_result(&CreateShotOutput { shot_id: id })
    }
}
