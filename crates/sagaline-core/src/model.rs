//! Domain entities for the Sagaline story / video workspace.
//!
//! These types are pure data: no SQLite, no GPUI, no I/O. They describe what a
//! long-form AI-generated video project actually is, in the shape defined by
//! the product spec:
//!
//! ```text
//! Story
//!   ├── StoryBible
//!   ├── Character
//!   │     ├── CharacterAge
//!   │     └── CharacterAppearance
//!   ├── Environment
//!   ├── Prop
//!   └── Chapter
//!         └── Scene
//!               └── Shot
//!                     ├── ShotCharacter
//!                     └── ShotProp
//! ```
//!
//! IDs are opaque newtypes so a `StoryId` and a `CharacterId` are not
//! interchangeable even though both wrap `String` under the hood.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// IDs
// ---------------------------------------------------------------------------

macro_rules! id_newtype {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new() -> Self {
                Self(uuid::Uuid::new_v4().to_string())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_string())
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id_newtype!(StoryId);
id_newtype!(CharacterId);
id_newtype!(CharacterAgeId);
id_newtype!(CharacterAppearanceId);
id_newtype!(EnvironmentId);
id_newtype!(PropId);
id_newtype!(ChapterId);
id_newtype!(SceneId);
id_newtype!(ShotId);
id_newtype!(ReferenceImageId);
id_newtype!(RelationshipId);

// ---------------------------------------------------------------------------
// Story
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StoryStatus {
    #[default]
    Draft,
    Active,
    Completed,
    Archived,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Story {
    pub id: StoryId,
    pub title: String,
    pub description: String,
    pub cover: Option<String>,
    pub genre: Option<String>,
    pub style: Option<String>,
    pub language: Option<String>,
    pub target_audience: Option<String>,
    pub visual_style: Option<String>,
    pub status: StoryStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Story Bible — the long-form story's "world memory".
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoryBible {
    pub story_id: StoryId,
    /// Where & when the story takes place (time, geography, politics, technology,
    /// society, history, world rules). One freeform `Markdown`-shaped blob per
    /// the spec; an editor can split it later.
    pub world: String,
    /// Story-level rules (magic, tech, abilities, character relations,
    /// plot constraints).
    pub rules: String,
    /// Timeline anchor — in-story date of the first event.
    pub timeline: String,
    /// Anything else worth carrying forward (lore, glossary, motifs).
    pub lore: String,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Characters
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Character {
    pub id: CharacterId,
    pub story_id: StoryId,
    pub name: String,
    pub profile: String,
    pub personality: String,
    pub background: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A specific age variant of a character. Each age variant owns its own
/// reference images; an Appearance hangs off an age.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CharacterAge {
    pub id: CharacterAgeId,
    pub character_id: CharacterId,
    pub age: u32,
    pub description: String,
    pub default_reference: Option<ReferenceImageId>,
    pub created_at: DateTime<Utc>,
}

/// A clothing / state / look variant for a character at a specific age.
/// Independent from age because the same 28-year-old wears Casual, Suit,
/// Combat, Injured, Winter, etc.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CharacterAppearance {
    pub id: CharacterAppearanceId,
    pub age_id: CharacterAgeId,
    pub name: String,
    pub description: String,
    pub clothing: String,
    pub hairstyle: String,
    pub accessories: String,
    pub emotion: String,
    pub body_state: String,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Reference images (shared between character ages, environments, props, shots)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    #[default]
    Character,
    Environment,
    Prop,
    Shot,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReferenceImage {
    pub id: ReferenceImageId,
    pub kind: ReferenceKind,
    /// Free-form label, e.g. "林默-28岁-战斗服" or "深圳地下城-入口".
    pub label: String,
    /// Path / URL — the renderer doesn't interpret it; the BYOK pipeline does.
    pub source: String,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Environment (location) — a long-lived reusable asset
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Environment {
    pub id: EnvironmentId,
    pub story_id: StoryId,
    pub name: String,
    pub description: String,
    pub architecture: String,
    pub lighting: String,
    pub weather: String,
    pub time_of_day: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Prop — recurring objects in the story world
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Prop {
    pub id: PropId,
    pub story_id: StoryId,
    pub name: String,
    pub description: String,
    pub appearance: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Character relationships
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipStatus {
    #[default]
    Active,
    Broken,
    Ended,
    Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Relationship {
    pub id: RelationshipId,
    pub story_id: StoryId,
    pub from_character: CharacterId,
    pub to_character: CharacterId,
    /// "lover", "mentor", "brother", "rival", etc. Free-form for now.
    pub kind: String,
    pub status: RelationshipStatus,
    pub note: String,
}

// ---------------------------------------------------------------------------
// Chapter → Scene → Shot
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChapterStatus {
    #[default]
    Draft,
    Planning,
    Generating,
    Generated,
    Published,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Chapter {
    pub id: ChapterId,
    pub story_id: StoryId,
    pub chapter_number: u32,
    pub title: String,
    pub summary: String,
    pub story: String,
    pub timeline: String,
    pub status: ChapterStatus,
    pub generated_video: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scene {
    pub id: SceneId,
    pub chapter_id: ChapterId,
    pub scene_number: u32,
    pub title: String,
    pub description: String,
    /// Optional explicit environment override. Empty means "pick from chapter".
    pub environment_id: Option<EnvironmentId>,
    pub emotion: String,
    pub duration_seconds: u32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Shot {
    pub id: ShotId,
    pub scene_id: SceneId,
    pub shot_number: u32,
    pub description: String,
    pub camera: String,
    pub camera_movement: String,
    pub composition: String,
    pub action: String,
    pub dialogue: String,
    pub emotion: String,
    pub duration_seconds: u32,
    pub environment_id: Option<EnvironmentId>,
    pub first_frame: Option<String>,
    pub last_frame: Option<String>,
    pub video_model: Option<String>,
    pub generation_settings: String,
    pub generated_video: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// A character's appearance-in-this-shot (joins Character × Age × Appearance).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShotCharacter {
    pub shot_id: ShotId,
    pub character_id: CharacterId,
    pub age_id: CharacterAgeId,
    pub appearance_id: CharacterAppearanceId,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShotProp {
    pub shot_id: ShotId,
    pub prop_id: PropId,
}

// ---------------------------------------------------------------------------
// Aggregates — the read shape the UI actually consumes.
// ---------------------------------------------------------------------------

/// Everything needed to render a full Story workspace page. Built by the
/// persistence layer with one query (or several small joins).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoryWorkspace {
    pub story: Option<Story>,
    pub bible: Option<StoryBible>,
    pub characters: Vec<CharacterWithAssets>,
    pub environments: Vec<EnvironmentWithAssets>,
    pub props: Vec<PropWithAssets>,
    pub chapters: Vec<ChapterWithAssets>,
    pub relationships: Vec<Relationship>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CharacterWithAssets {
    pub character: Character,
    pub ages: Vec<CharacterAgeWithAssets>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CharacterAgeWithAssets {
    pub age: CharacterAge,
    pub appearances: Vec<CharacterAppearance>,
    pub references: Vec<ReferenceImage>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnvironmentWithAssets {
    pub environment: Environment,
    pub references: Vec<ReferenceImage>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PropWithAssets {
    pub prop: Prop,
    pub references: Vec<ReferenceImage>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChapterWithAssets {
    pub chapter: Chapter,
    pub scenes: Vec<SceneWithAssets>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SceneWithAssets {
    pub scene: Scene,
    pub shots: Vec<ShotWithAssets>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShotWithAssets {
    pub shot: Shot,
    pub characters: Vec<ShotCharacter>,
    pub props: Vec<Prop>,
    pub references: Vec<ReferenceImage>,
}

// ---------------------------------------------------------------------------
// Tests — smoke check that newtypes and serde round-trip without UI.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_distinct_after_serde() {
        let s = StoryId::new();
        let c = CharacterId::new();
        let json = serde_json::to_string(&(&s, &c)).unwrap();
        assert!(json.contains('"'));
        // Two different ids deserialize back as different types; the wrapper
        // names are erased at runtime, but the structural type is enforced by
        // the type system at compile time.
        assert_ne!(s.as_str(), c.as_str());
    }

    #[test]
    fn story_round_trips() {
        let story = Story {
            id: StoryId::new(),
            title: "末日之城".into(),
            description: "2057 年能源灾难后的世界".into(),
            cover: None,
            genre: Some("Sci-Fi".into()),
            style: None,
            language: Some("zh".into()),
            target_audience: None,
            visual_style: Some("Cinematic Realistic".into()),
            status: StoryStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let j = serde_json::to_string(&story).unwrap();
        let back: Story = serde_json::from_str(&j).unwrap();
        assert_eq!(back.title, story.title);
        assert_eq!(back.status, StoryStatus::Active);
    }
}
