//! The agent's tool registry.
//!
//! Every action the agent can take — reading story state, mutating
//! the world, generating an image, etc. — is a [`Tool`]. Tools
//! describe themselves with a JSON Schema so an LLM client can
//! advertise them via function-calling without further translation.
//!
//! ## Capability tiers
//!
//! Every tool declares a [`Capability`]:
//!
//! - [`Capability::Read`] — read-only world queries (e.g.
//!   `get_story`, `list_characters`).
//! - [`Capability::Mutate`] — domain mutations that go through
//!   the Proposal → Commit workflow (Phase 3). Tools like
//!   `create_scene` sit here.
//! - [`Capability::Execute`] — side-effects outside the world
//!   (image / video / audio generation, file I/O). `generate_image`
//!   sits here.
//!
//! [`ToolRegistry::by_capability`] groups registered tools by tier;
//! `AgentConfig` (Phase 4) will pick which tiers an agent is
//! allowed to run with.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use schemars::{schema::RootSchema, schema_for};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

/// What a tool reports back to the agent. Tool outputs are JSON values;
/// the agent's prompt assembly (`crate::prompt`) is the only place that
/// knows how to render them.
pub type ToolResult = Value;

/// What the tool is allowed to do. See module-level docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Read-only world queries. May not mutate state.
    Read,
    /// Mutate world state. (Phase 3 wires these through the
    /// Proposal → Commit workflow; today they execute inline.)
    Mutate,
    /// External side-effects (image / video generation, network).
    /// These always run regardless of proposal state — they're
    /// not world-state mutations.
    Execute,
}

impl Capability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Capability::Read => "read",
            Capability::Mutate => "mutate",
            Capability::Execute => "execute",
        }
    }
}

/// Per-invocation context handed to every tool. Phase 2 keeps
/// this small; Phase 3 will add the `proposal_id` plumbing that
/// lets `Mutate` tools record themselves into the `agent_actions`
/// audit log alongside the proposal they're part of.
#[derive(Clone)]
pub struct ToolContext {
    /// Shared handle to the SQLite world DB.
    pub world: Arc<sagaline_store::World>,
    /// Identifier for the agent making the call (for the audit
    /// log).
    pub agent_id: String,
    /// Optional proposal this call belongs to. `Mutate` tools
    /// without a `proposal_id` are still allowed today but
    /// Phase 3 will make it required.
    pub proposal_id: Option<String>,
}

impl ToolContext {
    /// Convenience constructor for tests / non-agent callers.
    pub fn new(world: Arc<sagaline_store::World>) -> Self {
        Self {
            world,
            agent_id: "agent".into(),
            proposal_id: None,
        }
    }
}

/// Errors a tool can raise. Kept narrow — anything else is a `Bug`.
#[derive(Debug, Error)]
pub enum ToolError {
    #[error("tool `{name}` is not registered")]
    UnknownTool { name: String },

    #[error("tool `{name}` received invalid arguments: {message}")]
    BadArgs { name: String, message: String },

    #[error("tool `{name}` failed: {source}")]
    Execution {
        name: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("I/O error in tool `{name}` at {path}: {source}")]
    Io {
        name: String,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Static descriptor a tool exposes to the agent (and, later, to an LLM).
///
/// Mirrors the shape of an OpenAI function-calling tool definition:
/// name, human-readable description, a JSON Schema for arguments, and
/// a [`Capability`] tier.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub parameters: RootSchema,
    pub capability: Capability,
}

impl ToolDescriptor {
    /// Build a descriptor from a name, description, capability, and a
    /// sample argument type whose JSON Schema the tool wants to
    /// advertise.
    pub fn for_args<T>(
        name: impl Into<String>,
        description: impl Into<String>,
        capability: Capability,
    ) -> Self
    where
        T: schemars::JsonSchema,
    {
        Self {
            name: name.into(),
            description: description.into(),
            parameters: schema_for!(T),
            capability,
        }
    }
}

/// A single agent tool. Implementations are stateless and cheap to
/// clone; the registry holds `Arc<dyn Tool>`. `execute` is async so the
/// trait can wrap rig `Tool` invocations and HTTP I/O without spawning
/// a per-call runtime.
///
/// ## Phase 5: `execute_in_tx`
///
/// `execute_in_tx` is the transactional sibling of `execute`. Phase 5's
/// `approve_proposal` opens one outer SQLite transaction and replays
/// each action inside it; tools that opt into `execute_in_tx` see
/// their SQL run inside that transaction (so a mid-replay failure
/// rolls the whole batch back). Tools that don't opt in keep using
/// `execute`, which means a partial-failure leaves the world DB in
/// whatever state the successful replays reached (the Phase 3 caveat).
#[async_trait]
pub trait Tool: Send + Sync {
    /// Static metadata. Must be cheap — called once at registration.
    fn descriptor(&self) -> ToolDescriptor;

    /// Execute with JSON arguments. Argument shape is the one
    /// advertised in [`ToolDescriptor::parameters`]. The
    /// [`ToolContext`] carries the world handle + audit-log
    /// identifiers.
    async fn execute(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError>;

    /// Whether this tool implements [`Self::execute_in_tx`]. Tools
    /// that always mutate the world DB should return `true` once
    /// they implement the in-tx variant; tools that are read-only
    /// or that need a fresh connection (HTTP I/O) return `false`.
    /// Default: `false`.
    fn supports_in_tx(&self) -> bool {
        false
    }

    /// Same as [`Self::execute`], but the tool's writes happen
    /// inside the caller-supplied transaction. Default returns an
    /// error so callers can fall back to [`Self::execute`].
    ///
    /// Synchronous on purpose: `rusqlite::Transaction` is `!Send`
    /// (SQLite transactions are per-thread), so wrapping this in
    /// an async fn would force `Send` on the future. The work
    /// here is short — a single `tx.execute` — so blocking is
    /// fine. The caller is itself async; `await`ing this method
    /// is unnecessary.
    ///
    /// **Contract:** the implementation must NOT call `tx.commit()`
    /// — the caller owns the transaction boundary.
    fn execute_in_tx(
        &self,
        _ctx: &ToolContext,
        _tx: &rusqlite::Transaction<'_>,
        _args: Value,
    ) -> Result<ToolResult, ToolError> {
        Err(ToolError::BadArgs {
            name: self.descriptor().name,
            message: "this tool does not support execute_in_tx".into(),
        })
    }
}

/// A registry of tools, keyed by name. The agent looks up tools here
/// before invoking them. Cheap to clone (tools live in `Arc`; cloning
/// bumps refcounts, not the tools).
#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool. Overwrites any existing tool with the same
    /// name; the agent does not support duplicate names by design.
    pub fn register<T: Tool + 'static>(&mut self, tool: T) -> &mut Self {
        let descriptor = tool.descriptor();
        self.tools.insert(descriptor.name, Arc::new(tool));
        self
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// Wrap this registry in an `Arc` so callers (e.g.
    /// `ApproveProposalTool`) can keep a long-lived reference
    /// without holding the agent's `&mut` borrow.
    pub fn into_arc(self) -> Arc<Self> {
        Arc::new(self)
    }

    /// All registered tool descriptors, in name order.
    pub fn descriptors(&self) -> Vec<ToolDescriptor> {
        self.tools.values().map(|t| t.descriptor()).collect()
    }

    /// Subset of descriptors matching the given capability tier.
    /// Used by `AgentConfig` (Phase 4) to disable tiers per agent.
    pub fn descriptors_with_capability(&self, cap: Capability) -> Vec<ToolDescriptor> {
        self.tools
            .values()
            .map(|t| t.descriptor())
            .filter(|d| d.capability == cap)
            .collect()
    }

    /// Group registered tool names by capability. Used by the
    /// UI's "tools" tab in the activity panel.
    pub fn by_capability(&self) -> BTreeMap<Capability, Vec<String>> {
        let mut out: BTreeMap<Capability, Vec<String>> = BTreeMap::new();
        for d in self.descriptors() {
            out.entry(d.capability).or_default().push(d.name);
        }
        out
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::JsonSchema;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, JsonSchema)]
    struct DummyArgs {
        x: i64,
    }

    struct DummyTool;
    #[async_trait]
    impl Tool for DummyTool {
        fn descriptor(&self) -> ToolDescriptor {
            ToolDescriptor::for_args::<DummyArgs>("dummy", "test tool", Capability::Read)
        }
        async fn execute(&self, _ctx: ToolContext, _args: Value) -> Result<ToolResult, ToolError> {
            Ok(Value::Null)
        }
    }

    #[test]
    fn capability_str_round_trip() {
        assert_eq!(Capability::Read.as_str(), "read");
        assert_eq!(Capability::Mutate.as_str(), "mutate");
        assert_eq!(Capability::Execute.as_str(), "execute");
    }

    #[test]
    fn by_capability_groups_names() {
        let mut reg = ToolRegistry::new();
        reg.register(DummyTool);
        let groups = reg.by_capability();
        assert_eq!(
            groups.get(&Capability::Read),
            Some(&vec!["dummy".to_string()])
        );
        assert!(groups.get(&Capability::Mutate).is_none());
    }

    #[test]
    fn descriptors_with_capability_filters() {
        let mut reg = ToolRegistry::new();
        reg.register(DummyTool);
        assert_eq!(reg.descriptors_with_capability(Capability::Read).len(), 1);
        assert_eq!(reg.descriptors_with_capability(Capability::Mutate).len(), 0);
    }
}
