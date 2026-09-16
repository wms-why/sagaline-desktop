//! `list_dir` agent tool — enumerate a story-relative directory.
//!
//! The agent uses this to discover chapters / scenes / shots / etc.
//! Output is a JSON array of `{path, type, slug}` entries. The
//! `type` is the [`EntityType`] string when the entry matches a
//! known layout; otherwise it's `"other"`.

use std::fs;
use std::path::Path;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use sagaline_core::path;

use crate::tool::{Tool, ToolDescriptor, ToolError, ToolResult};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ListDirArgs {
    /// Path relative to the story root.
    pub path: String,
    /// Recurse into subdirectories. Default: false.
    #[serde(default)]
    pub recursive: bool,
}

#[derive(Debug, Clone, Serialize)]
struct Entry {
    path: String,
    /// `EntityType::as_str()` for known layouts; `"other"` otherwise.
    #[serde(rename = "type")]
    type_: String,
    /// Slug (only set for tracked layouts; `None` for `other`).
    #[serde(skip_serializing_if = "Option::is_none")]
    slug: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ListDirOutput {
    entries: Vec<Entry>,
}

#[derive(Clone)]
pub struct ListDirTool {
    root: std::path::PathBuf,
}

impl ListDirTool {
    pub fn new(root: impl Into<std::path::PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

#[async_trait]
impl Tool for ListDirTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<ListDirArgs>(
            "list_dir",
            "List entries under a story-relative directory. Returns \
             `{entries: [{path, type, slug}]}` where `type` is the \
             entity type (\"scene\"/\"shot\"/…) for known layouts and \
             \"other\" otherwise. Set `recursive: true` to descend.",
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: ListDirArgs = serde_json::from_value(args).map_err(|e| {
            ToolError::BadArgs {
                name: "list_dir".into(),
                message: e.to_string(),
            }
        })?;
        let target = self.root.join(&parsed.path);
        let mut entries = vec![];
        visit(&target, &self.root, parsed.recursive, &mut entries)?;
        let out = ListDirOutput { entries };
        serde_json::to_value(out).map_err(|e| ToolError::Execution {
            name: "list_dir".into(),
            source: Box::new(e),
        })
    }
}

fn visit(
    abs_dir: &Path,
    root: &Path,
    recursive: bool,
    out: &mut Vec<Entry>,
) -> Result<(), ToolError> {
    let read = fs::read_dir(abs_dir).map_err(|e| ToolError::Io {
        name: "list_dir".into(),
        path: abs_dir.to_path_buf(),
        source: e,
    })?;
    for dent in read {
        let dent = dent.map_err(|e| ToolError::Io {
            name: "list_dir".into(),
            path: abs_dir.to_path_buf(),
            source: e,
        })?;
        let file_name = dent.file_name();
        let name = file_name.to_string_lossy();
        // Skip hidden + asset / references dirs (mirrors graph.rs).
        if name.starts_with('.') || name == "assets" || name == "references" {
            continue;
        }
        let abs = dent.path();
        let rel = abs
            .strip_prefix(root)
            .unwrap_or(&abs)
            .to_string_lossy()
            .into_owned();

        let meta = fs::metadata(&abs).map_err(|e| ToolError::Io {
            name: "list_dir".into(),
            path: abs.clone(),
            source: e,
        })?;

        if meta.is_dir() {
            if recursive {
                visit(&abs, root, recursive, out)?;
            }
            // Dirs themselves aren't typed; their descendants are.
            continue;
        }

        let (type_, slug) = match path::classify(Path::new(&rel)) {
            Some((t, s)) => (t.as_str().to_string(), Some(s)),
            None => ("other".to_string(), None),
        };
        out.push(Entry {
            path: rel,
            type_,
            slug,
        });
    }
    // Stable order: sort by path.
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn lists_typed_entries() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("characters/lin-mo")).unwrap();
        fs::write(
            dir.path().join("characters/lin-mo/character.md"),
            "---\nid: x\ntype: character\nslug: lin-mo\ncreated_at: t\nupdated_at: t\n---\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("story.md"),
            "---\nid: story\ntype: story\nslug: story\ncreated_at: t\nupdated_at: t\n---\n",
        )
        .unwrap();

        let tool = ListDirTool::new(dir.path());
        let v = tool
            .execute(serde_json::json!({ "path": "characters", "recursive": true }))
            .await
            .unwrap();
        let entries = v["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["type"], "character");
        assert_eq!(entries[0]["slug"], "lin-mo");
    }

    #[tokio::test]
    async fn skips_assets_and_hidden() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("assets")).unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join("assets/dummy.png"), b"x").unwrap();

        let tool = ListDirTool::new(dir.path());
        let v = tool
            .execute(serde_json::json!({ "path": "." }))
            .await
            .unwrap();
        assert_eq!(v["entries"].as_array().unwrap().len(), 0);
    }
}
