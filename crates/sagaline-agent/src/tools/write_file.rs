//! `write_file` agent tool — write or overwrite a file under a fixed
//! story root.
//!
//! The agent uses this to write shot frontmatter back to disk after
//! generation. Mirrors [`ReadFileTool`]'s path-confinement
//! guarantee: any attempt to escape the root (`../`, absolute paths
//! outside the root) is rejected with [`ToolError::BadArgs`].

use std::fs;
use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tool::{Tool, ToolDescriptor, ToolError, ToolResult};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct WriteFileArgs {
    /// Path to write, relative to the story root (preferred) or
    /// absolute (only if it falls inside the story root).
    pub path: String,
    /// The full file contents.
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
struct WriteFileOutput {
    path: String,
    bytes: u64,
}

/// Write files under a fixed story root. Construct with
/// [`WriteFileTool::new`].
#[derive(Clone)]
pub struct WriteFileTool {
    root: PathBuf,
}

impl WriteFileTool {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<WriteFileArgs>(
            "write_file",
            "Write a file under the story root. Overwrites if the file exists; \
             creates parent directories. Path is relative to the story root \
             (preferred) or absolute (only if it falls inside the root). \
             Returns `{path, bytes}`.",
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: WriteFileArgs = serde_json::from_value(args).map_err(|e| {
            ToolError::BadArgs {
                name: "write_file".into(),
                message: e.to_string(),
            }
        })?;
        let target = resolve_under_root(&self.root, Path::new(&parsed.path))?;
        if let Some(parent) = target.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| ToolError::Io {
                    name: "write_file".into(),
                    path: parent.to_path_buf(),
                    source: e,
                })?;
            }
        }
        fs::write(&target, parsed.content.as_bytes()).map_err(|e| ToolError::Io {
            name: "write_file".into(),
            path: target.clone(),
            source: e,
        })?;
        let bytes = parsed.content.len() as u64;
        let out = WriteFileOutput {
            path: target.to_string_lossy().into_owned(),
            bytes,
        };
        serde_json::to_value(out).map_err(|e| ToolError::Execution {
            name: "write_file".into(),
            source: Box::new(e),
        })
    }
}

/// Resolve `path` to an absolute path inside `root`. Reject anything
/// that escapes (contains `..` segments, or an absolute path that
/// isn't a descendant of `root`).
fn resolve_under_root(root: &Path, path: &Path) -> Result<PathBuf, ToolError> {
    if path.is_absolute() {
        // Absolute: must lie inside `root`.
        if !path.starts_with(root) {
            return Err(ToolError::BadArgs {
                name: "write_file".into(),
                message: format!(
                    "absolute path `{}` is outside the story root",
                    path.display()
                ),
            });
        }
        return Ok(path.to_path_buf());
    }

    // Relative: walk segments and reject any `..`.
    for c in path.components() {
        if matches!(c, Component::ParentDir) {
            return Err(ToolError::BadArgs {
                name: "write_file".into(),
                message: format!("path `{}` escapes the story root", path.display()),
            });
        }
    }
    Ok(root.join(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn writes_relative_path() {
        let dir = tempdir().unwrap();
        let tool = WriteFileTool::new(dir.path());
        let v = tool
            .execute(serde_json::json!({
                "path": "chapters/001-start/chapter.md",
                "content": "# hello\n",
            }))
            .await
            .unwrap();
        let on_disk = dir.path().join("chapters/001-start/chapter.md");
        assert!(on_disk.exists());
        assert_eq!(v["bytes"].as_u64().unwrap(), 8);
    }

    #[tokio::test]
    async fn rejects_parent_dir() {
        let dir = tempdir().unwrap();
        let tool = WriteFileTool::new(dir.path());
        let err = tool
            .execute(serde_json::json!({
                "path": "../escape.md",
                "content": "nope",
            }))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::BadArgs { .. }));
    }

    #[tokio::test]
    async fn accepts_absolute_inside_root() {
        let dir = tempdir().unwrap();
        let tool = WriteFileTool::new(dir.path());
        let abs = dir.path().join("inside.md");
        let v = tool
            .execute(serde_json::json!({
                "path": abs.to_string_lossy(),
                "content": "ok",
            }))
            .await
            .unwrap();
        assert!(abs.exists());
        assert_eq!(v["bytes"].as_u64().unwrap(), 2);
    }

    #[tokio::test]
    async fn rejects_absolute_outside_root() {
        let dir = tempdir().unwrap();
        let tool = WriteFileTool::new(dir.path());
        let err = tool
            .execute(serde_json::json!({
                "path": "/tmp/escape.md",
                "content": "nope",
            }))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::BadArgs { .. }));
    }
}
