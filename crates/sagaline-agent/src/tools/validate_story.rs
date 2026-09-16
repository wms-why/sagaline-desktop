//! `validate_story` agent tool — call `StoryGraph::validate()`.
//!
//! Returns `{ok: bool, errors: [{kind, path, ...}]}`. The agent's
//! REFLECT step surfaces these.

use std::path::PathBuf;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use sagaline_core::StoryRoot;

use crate::tool::{Tool, ToolDescriptor, ToolError, ToolResult};

#[derive(Debug, Clone, JsonSchema, Deserialize)]
pub struct ValidateStoryArgs {
    /// Path to the story root directory.
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
struct ValidateStoryOutput {
    ok: bool,
    errors: Vec<ErrorJson>,
}

/// Serialized [`sagaline_core::ValidationError`]. We hand-roll this
/// rather than serializing the enum directly so the agent's JSON
/// shape stays stable across core refactors.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "error_kind", rename_all = "snake_case")]
enum ErrorJson {
    MissingField {
        path: String,
        field: String,
    },
    InvalidEnum {
        path: String,
        field: String,
        value: String,
        allowed: Vec<String>,
    },
    SlugMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    BrokenReference {
        path: String,
        kind: String,
        missing: String,
        available: Vec<String>,
    },
    DuplicateId {
        id: String,
        paths: Vec<String>,
    },
    InvalidAssetPath {
        path: String,
        asset: String,
        expected: String,
        actual: String,
    },
}

#[derive(Clone)]
pub struct ValidateStoryTool;

impl ValidateStoryTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ValidateStoryTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ValidateStoryTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<ValidateStoryArgs>(
            "validate_story",
            "Validate the story graph under the given root. Returns \
             `{ok: bool, errors: [{kind, path, ...}]}`. `kind` is one \
             of: missing_field, invalid_enum, slug_mismatch, \
             broken_reference, duplicate_id, invalid_asset_path.",
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: ValidateStoryArgs = serde_json::from_value(args).map_err(|e| {
            ToolError::BadArgs {
                name: "validate_story".into(),
                message: e.to_string(),
            }
        })?;
        let root = StoryRoot::new(&PathBuf::from(&parsed.path)).map_err(|e| {
            ToolError::Execution {
                name: "validate_story".into(),
                source: Box::new(e),
            }
        })?;
        let graph = sagaline_core::StoryGraph::load(&root).map_err(|e| ToolError::Execution {
            name: "validate_story".into(),
            source: Box::new(e),
        })?;
        let errs = match graph.validate() {
            Ok(()) => Vec::new(),
            Err(es) => es.into_iter().map(serialize_err).collect(),
        };
        let out = ValidateStoryOutput {
            ok: errs.is_empty(),
            errors: errs,
        };
        serde_json::to_value(out).map_err(|e| ToolError::Execution {
            name: "validate_story".into(),
            source: Box::new(e),
        })
    }
}

fn serialize_err(e: sagaline_core::ValidationError) -> ErrorJson {
    use sagaline_core::ValidationError as V;
    match e {
        V::MissingField { path, field } => ErrorJson::MissingField {
            path: path.to_string_lossy().into_owned(),
            field: field.to_string(),
        },
        V::InvalidEnum {
            path,
            field,
            value,
            allowed,
        } => ErrorJson::InvalidEnum {
            path: path.to_string_lossy().into_owned(),
            field: field.to_string(),
            value,
            allowed: allowed.into_iter().map(String::from).collect(),
        },
        V::SlugMismatch {
            path,
            expected,
            actual,
        } => ErrorJson::SlugMismatch {
            path: path.to_string_lossy().into_owned(),
            expected,
            actual,
        },
        V::BrokenReference {
            path,
            kind,
            missing,
            available,
        } => ErrorJson::BrokenReference {
            path: path.to_string_lossy().into_owned(),
            kind,
            missing,
            available,
        },
        V::DuplicateId { id, paths } => ErrorJson::DuplicateId {
            id,
            paths: paths
                .into_iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
        },
        V::InvalidAssetPath {
            path,
            asset,
            expected,
            actual,
        } => ErrorJson::InvalidAssetPath {
            path: path.to_string_lossy().into_owned(),
            asset,
            expected,
            actual,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn validates_a_clean_story() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("story.md"),
            "---\nid: story_demo\ntype: story\nslug: demo\ncreated_at: t\nupdated_at: t\n---\n",
        )
        .unwrap();

        let tool = ValidateStoryTool::new();
        let v = tool
            .execute(serde_json::json!({ "path": dir.path().to_string_lossy() }))
            .await
            .unwrap();
        assert_eq!(v["ok"], true);
        assert!(v["errors"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn runs_without_panic_on_broken_story() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("story.md"),
            "---\nid: story_demo\ntype: story\nslug: demo\ncreated_at: t\nupdated_at: t\n---\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("characters/lin-mo")).unwrap();
        fs::write(
            dir.path().join("characters/lin-mo/character.md"),
            "---\ntype: character\nslug: lin-mo\ncreated_at: t\nupdated_at: t\n---\n",
        )
        .unwrap();

        let tool = ValidateStoryTool::new();
        // Loading a story whose child is missing `id` is a hard
        // error from StoryGraph::load. We assert the tool surface
        // doesn't panic — the actual error mapping is exercised by
        // sagaline-core tests.
        let _ = tool
            .execute(serde_json::json!({ "path": dir.path().to_string_lossy() }))
            .await;
    }
}
