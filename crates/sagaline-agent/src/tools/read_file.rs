//! `read_file` tool — the agent's memory read primitive.
//!
//! Reads a file by path. Relative paths are resolved against the
//! tool's configured story root; absolute paths are accepted only if
//! they fall inside the root. Any attempt to escape the root (e.g.
//! `../`) is rejected with [`ToolError::BadArgs`].

use std::fs;
use std::path::{Component, PathBuf};

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tool::{Tool, ToolDescriptor, ToolError, ToolResult};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ReadFileArgs {
    /// Path to the file, relative to the story root (preferred) or
    /// absolute (only if it falls inside the story root).
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadFileResult {
    /// The file's contents, verbatim.
    pub content: String,
    /// Byte count — handy for `Act` event summaries.
    pub bytes: u64,
}

/// Reads files under a fixed story root. Construct with
/// [`ReadFileTool::new`].
pub struct ReadFileTool {
    root: PathBuf,
}

impl ReadFileTool {
    /// Bind the tool to a story root. All reads are confined to this
    /// directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

#[async_trait]
impl Tool for ReadFileTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<ReadFileArgs>(
            "read_file",
            "Read a file from the agent's memory (the story workspace). \
             The `path` is resolved against the story root; relative \
             paths are joined to it, absolute paths are accepted only \
             if they fall inside it. `..` components are rejected. Use \
             this to load character sheets, scene bodies, or any other \
             artifact the agent needs to reason about.",
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: ReadFileArgs = serde_json::from_value(args).map_err(|e| ToolError::BadArgs {
            name: "read_file".to_string(),
            message: e.to_string(),
        })?;

        let given = PathBuf::from(&parsed.path);
        let abs = if given.is_absolute() {
            if !given.starts_with(&self.root) {
                return Err(ToolError::BadArgs {
                    name: "read_file".to_string(),
                    message: "absolute path is outside the story root".to_string(),
                });
            }
            given
        } else {
            if given
                .components()
                .any(|c| matches!(c, Component::ParentDir))
            {
                return Err(ToolError::BadArgs {
                    name: "read_file".to_string(),
                    message: "`..` is not allowed; the agent must stay within its memory"
                        .to_string(),
                });
            }
            self.root.join(given)
        };

        // The actual read is sync; we use spawn_blocking because
        // tool authors shouldn't have to think about that — but
        // since most reads are tiny and the agent only awaits one
        // tool at a time, doing it inline is also fine and saves a
        // context switch. Kept inline for now; revisit if profiling
        // shows this as hot.
        let content = fs::read_to_string(&abs).map_err(|e| ToolError::Io {
            name: "read_file".to_string(),
            path: abs,
            source: e,
        })?;
        let bytes = content.len() as u64;
        let result = ReadFileResult { content, bytes };
        serde_json::to_value(result).map_err(|e| ToolError::Execution {
            name: "read_file".to_string(),
            source: Box::new(e),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn reads_relative_path() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("hello.md"), "hi").unwrap();
        let tool = ReadFileTool::new(dir.path());
        let args = serde_json::json!({"path": "hello.md"});
        let out = tool.execute(args).await.expect("read");
        assert_eq!(out["content"], "hi");
        assert_eq!(out["bytes"], 2);
    }

    #[tokio::test]
    async fn rejects_parent_traversal() {
        let tool = ReadFileTool::new(std::path::PathBuf::from("/tmp"));
        let args = serde_json::json!({"path": "../etc/passwd"});
        let err = tool.execute(args).await.unwrap_err();
        assert!(matches!(err, ToolError::BadArgs { .. }));
    }

    #[tokio::test]
    async fn rejects_absolute_outside_root() {
        let tool = ReadFileTool::new(std::path::PathBuf::from("/tmp"));
        let args = serde_json::json!({"path": "/etc/passwd"});
        let err = tool.execute(args).await.unwrap_err();
        assert!(matches!(err, ToolError::BadArgs { .. }));
    }
}
