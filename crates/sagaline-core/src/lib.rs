//! Sagaline story workspace — Markdown Source of Truth, file-system graph.
//!
//! The whole story is a directory of Markdown + YAML front-matter files. There
//! is no embedded database in this crate: [`StoryGraph::load`] walks the
//! directory once and produces an in-memory graph of [`ParsedEntity`]s and the
//! [`Reference`]s between them. Validation runs in memory against the same
//! graph.
//!
//! See [`graph`] for the entry points and [`entity`] for the type vocabulary.

pub mod entity;
pub mod error;
pub mod frontmatter;
pub mod graph;
pub mod markdown;
pub mod path;
pub mod schema;
pub mod story_root;

pub use entity::{EntityId, EntityType, ParsedEntity, Reference, ReferenceKind};
pub use error::{CoreError, ValidationError};
pub use graph::StoryGraph;
pub use story_root::StoryRoot;