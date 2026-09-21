//! `scene` domain tools.
//!
//! - `create_scene` — multi-table mutation: writes a Scene row +
//!   the character / appearance / environment assignments in a
//!   single transaction (the discussion in `AGENTS.md` called
//!   this out specifically).
//! - `assign_character_to_scene` — link a character to a scene
//!   with optional age + appearance references.
//! - `assign_environment_to_scene` — link an environment to a
//!   scene.

use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use sagaline_store::{repo::SceneRow, World};

use crate::tool::{Capability, Tool, ToolContext, ToolDescriptor, ToolError, ToolResult};
use crate::tools::domain;

// ---- shared arg shapes --------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CharacterRefArgs {
    pub character_id: String,
    #[serde(default)]
    pub character_age_id: Option<String>,
    #[serde(default)]
    pub appearance_id: Option<String>,
}

// ---- create_scene -------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateSceneArgs {
    pub chapter_id: String,
    pub slug: String,
    pub ordinal: i64,
    pub title: String,
    #[serde(default)]
    pub synopsis: String,
    #[serde(default)]
    pub characters: Vec<CharacterRefArgs>,
    #[serde(default)]
    pub environment_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateSceneOutput {
    pub scene: SceneRow,
    pub character_assignments: usize,
    pub environment_assigned: bool,
}

pub struct CreateSceneTool {
    world: Arc<World>,
}

impl CreateSceneTool {
    pub fn new(world: Arc<World>) -> Self {
        Self { world }
    }
}

#[async_trait]
impl Tool for CreateSceneTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<CreateSceneArgs>(
            "create_scene",
            "Create a Scene, optionally linking characters (each \
             with optional `character_age_id` and `appearance_id`) \
             and an environment. All writes happen in one \
             transaction — either every row lands or none does. \
             Returns the new scene row + counts of the linked \
             assignments. This is the headline multi-table \
             mutation the discussion in `AGENTS.md` calls out.",
            Capability::Mutate,
        )
    }

    async fn execute(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: CreateSceneArgs = domain::parse_args("create_scene", args)?;
        let mut conn = self
            .world
            .conn()
            .map_err(|e| domain::map_store_err("create_scene", e))?;
        let tx = conn
            .transaction()
            .map_err(|e| domain::map_store_err("create_scene", e))?;
        let result = self.create_scene_in_tx(&parsed, &tx)?;
        tx.commit()
            .map_err(|e| domain::map_store_err("create_scene", e))?;
        let _ = ctx;
        domain::to_result(&result)
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
        let parsed: CreateSceneArgs = domain::parse_args("create_scene", args)?;
        let result = self.create_scene_in_tx(&parsed, tx)?;
        domain::to_result(&result)
    }
}

impl CreateSceneTool {
    /// Shared inner body for `execute` (auto-commit) and
    /// `execute_in_tx` (caller-supplied tx). Phase 5 keeps the
    /// multi-table INSERT sequence in one place.
    fn create_scene_in_tx(
        &self,
        parsed: &CreateSceneArgs,
        tx: &rusqlite::Transaction<'_>,
    ) -> Result<CreateSceneOutput, ToolError> {
        let scene = self
            .world
            .scenes()
            .create_scene_in_tx(
                tx,
                sagaline_store::repo::NewScene {
                    chapter_id: &parsed.chapter_id,
                    slug: &parsed.slug,
                    ordinal: parsed.ordinal,
                    title: &parsed.title,
                    synopsis: &parsed.synopsis,
                },
            )
            .map_err(|e| domain::map_store_err("create_scene", e))?;

        let mut n_assigned = 0usize;
        for cref in &parsed.characters {
            tx.execute(
                "INSERT INTO scene_characters \
                    (scene_id, character_id, character_age_id, appearance_id) \
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    scene.id,
                    cref.character_id,
                    cref.character_age_id,
                    cref.appearance_id,
                ],
            )
            .map_err(|e| domain::map_store_err("create_scene", e))?;
            n_assigned += 1;
        }

        let mut env_assigned = false;
        if let Some(env_id) = parsed.environment_id.as_deref() {
            tx.execute(
                "INSERT INTO scene_environments (scene_id, environment_id) \
                 VALUES (?1, ?2)",
                rusqlite::params![scene.id, env_id],
            )
            .map_err(|e| domain::map_store_err("create_scene", e))?;
            env_assigned = true;
        }

        Ok(CreateSceneOutput {
            scene,
            character_assignments: n_assigned,
            environment_assigned: env_assigned,
        })
    }
}

// ---- assign_character_to_scene ------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AssignCharacterToSceneArgs {
    pub scene_id: String,
    pub character_id: String,
    #[serde(default)]
    pub character_age_id: Option<String>,
    #[serde(default)]
    pub appearance_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AssignCharacterToSceneOutput {
    pub scene_id: String,
    pub character_id: String,
}

pub struct AssignCharacterToSceneTool {
    world: Arc<World>,
}

impl AssignCharacterToSceneTool {
    pub fn new(world: Arc<World>) -> Self {
        Self { world }
    }
}

#[async_trait]
impl Tool for AssignCharacterToSceneTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<AssignCharacterToSceneArgs>(
            "assign_character_to_scene",
            "Link a character to an existing scene. The pair \
             `(scene_id, character_id)` is unique; re-linking \
             with new age / appearance IDs replaces the prior \
             row. Useful when the scene was created without \
             characters and the agent wants to add them later.",
            Capability::Mutate,
        )
    }

    async fn execute(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: AssignCharacterToSceneArgs =
            domain::parse_args("assign_character_to_scene", args)?;
        let conn = self
            .world
            .conn()
            .map_err(|e| domain::map_store_err("assign_character_to_scene", e))?;
        conn.execute(
            "INSERT INTO scene_characters \
                (scene_id, character_id, character_age_id, appearance_id) \
             VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(scene_id, character_id) DO UPDATE SET \
                 character_age_id = excluded.character_age_id, \
                 appearance_id     = excluded.appearance_id",
            rusqlite::params![
                parsed.scene_id,
                parsed.character_id,
                parsed.character_age_id,
                parsed.appearance_id,
            ],
        )
        .map_err(|e| domain::map_store_err("assign_character_to_scene", e))?;
        let _ = ctx;
        domain::to_result(&AssignCharacterToSceneOutput {
            scene_id: parsed.scene_id,
            character_id: parsed.character_id,
        })
    }
}

// ---- assign_environment_to_scene ----------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AssignEnvironmentToSceneArgs {
    pub scene_id: String,
    pub environment_id: String,
}

#[derive(Debug, Serialize)]
pub struct AssignEnvironmentToSceneOutput {
    pub scene_id: String,
    pub environment_id: String,
}

pub struct AssignEnvironmentToSceneTool {
    world: Arc<World>,
}

impl AssignEnvironmentToSceneTool {
    pub fn new(world: Arc<World>) -> Self {
        Self { world }
    }
}

#[async_trait]
impl Tool for AssignEnvironmentToSceneTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<AssignEnvironmentToSceneArgs>(
            "assign_environment_to_scene",
            "Link an environment to an existing scene. The pair \
             `(scene_id, environment_id)` is unique; a second \
             call replaces the prior link.",
            Capability::Mutate,
        )
    }

    async fn execute(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: AssignEnvironmentToSceneArgs =
            domain::parse_args("assign_environment_to_scene", args)?;
        let conn = self
            .world
            .conn()
            .map_err(|e| domain::map_store_err("assign_environment_to_scene", e))?;
        conn.execute(
            "INSERT INTO scene_environments (scene_id, environment_id) \
             VALUES (?1, ?2) \
             ON CONFLICT(scene_id, environment_id) DO NOTHING",
            rusqlite::params![parsed.scene_id, parsed.environment_id],
        )
        .map_err(|e| domain::map_store_err("assign_environment_to_scene", e))?;
        let _ = ctx;
        domain::to_result(&AssignEnvironmentToSceneOutput {
            scene_id: parsed.scene_id,
            environment_id: parsed.environment_id,
        })
    }
}
