//! `find` agent tool — literal substring search across Markdown
//! files under the story root.
//!
//! Avoids the `regex` dep gate (no regex semantics yet; literal
//! matches only). The agent uses this to locate entity ids / slugs
//! across the workspace.

use std::fs;
use std::path::Path;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tool::{Tool, ToolDescriptor, ToolError, ToolResult};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FindArgs {
    /// Path under which to search, relative to the story root.
    pub path: String,
    /// literal substring to search for (case-sensitive).
    pub needle: String,
    /// Search bodies (Markdown text) in addition to frontmatter.
    /// Default: false (frontmatter only — cheaper, more deterministic).
    #[serde(default)]
    pub include_bodies: bool,
}

#[derive(Debug, Clone, Serialize)]
struct Match {
    path: String,
    /// `"frontmatter"` or `"body"`.
    region: &'static str,
    /// Byte offset within the file's UTF-8 text.
    offset: u64,
    /// The matching line.
    line: String,
}

#[derive(Debug, Clone, Serialize)]
struct FindOutput {
    matches: Vec<Match>,
}

#[derive(Clone)]
pub struct FindTool {
    root: std::path::PathBuf,
}

impl FindTool {
    pub fn new(root: impl Into<std::path::PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

#[async_trait]
impl Tool for FindTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<FindArgs>(
            "find",
            "Search for a literal substring across Markdown files under a story-relative \
             directory. Returns `{matches: [{path, region, offset, line}]}`. Set \
             `include_bodies: true` to also scan the Markdown body (frontmatter is \
             always scanned).",
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: FindArgs = serde_json::from_value(args).map_err(|e| {
            ToolError::BadArgs {
                name: "find".into(),
                message: e.to_string(),
            }
        })?;
        let target = self.root.join(&parsed.path);
        let mut out = Vec::new();
        scan(&target, &self.root, &parsed, &mut out)?;
        let out = FindOutput { matches: out };
        serde_json::to_value(out).map_err(|e| ToolError::Execution {
            name: "find".into(),
            source: Box::new(e),
        })
    }
}

fn scan(
    abs_dir: &Path,
    root: &Path,
    args: &FindArgs,
    out: &mut Vec<Match>,
) -> Result<(), ToolError> {
    let read = fs::read_dir(abs_dir).map_err(|e| ToolError::Io {
        name: "find".into(),
        path: abs_dir.to_path_buf(),
        source: e,
    })?;
    for dent in read {
        let dent = dent.map_err(|e| ToolError::Io {
            name: "find".into(),
            path: abs_dir.to_path_buf(),
            source: e,
        })?;
        let file_name = dent.file_name();
        let name = file_name.to_string_lossy();
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
            name: "find".into(),
            path: abs.clone(),
            source: e,
        })?;
        if meta.is_dir() {
            scan(&abs, root, args, out)?;
            continue;
        }

        if !rel.ends_with(".md") {
            continue;
        }

        let text = fs::read_to_string(&abs).map_err(|e| ToolError::Io {
            name: "find".into(),
            path: abs.clone(),
            source: e,
        })?;
        if let Some(fm_end) = find_frontmatter_end(&text) {
            let (fm, body) = text.split_at(fm_end);
            scan_text(&rel, "frontmatter", fm, &args.needle, out);
            if args.include_bodies {
                scan_text(&rel, "body", body, &args.needle, out);
            }
        } else {
            scan_text(&rel, "body", &text, &args.needle, out);
        }
    }
    Ok(())
}

fn find_frontmatter_end(text: &str) -> Option<usize> {
    if !text.starts_with("---\n") {
        return None;
    }
    text[4..]
        .find("\n---")
        .map(|idx| idx + 4 + 4)
}

fn scan_text(rel: &str, region: &'static str, text: &str, needle: &str, out: &mut Vec<Match>) {
    for line in text.lines() {
        if let Some(byte_offset) = line.find(needle) {
            out.push(Match {
                path: rel.to_string(),
                region,
                offset: byte_offset as u64,
                line: line.to_string(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn finds_in_frontmatter_only() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("story.md"),
            "---\nid: lin-mo\ntype: character\nslug: lin-mo\ncreated_at: t\nupdated_at: t\n---\nbody text\n",
        )
        .unwrap();

        let tool = FindTool::new(dir.path());
        let v = tool
            .execute(serde_json::json!({
                "path": ".",
                "needle": "lin-mo",
            }))
            .await
            .unwrap();
        let ms = v["matches"].as_array().unwrap();
        assert!(!ms.is_empty(), "should find in frontmatter");
        assert!(ms.iter().all(|m| m["region"] == "frontmatter"));
    }

    #[tokio::test]
    async fn find_bodies_when_asked() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("story.md"),
            "---\nid: story\ntype: story\nslug: s\ncreated_at: t\nupdated_at: t\n---\n林默走进实验室\n",
        )
        .unwrap();

        let tool = FindTool::new(dir.path());
        let v = tool
            .execute(serde_json::json!({
                "path": ".",
                "needle": "林默",
                "include_bodies": true,
            }))
            .await
            .unwrap();
        let ms = v["matches"].as_array().unwrap();
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0]["region"], "body");
    }
}
