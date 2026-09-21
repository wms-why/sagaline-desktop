//! Sagaline world state — SQLite-backed Source of Truth.
//!
//! Lives at `~/.sageline/data/world.db` once [`World::open`] has
//! been called. The schema is defined by the SQL files under
//! `migrations/` and applied by [`refinery`] at first open.
//!
//! ```no_run
//! use sagaline_store::World;
//! let world = World::open(std::path::Path::new("/Users/me/.sageline/data")).unwrap();
//! let story = world.stories().create(
//!     sagaline_store::repo::NewStory { slug: "lin-mo", title: "Lin Mo", summary: "" },
//! ).unwrap();
//! # let _ = story;
//! ```
//!
//! The agent tools crate (`sagaline-agent`) is the only intended
//! writer; the UI (`sagaline-ui`) is read-only. BYOK keys and
//! generation jobs live in this same world DB (the `provider_key`
//! and `jobs` tables) — see [`World::keys`] / [`World::jobs`].

pub mod error;
pub mod identity;
pub mod key_id;
pub mod pool;
pub mod repo;
pub mod time_util;
pub mod validation;
pub mod world;

pub use error::StoreError;
pub use key_id::{KeyHandle, ProviderKeyId};
pub use pool::{Pool, PooledConn};
pub use repo::{
    AgentActionRepo, AgentActionRow, CharacterRepo, EnvironmentRepo, EnvironmentRow, Job,
    JobRepo, JobStatus, KeyRepo, NewAgentAction, NewCharacter, NewChapter, NewEnvironment,
    NewProp, NewProposal, NewProposalAction, NewScene, NewStory, PropRepo, PropRow,
    ProposalActionRepo, ProposalActionRow, ProposalRepo, ProposalRow, ProposalStatus,
    SceneRepo, StoryRepo,
};
pub use validation::{validate_world_in_tx, ValidationIssue};
pub use world::World;
