//! Typed repositories. Each repo borrows from [`crate::World`] for
//! the lifetime of a query. Multi-table mutations (`SceneRepo`,
//! `VersionRepo`) take `&Connection` so the caller controls the
//! transaction boundary.

pub mod agent_action;
pub mod character;
pub mod environment;
pub mod job;
pub mod key;
pub mod prop;
pub mod proposal;
pub mod proposal_action;
pub mod scene;
pub mod story;

pub use agent_action::{AgentActionRepo, AgentActionRow, NewAgentAction};
pub use character::{CharacterRepo, CharacterRow, NewCharacter};
pub use environment::{EnvironmentRepo, EnvironmentRow, NewEnvironment};
pub use job::{Job, JobRepo, JobStatus};
pub use key::KeyRepo;
pub use prop::{NewProp, PropRepo, PropRow};
pub use proposal::{NewProposal, ProposalRepo, ProposalRow, ProposalStatus};
pub use proposal_action::{NewProposalAction, ProposalActionRepo, ProposalActionRow};
pub use scene::{ChapterRow, NewChapter, NewScene, SceneRepo, SceneRow};
pub use story::{NewStory, StoryRepo, StoryRow};
