//! Core vocabulary: entity types, IDs, parsed file shape, references.
//!
//! Every story artifact is one of the [`EntityType`] variants. Each is
//! represented on disk by a Markdown file with a YAML front matter block; the
//! parsed shape is [`ParsedEntity`].

use std::fmt;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The eight kinds of artifacts a story can contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EntityType {
    Story,
    Bible,
    Character,
    Environment,
    Prop,
    Chapter,
    Scene,
    Shot,
}

impl EntityType {
    /// Stable string used in front matter and error messages.
    pub const fn as_str(self) -> &'static str {
        match self {
            EntityType::Story => "story",
            EntityType::Bible => "bible",
            EntityType::Character => "character",
            EntityType::Environment => "environment",
            EntityType::Prop => "prop",
            EntityType::Chapter => "chapter",
            EntityType::Scene => "scene",
            EntityType::Shot => "shot",
        }
    }
}

impl fmt::Display for EntityType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Front-matter `id` field. Wrapped to give call sites a real type rather
/// than passing raw `String` everywhere.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntityId(pub String);

impl EntityId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for EntityId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for EntityId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Reference kind.
///
/// - `Character` / `Environment` / `Prop` — a scene points at these
///   via its front matter.
/// - `Shot` — a scene contains shots; the edge is parent_scene →
///   shot. We emit it the same way as the others so the validator
///   and the UI see a uniform graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReferenceKind {
    Character,
    Environment,
    Prop,
    Shot,
}

impl ReferenceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            ReferenceKind::Character => "character",
            ReferenceKind::Environment => "environment",
            ReferenceKind::Prop => "prop",
            ReferenceKind::Shot => "shot",
        }
    }
}

impl fmt::Display for ReferenceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A directed edge between two entities, as extracted from a scene's front
/// matter (`characters: [...]`, `environment: ...`, `props: [...]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub from: EntityId,
    pub to: EntityId,
    pub kind: ReferenceKind,
}

/// One Markdown file, parsed into its front matter (kept as YAML value for
/// flexibility) and its body. `path` is relative to the story root.
#[derive(Debug, Clone)]
pub struct ParsedEntity {
    pub id: EntityId,
    pub type_: EntityType,
    pub slug: String,
    pub path: PathBuf,
    /// Raw YAML front matter. Per-type shape is enforced by [`schema`].
    pub frontmatter: serde_yaml::Value,
    pub body: String,
}