//! Built-in tools.
//!
//! ## Phase 2 surface
//!
//! Domain tools (`story`, `character`, `chapter`, `scene`,
//! `shot`, `environment`, `prop`, `world_validate`) are the
//! agent's only path to mutating the world DB. They classify
//! into [`Capability::Read`] / [`Capability::Mutate`] /
//! [`Capability::Execute`] tiers — see `crate::tool`.
//!
//! ## Phase 3 surface (Proposal → Commit)
//!
//! `proposal` adds four tools that gate every mutation:
//! `propose_change` queues a batch, `list_pending_proposals`
//! shows the queue, `approve_proposal` validates + replays +
//! flips status to `committed`, `reject_proposal` discards.
//!
//! ## Phase 2.5 surface (loop rewire)
//!
//! The legacy Markdown-era filesystem tools (`read_file`,
//! `write_file`, `list_dir`, `find`, `validate_story`) are
//! deleted; the loop now drives the SQLite [`World`] directly.
//! `generate_image` stays — it's a provider I/O tool, not a
//! filesystem one.

pub mod chapter;
pub mod character;
pub mod domain;
pub mod environment;
pub mod generate_image;
pub mod prop;
pub mod proposal;
pub mod scene;
pub mod shot;
pub mod story;
pub mod world_validate;

pub use chapter::CreateChapterTool;
pub use character::{
    AddCharacterAgeTool, AddCharacterAppearanceTool, CreateCharacterTool, UpdateCharacterTool,
};
pub use environment::CreateEnvironmentTool;
pub use generate_image::{GenerateImageArgs, GenerateImageTool};
pub use prop::CreatePropTool;
pub use proposal::{
    ApproveProposalTool, ListPendingProposalsTool, ProposeChangeTool, RejectProposalTool,
};
pub use sagaline_store::ValidationIssue;
pub use scene::{AssignCharacterToSceneTool, AssignEnvironmentToSceneTool, CreateSceneTool};
pub use shot::CreateShotTool;
pub use story::{GetStoryTool, ListStoriesTool, SearchStoryTool};
pub use world_validate::ValidateWorldTool;
