//! Error types for story loading and validation.

use std::path::PathBuf;

use thiserror::Error;

/// Errors that prevent a story from being loaded at all.
///
/// These are I/O / structure problems — directory missing, no `story.md`,
/// unreadable front matter. Once the story loads, [`ValidationError`] is
/// used instead.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("story path does not exist: {0}")]
    StoryPathMissing(PathBuf),

    #[error("story path is not a directory: {0}")]
    StoryPathNotDir(PathBuf),

    #[error("target story directory already exists: {0}")]
    StoryDirExists(PathBuf),

    #[error("story directory is missing story.md: {0}")]
    MissingStoryFile(PathBuf),

    #[error("invalid slug `{slug}`: {reason}")]
    InvalidSlug {
        slug: String,
        reason: &'static str,
    },

    #[error("failed to read {path}: {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse front matter in {path}: {source}")]
    FrontMatter {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },

    #[error("failed to walk story directory {root}: {source}")]
    Walk {
        root: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Semantic problems inside a successfully-loaded story.
///
/// The story directory and its files all parse — these are content-level
/// concerns: missing required frontmatter fields, enum values out of range,
/// slugs that disagree with the path, references that point to nothing, ids
/// that collide.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ValidationError {
    #[error("missing required field `{field}` in {path}")]
    MissingField { path: PathBuf, field: &'static str },

    #[error(
        "invalid enum value `{value}` for field `{field}` in {path} (allowed: {allowed:?})"
    )]
    InvalidEnum {
        path: PathBuf,
        field: &'static str,
        value: String,
        allowed: Vec<&'static str>,
    },

    #[error("slug mismatch in {path}: expected `{expected}`, got `{actual}`")]
    SlugMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },

    #[error(
        "broken {kind} reference `{missing}` in {path} (available: {available:?})"
    )]
    BrokenReference {
        path: PathBuf,
        kind: String,
        missing: String,
        available: Vec<String>,
    },

    #[error("duplicate id `{id}` across: {paths:?}")]
    DuplicateId { id: String, paths: Vec<PathBuf> },


    #[error(
        "shot `{path}` has asset `{asset}` at `{actual}` which doesn't match the slug-derived directory `{expected}`"
    )]
    InvalidAssetPath {
        path: PathBuf,
        asset: String,
        expected: String,
        actual: String,
    },
}