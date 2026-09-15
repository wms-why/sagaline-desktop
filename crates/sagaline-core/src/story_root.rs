//! Story root — the directory that holds `story.md` and all artifacts.

use std::path::{Path, PathBuf};

use serde_yaml::Value;

use crate::entity::{EntityId, EntityType, ParsedEntity};
use crate::error::CoreError;
use crate::frontmatter;
use crate::markdown::{split, SplitFile};

/// The root of a single story. Holds the directory path and the cached
/// `story.md` parse (id + title).
///
/// Construct with [`StoryRoot::new`]; the directory must exist, be a
/// directory, and contain `story.md` with at least `id` / `type` / `slug`
/// front matter.
#[derive(Debug, Clone)]
pub struct StoryRoot {
    root: PathBuf,
    story_id: EntityId,
    title: String,
}

impl StoryRoot {
    /// Validate the path and load the top-level `story.md`.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, CoreError> {
        let root = root.into();
        let meta = std::fs::metadata(&root)
            .map_err(|_| CoreError::StoryPathMissing(root.clone()))?;
        if !meta.is_dir() {
            return Err(CoreError::StoryPathNotDir(root));
        }
        let story_file = root.join("story.md");
        if !story_file.is_file() {
            return Err(CoreError::MissingStoryFile(root));
        }
        let text = std::fs::read_to_string(&story_file).map_err(|source| CoreError::ReadFile {
            path: story_file.clone(),
            source,
        })?;
        let SplitFile { frontmatter, body: _ } = split(&text).map_err(|e| match e {
            CoreError::FrontMatter { source, .. } => CoreError::FrontMatter {
                path: story_file,
                source,
            },
            other => other,
        })?;

        let id = frontmatter::id_of(&frontmatter)
            .ok_or_else(|| CoreError::MissingStoryFile(root.clone()))?;
        let type_ = frontmatter
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| CoreError::MissingStoryFile(root.clone()))?;
        if type_ != EntityType::Story.as_str() {
            return Err(CoreError::MissingStoryFile(root));
        }

        let title = frontmatter::title_of(&frontmatter).unwrap_or_else(|| id.clone());
        Ok(Self {
            root,
            story_id: EntityId::new(id),
            title,
        })
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    pub fn story_id(&self) -> &EntityId {
        &self.story_id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    /// Build the canonical [`ParsedEntity`] for the story itself.
    pub fn story_entity(&self, full_frontmatter: Value, body: String) -> ParsedEntity {
        ParsedEntity {
            id: self.story_id.clone(),
            type_: EntityType::Story,
            slug: "story".into(),
            path: PathBuf::from("story.md"),
            frontmatter: full_frontmatter,
            body,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(p: &Path, body: &str) {
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn new_rejects_missing_dir() {
        let tmp = tempdir().unwrap();
        let missing = tmp.path().join("nope");
        assert!(matches!(
            StoryRoot::new(&missing),
            Err(CoreError::StoryPathMissing(_))
        ));
    }

    #[test]
    fn new_rejects_file_as_root() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("file.txt");
        write(&f, "hi");
        assert!(matches!(
            StoryRoot::new(&f),
            Err(CoreError::StoryPathNotDir(_))
        ));
    }

    #[test]
    fn new_rejects_no_story_md() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join("story");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(matches!(
            StoryRoot::new(&dir),
            Err(CoreError::MissingStoryFile(_))
        ));
    }

    #[test]
    fn new_accepts_minimal_story() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join("story");
        std::fs::create_dir_all(&dir).unwrap();
        write(
            &dir.join("story.md"),
            "---\nid: s1\ntype: story\nslug: story\ntitle: My Story\n---\n",
        );
        let root = StoryRoot::new(&dir).unwrap();
        assert_eq!(root.story_id().as_str(), "s1");
        assert_eq!(root.title(), "My Story");
    }
}