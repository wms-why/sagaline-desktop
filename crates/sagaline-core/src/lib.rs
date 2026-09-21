//! Sagaline story domain — types and Markdown render layer.
//!
//! The Source of Truth lives in `sagaline-store` (a SQLite world DB).
//! This crate contributes:
//!
//! - the **domain types** ([`Story`], [`Bible`], [`Character`],
//!   [`Environment`], [`Prop`], [`Chapter`], [`Scene`], [`Shot`],
//!   [`Reference`]) shared between the store, the agent tools,
//!   and the UI;
//! - the **Markdown render layer** ([`frontmatter`], [`render`]) used
//!   by the agent's context compiler and by the preview pane in
//!   `sagaline-ui`.
//!
//! No filesystem I/O lives here anymore. Reads flow through
//! `sagaline-store`; this crate only knows how to format what it
//! reads. Validation runs against the in-memory domain types
//! ([`entity`]) and is invoked by the agent's reflection step.

pub mod entity;
pub mod error;
pub mod frontmatter;
pub mod graph;
pub mod markdown;
pub mod path;
pub mod render;
pub mod story;
pub mod shot;
pub mod story_root;

pub use entity::{EntityId, EntityType, ParsedEntity, Reference, ReferenceKind};
pub use error::{CoreError, ValidationError};
pub use graph::StoryGraph;
pub use render::render_preview_lines;
pub use story::{
    FileStoryStore, ProjectLocation, Story, StoryHandle, StoryStore, StorySummary,
};
pub use story_root::StoryRoot;