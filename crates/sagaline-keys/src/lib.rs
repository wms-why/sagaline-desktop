//! BYOK (Bring Your Own Key) encrypted storage + job store.
//!
//! Both live under one redb file at `~/.sageline/data/keys.db`,
//! encrypted with [`age`]. The DB never touches the network. Story
//! directories stay zero-key — agents read keys only from here.
//!
//! Layered model (per monorepo `AGENTS.md`):
//!
//! - Story content stays in the Markdown files (filesystem = database).
//! - Generation jobs / assets / embeddings may live alongside keys
//!   in `~/.sageline/data/keys.db` (this crate).
//! - API keys are encrypted with the per-machine X25519 identity at
//!   `~/.sageline/identity.age`.
//!
//! ## Layout
//!
//! ```text
//! ~/.sageline/data/keys.db     # redb, holds `provider_key` + `jobs` tables
//! ~/.sageline/identity.age     # X25519 identity (machine-bound, auto-created)
//! ```
//!
//! One identity per machine. Re-encrypting on a new machine requires
//! the old machine's secret key (intentional; keys never leave the
//! device).
//!
//! ## Entry point
//!
//! Use [`SagalineStore::open`] to construct the unified store, then
//! [`SagalineStore::keys`] / [`SagalineStore::jobs`] to get typed
//! views. The views borrow from the store; hold the store alive for
//! the duration of the views' use.

pub mod error;
pub mod jobs;
pub mod store;

pub use error::KeyError;
pub use jobs::{Job, JobStatus, JobStore};
pub use store::{KeyHandle, KeyStore, ProviderKeyId, SagalineStore};
