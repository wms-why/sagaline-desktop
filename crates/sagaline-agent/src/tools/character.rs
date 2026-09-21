//! `character` domain tools.
//!
//! - `create_character` — register a new character under a story.
//! - `update_character` — overwrite the character's editable
//!   fields (name / occupation / bio).
//! - `add_character_age` — append an `(age, note)` variant.
//! - `add_character_appearance` — append a `(label, description)`
//!   appearance variant.

use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use sagaline_store::{repo::CharacterRow, World};

use crate::tool::{Capability, Tool, ToolContext, ToolDescriptor, ToolError, ToolResult};
use crate::tools::domain;

// ---- create_character -----------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateCharacterArgs {
    pub story_id: String,
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub occupation: Option<String>,
    #[serde(default)]
    pub bio: String,
}

#[derive(Debug, Serialize)]
pub struct CreateCharacterOutput {
    pub character: CharacterRow,
}

pub struct CreateCharacterTool {
    world: Arc<World>,
}

impl CreateCharacterTool {
    pub fn new(world: Arc<World>) -> Self {
        Self { world }
    }
}

#[async_trait]
impl Tool for CreateCharacterTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<CreateCharacterArgs>(
            "create_character",
            "Register a new character under a story. The `slug` \
             must be unique within the story. Returns \
             `{character: {id, story_id, slug, name, ...}}`.",
            Capability::Mutate,
        )
    }

    async fn execute(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: CreateCharacterArgs = domain::parse_args("create_character", args)?;
        let row = self
            .world
            .characters()
            .create(sagaline_store::repo::NewCharacter {
                story_id: &parsed.story_id,
                slug: &parsed.slug,
                name: &parsed.name,
                occupation: parsed.occupation.as_deref(),
                bio: &parsed.bio,
            })
            .map_err(|e| domain::map_store_err("create_character", e))?;
        let _ = ctx; // Phase 3: write agent_actions audit row
        domain::to_result(&CreateCharacterOutput { character: row })
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
        let parsed: CreateCharacterArgs = domain::parse_args("create_character", args)?;
        let row = self
            .world
            .characters()
            .create_in_tx(
                tx,
                sagaline_store::repo::NewCharacter {
                    story_id: &parsed.story_id,
                    slug: &parsed.slug,
                    name: &parsed.name,
                    occupation: parsed.occupation.as_deref(),
                    bio: &parsed.bio,
                },
            )
            .map_err(|e| domain::map_store_err("create_character", e))?;
        domain::to_result(&CreateCharacterOutput { character: row })
    }
}

// ---- update_character -----------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateCharacterArgs {
    pub character_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub occupation: Option<String>,
    #[serde(default)]
    pub bio: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateCharacterOutput {
    pub character: CharacterRow,
}

pub struct UpdateCharacterTool {
    world: Arc<World>,
}

impl UpdateCharacterTool {
    pub fn new(world: Arc<World>) -> Self {
        Self { world }
    }
}

#[async_trait]
impl Tool for UpdateCharacterTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<UpdateCharacterArgs>(
            "update_character",
            "Overwrite a character's editable fields. Any of \
             `name`, `occupation`, `bio` may be `null` to leave \
             it untouched. Returns the updated row.",
            Capability::Mutate,
        )
    }

    async fn execute(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: UpdateCharacterArgs = domain::parse_args("update_character", args)?;
        let conn = self
            .world
            .conn()
            .map_err(|e| domain::map_store_err("update_character", e))?;
        let mut row: CharacterRow = conn
            .query_row(
                "SELECT id, story_id, slug, name, occupation, bio, created_at, updated_at \
                 FROM characters WHERE id = ?1",
                [&parsed.character_id],
                |r| {
                    Ok(CharacterRow {
                        id: r.get("id")?,
                        story_id: r.get("story_id")?,
                        slug: r.get("slug")?,
                        name: r.get("name")?,
                        occupation: r.get("occupation")?,
                        bio: r.get("bio")?,
                        created_at: r.get("created_at")?,
                        updated_at: r.get("updated_at")?,
                    })
                },
            )
            .map_err(|e| domain::map_store_err("update_character", e))?;
        if let Some(name) = parsed.name {
            row.name = name;
        }
        if let Some(occ) = parsed.occupation {
            row.occupation = Some(occ);
        }
        if let Some(bio) = parsed.bio {
            row.bio = bio;
        }
        let now = sagaline_store::time_util::now_iso();
        conn.execute(
            "UPDATE characters SET name = ?1, occupation = ?2, bio = ?3, updated_at = ?4 \
             WHERE id = ?5",
            rusqlite::params![row.name, row.occupation, row.bio, now, parsed.character_id,],
        )
        .map_err(|e| domain::map_store_err("update_character", e))?;
        row.updated_at = now;
        let _ = ctx;
        domain::to_result(&UpdateCharacterOutput { character: row })
    }
}

// ---- add_character_age ----------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddCharacterAgeArgs {
    pub character_id: String,
    pub age: i64,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Serialize)]
pub struct AddCharacterAgeOutput {
    pub age_id: String,
}

pub struct AddCharacterAgeTool {
    world: Arc<World>,
}

impl AddCharacterAgeTool {
    pub fn new(world: Arc<World>) -> Self {
        Self { world }
    }
}

#[async_trait]
impl Tool for AddCharacterAgeTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<AddCharacterAgeArgs>(
            "add_character_age",
            "Append an `(age, note)` variant to a character. The \
             pair `(character_id, age)` is unique. Returns the \
             new age row's id.",
            Capability::Mutate,
        )
    }

    async fn execute(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: AddCharacterAgeArgs = domain::parse_args("add_character_age", args)?;
        let conn = self
            .world
            .conn()
            .map_err(|e| domain::map_store_err("add_character_age", e))?;
        let id = uuid::Uuid::now_v7().to_string();
        let now = sagaline_store::time_util::now_iso();
        conn.execute(
            "INSERT INTO character_ages (id, character_id, age, note, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, parsed.character_id, parsed.age, parsed.note, now],
        )
        .map_err(|e| domain::map_store_err("add_character_age", e))?;
        let _ = ctx;
        domain::to_result(&AddCharacterAgeOutput { age_id: id })
    }
}

// ---- add_character_appearance --------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddCharacterAppearanceArgs {
    pub character_id: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Serialize)]
pub struct AddCharacterAppearanceOutput {
    pub appearance_id: String,
}

pub struct AddCharacterAppearanceTool {
    world: Arc<World>,
}

impl AddCharacterAppearanceTool {
    pub fn new(world: Arc<World>) -> Self {
        Self { world }
    }
}

#[async_trait]
impl Tool for AddCharacterAppearanceTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<AddCharacterAppearanceArgs>(
            "add_character_appearance",
            "Append a `(label, description)` appearance variant \
             to a character. The pair `(character_id, label)` \
             is unique. Returns the new appearance row's id.",
            Capability::Mutate,
        )
    }

    async fn execute(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: AddCharacterAppearanceArgs =
            domain::parse_args("add_character_appearance", args)?;
        let conn = self
            .world
            .conn()
            .map_err(|e| domain::map_store_err("add_character_appearance", e))?;
        let id = uuid::Uuid::now_v7().to_string();
        let now = sagaline_store::time_util::now_iso();
        conn.execute(
            "INSERT INTO character_appearances \
                (id, character_id, label, description, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                id,
                parsed.character_id,
                parsed.label,
                parsed.description,
                now
            ],
        )
        .map_err(|e| domain::map_store_err("add_character_appearance", e))?;
        let _ = ctx;
        domain::to_result(&AddCharacterAppearanceOutput { appearance_id: id })
    }
}
