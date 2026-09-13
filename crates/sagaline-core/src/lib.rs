//! Sagaline domain model + SQLite persistence.
//!
//! See [`model`] for the entity types and [`db`] for the SQLite layer.

pub use rusqlite;

pub mod cmd;
pub mod db;
pub mod model;
pub mod provider;
pub mod seed;
