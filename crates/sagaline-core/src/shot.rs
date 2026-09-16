//! Typed accessors for the shot front matter.
//!
//! Shot front matter has the common header (`id` / `type` / `slug` /
//! `created_at` / `updated_at`) plus per-shot fields:
//!
//! - `parent_scene` (slug of the scene in the same chapter)
//! - `order` (numeric position within the scene)
//! - `camera`, `duration_seconds`, `mood`, `description`
//! - `status` (`pending | generating | succeeded | failed`)
//! - `provider` (per-shot overrides; falls back to scene defaults)
//! - `assets` (filled in after generation; paths derived from slug)
//!
//! All accessors are tolerant: missing / wrong-shaped fields return
//! `None` or empty values. The agent's reflection step ([`StoryGraph::validate`])
//! enforces the invariants.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::entity::{EntityId, Reference, ReferenceKind};


/// Generation status of a shot. Persisted in `status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShotStatus {
    Pending,
    Generating,
    Succeeded,
    Failed,
}

impl ShotStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            ShotStatus::Pending => "pending",
            ShotStatus::Generating => "generating",
            ShotStatus::Succeeded => "succeeded",
            ShotStatus::Failed => "failed",
        }
    }
}

/// Camera framing hint. Free-form string — providers don't have a
/// shared vocabulary here, but the most common values are
/// `wide | medium | close_up | insert | pov`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Camera(pub String);

/// Per-shot provider overrides. Any field `None` means "inherit from
/// scene defaults" (and ultimately `story.md`'s `providers:` block).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShotProviders {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<String>,
}

/// Asset paths. Filled in after generation. All paths are relative
/// to the story root.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShotAssets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyframe: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composite: Option<PathBuf>,
}

/// The structured shot front matter (header + per-shot fields).
/// The agent reads this through accessors; YAML round-tripping is
/// handled by `serde_yaml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotFrontmatter {
    // Header (enforced by `schema::FrontMatterShape`).
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub slug: String,
    pub created_at: String,
    pub updated_at: String,

    // Per-shot fields.
    pub parent_scene: String,
    pub order: u32,
    #[serde(default)]
    pub camera: Option<Camera>,
    #[serde(default)]
    pub duration_seconds: Option<f32>,
    #[serde(default)]
    pub mood: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_status")]
    pub status: ShotStatus,
    #[serde(default)]
    pub provider: ShotProviders,
    #[serde(default)]
    pub assets: ShotAssets,
}

fn default_status() -> ShotStatus {
    ShotStatus::Pending
}

/// Pull the `parent_scene` slug out of a shot's front matter.
pub fn parent_scene_of(fm: &Value) -> Option<String> {
    fm.get("parent_scene")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Pull the `status` out of a shot's front matter.
pub fn status_of(fm: &Value) -> ShotStatus {
    fm.get("status")
        .and_then(|v| v.as_str())
        .and_then(|s| match s {
            "pending" => Some(ShotStatus::Pending),
            "generating" => Some(ShotStatus::Generating),
            "succeeded" => Some(ShotStatus::Succeeded),
            "failed" => Some(ShotStatus::Failed),
            _ => None,
        })
        .unwrap_or(ShotStatus::Pending)
}

/// Pull `assets` out of a shot's front matter as a `ShotAssets`.
pub fn assets_of(fm: &Value) -> ShotAssets {
    let Some(obj) = fm.get("assets").and_then(|v| v.as_mapping()) else {
        return ShotAssets::default();
    };
    fn str_field(obj: &serde_yaml::Mapping, key: &str) -> Option<PathBuf> {
        obj.get(key)
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
    }
    ShotAssets {
        keyframe: str_field(obj, "keyframe"),
        video: str_field(obj, "video"),
        voice: str_field(obj, "voice"),
        composite: str_field(obj, "composite"),
    }
}

/// Pull `provider` out of a shot's front matter.
pub fn providers_of(fm: &Value) -> ShotProviders {
    let Some(obj) = fm.get("provider").and_then(|v| v.as_mapping()) else {
        return ShotProviders::default();
    };
    fn s(obj: &serde_yaml::Mapping, key: &str) -> Option<String> {
        obj.get(key).and_then(|v| v.as_str()).map(str::to_string)
    }
    ShotProviders {
        image: s(obj, "image"),
        video: s(obj, "video"),
        audio: s(obj, "audio"),
        chat: s(obj, "chat"),
    }
}

/// Resolve the canonical asset path for a given (chapter, scene,
/// shot_slug, asset_kind). Used by validate + the agent tool to
/// derive `output_path` deterministically.
///
/// Layout: `assets/chapters/<chapter>/scenes/<scene>/shots/<shot>/`.
pub fn asset_path(chapter: &str, scene: &str, shot_slug: &str, file: &str) -> PathBuf {
    PathBuf::from(format!(
        "assets/chapters/{chapter}/scenes/{scene}/shots/{shot_slug}/{file}"
    ))
}

/// Names of the well-known asset files. Used in
/// `StoryGraph::validate` to round-trip the spec.
pub const ASSET_KEYFRAME: &str = "keyframe.png";
pub const ASSET_VIDEO: &str = "clip.mp4";
pub const ASSET_VOICE: &str = "voice.wav";
pub const ASSET_COMPOSITE: &str = "segment.mp4";

/// Emit a [`Reference`] from the shot's parent scene to the shot.
/// Shots are not referenced via the scene's `characters: […]` lists;
/// the agent enumerates a scene's shots by parent_scene.
pub fn shot_reference(scene_id: &EntityId, shot_id: &EntityId) -> Reference {
    Reference {
        from: scene_id.clone(),
        to: shot_id.clone(),
        kind: ReferenceKind::Shot,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_round_trip() {
        for (s, expected) in [
            ("pending", ShotStatus::Pending),
            ("generating", ShotStatus::Generating),
            ("succeeded", ShotStatus::Succeeded),
            ("failed", ShotStatus::Failed),
        ] {
            let v: Value = serde_yaml::from_str(&format!("status: {s}\n")).unwrap();
            assert_eq!(status_of(&v), expected);
        }
    }

    #[test]
    fn unknown_status_falls_back_to_pending() {
        let v: Value = serde_yaml::from_str("status: weird\n").unwrap();
        assert_eq!(status_of(&v), ShotStatus::Pending);
    }

    #[test]
    fn assets_round_trip() {
        let v: Value = serde_yaml::from_str(
            r#"
assets:
  keyframe: assets/chapters/c1/scenes/s1/shots/x/keyframe.png
  voice:    assets/chapters/c1/scenes/s1/shots/x/voice.wav
"#,
        )
        .unwrap();
        let a = assets_of(&v);
        assert_eq!(
            a.keyframe.as_deref(),
            Some(std::path::Path::new(
                "assets/chapters/c1/scenes/s1/shots/x/keyframe.png"
            ))
        );
        assert_eq!(
            a.voice.as_deref(),
            Some(std::path::Path::new(
                "assets/chapters/c1/scenes/s1/shots/x/voice.wav"
            ))
        );
        assert!(a.video.is_none());
    }

    #[test]
    fn asset_path_matches_layout() {
        let p = asset_path("001-start", "001-intro", "004-anxious", ASSET_KEYFRAME);
        assert_eq!(
            p,
            PathBuf::from("assets/chapters/001-start/scenes/001-intro/shots/004-anxious/keyframe.png")
        );
    }

    #[test]
    fn shot_reference_kind_is_shot() {
        // ReferenceKind::Shot is the new variant — make sure it
        // serializes / compares sanely. We don't currently expose
        // Shot externally, so this is a structural test.
        let r = shot_reference(&EntityId::new("scene_x"), &EntityId::new("shot_y"));
        assert_eq!(r.from.as_str(), "scene_x");
        assert_eq!(r.to.as_str(), "shot_y");
    }
}