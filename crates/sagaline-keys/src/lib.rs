//! BYOK (Bring Your Own Key) encrypted storage.
//!
//! API keys live under `~/.sageline/data/keys.db` (redb), encrypted with
//! [`age`]. The DB never touches the network. Story directories stay zero-
//! key — agents read keys only from here.
//!
//! Layered model (per monorepo `AGENTS.md`):
//!
//! - Story content stays in the Markdown files (filesystem = database).
//! - Generation jobs / assets / embeddings may live in
//!   `~/.sageline/data/index.db` (later phase).
//! - API keys live in `~/.sageline/data/keys.db` (this crate).
//!
//! ## Layout
//!
//! ```text
//! ~/.sageline/data/keys.db     # redb, holds age ciphertext blobs
//! ~/.sageline/identity.age     # X25519 identity (machine-bound, auto-created)
//! ```
//!
//! One identity per machine. Re-encrypting on a new machine requires the
//! old machine's secret key (intentional; keys never leave the device).
pub mod jobs;

pub use jobs::{Job, JobStatus, JobStore};

pub mod error;
pub mod store;

pub use error::KeyError;
pub use store::{KeyHandle, KeyStore, ProviderKeyId};