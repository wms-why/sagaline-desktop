//! The desktop app's long-lived shared state.
//!
//! [`AppEnv`] is constructed once at startup and stored as a gpui
//! global. It owns:
//!
//! - the SQLite world DB [`sagaline_store::World`] (encrypted
//!   keys + job records + story / character / scene state — see
//!   Phase 1 of the SQLite-world-state pivot in `AGENTS.md`),
//! - the public per-provider [`ProviderConfigSet`] (parsed from
//!   `~/.sageline/data/config.toml`; missing file → empty set),
//! - the [`ProviderRegistry`] (currently empty; a future phase will
//!   pre-register `MinimaxImage` and OpenAI's gpt-image-1),
//! - the persisted [`Prefs`] (project location, future non-secret
//!   per-machine preferences),
//! - the [`StoryStore`] the UI calls to list / open / create
//!   stories without ever naming a path or a slug.
//!
//! Tools take `Arc`s of these — see
//! [`sagaline_agent::tools::GenerateImageTool`] — so the agent
//! built by [`AppEnv::build_agent`] just clones the Arcs and
//! registers the tools. No I/O on the agent path beyond tool
//! execution.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use thiserror::Error;
use tracing::warn;

use sagaline_agent::Agent;
use sagaline_core::{FileStoryStore, ProjectLocation, StoryStore};
use sagaline_store::World;
use sagaline_providers::{ProviderConfigError, ProviderConfigSet, ProviderRegistry};

use crate::prefs::{Prefs, PrefsError};

/// `~/.sageline/data/` — the per-user store directory.
///
/// We honour `SAGALINE_DATA_DIR` for tests / portable installs; when
/// the env var is unset, fall back to `$HOME/.sageline/data` on
/// Unix and `%USERPROFILE%/.sageline/data` on Windows.
pub fn default_data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("SAGALINE_DATA_DIR") {
        return PathBuf::from(p);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".sageline").join("data");
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        return PathBuf::from(profile).join(".sageline").join("data");
    }
    PathBuf::from(".sageline/data")
}

/// The shared app environment.
pub struct AppEnv {
    /// Resolved data directory. May be a non-default value if
    /// `SAGALINE_DATA_DIR` is set; useful for diagnostics.
    pub data_dir: PathBuf,
    /// SQLite world DB. `keys()` / `jobs()` / `stories()` /
    /// `characters()` / `scenes()` all borrow from this.
    pub store: Arc<World>,
    /// Public per-provider config (no secrets).
    pub config: Arc<ProviderConfigSet>,
    /// Provider registry. Pre-populated in later phases (image
    /// backends); empty for now.
    pub registry: Arc<ProviderRegistry>,
    /// Tokio runtime for the agent / provider I/O path. GPUI
    /// tasks run on their own scheduler, so anything that needs
    /// `tokio::fs` / `tokio::task::spawn_blocking` / etc. must
    /// be scheduled through this runtime via `sagaline_bridge`.
    /// Kept alive for the lifetime of the app; `Arc` so a
    /// cheap clone can hand the `Handle` to the bridge.
    pub runtime: Arc<tokio::runtime::Runtime>,
    /// User preferences (project location, future non-secret
    /// per-machine settings). Wrapped in `RwLock` because the
    /// settings modal mutates it.
    prefs: Arc<RwLock<Prefs>>,
    /// Storage-agnostic story store. `None` until the user picks a
    /// project location in Settings; the UI surfaces this gap.
    story_store: Arc<RwLock<Option<Arc<dyn StoryStore>>>>,
}

impl AppEnv {
    /// Open the data directory, build the unified store, parse
    /// `config.toml`, and assemble the registry. Missing config
    /// file is non-fatal: it just produces an empty config.
    pub fn open() -> Result<Self, EnvError> {
        Self::open_at(default_data_dir())
    }

    /// Like [`Self::open`] but at an explicit data directory.
    pub fn open_at(data_dir: PathBuf) -> Result<Self, EnvError> {
        let store = World::open(&data_dir)?;
        let config = match ProviderConfigSet::load(&data_dir.join("config.toml")) {
            Ok(c) => Arc::new(c),
            Err(ProviderConfigError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                warn!(
                    "no config.toml at {}; using empty config",
                    data_dir.join("config.toml").display()
                );
                Arc::new(ProviderConfigSet::default())
            }
            Err(e) => return Err(e.into()),
        };
        let registry = Arc::new(ProviderRegistry::new());
        let prefs = Prefs::load(&data_dir)?;
        let story_store = build_story_store(prefs.project_location.clone());
        // The agent's tool calls go through this runtime; multi-thread
        // so parallel LLM / image / file I/O don't serialize on a
        // single worker. `enable_all` flips on the IO + timer
        // drivers that `tokio::fs` and `sleep` need.
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("sagaline-tokio")
                .build()
                .map_err(|e| EnvError::Runtime(e.to_string()))?,
        );
        Ok(Self {
            data_dir,
            store: Arc::new(store),
            config,
            registry,
            runtime,
            prefs: Arc::new(RwLock::new(prefs)),
            story_store: Arc::new(RwLock::new(story_store)),
        })
    }

    /// Tokio [`Handle`] into the agent's runtime. Cheap to clone.
    pub fn tokio_handle(&self) -> tokio::runtime::Handle {
        self.runtime.handle().clone()
    }

    /// Snapshot of the current preferences. Cheap; clones the
    /// `ProjectLocation` arc.
    pub fn prefs(&self) -> Prefs {
        self.prefs.read().expect("prefs lock poisoned").clone()
    }

    /// Update the project location. Persists to `prefs.toml` and
    /// rebuilds the [`StoryStore`] so subsequent `list` / `create`
    /// calls hit the new path. Replaces an unset location
    /// (`None` → `Some(loc)`) the same way as a change.
    pub fn set_project_location(&self, loc: ProjectLocation) -> Result<(), EnvError> {
        {
            let mut prefs = self.prefs.write().expect("prefs lock poisoned");
            prefs.project_location = Some(loc.clone());
            prefs.save(&self.data_dir)?;
        }
        let store = build_story_store(Some(loc));
        *self.story_store.write().expect("story_store lock poisoned") = store;
        Ok(())
    }

    /// The current [`StoryStore`], if a project location has been
    /// chosen. The UI shows a "set a project location first" hint
    /// when this is `None`.
    pub fn story_store(&self) -> Option<Arc<dyn StoryStore>> {
        self.story_store
            .read()
            .expect("story_store lock poisoned")
            .clone()
    }

    /// Build an [`Agent`] pre-registered with the file-system tools
    /// (`read_file`, `write_file`, `list_dir`, `find`, `validate_story`)
    /// plus `generate_image` (which routes through `self.registry`).
    /// `story_root` is the path the user opened; the file tools
    /// confine themselves to that directory.
    ///
    /// If a chat provider is configured AND a key is registered for
    /// it, the agent is constructed with a [`sagaline_agent::RigLlm`]
    /// so PLAN / REFLECT go through the LLM. Without that pair, the
    /// agent runs in canned mode (the deterministic scaffold used by
    /// tests and headless runs).
    /// Build an [`Agent`] wired with the domain tool surface
    /// (Phase 2.5) plus `GenerateImageTool` (Phase 3 + Execute
    /// tier). The story id is supplied at `run_stream` time, so
    /// this method no longer needs a Markdown story path.
    ///
    /// `commit_policy` is read from `SAGALINE_COMMIT_POLICY`:
    /// `auto` (default) | `manual` (route mutations through the
    /// Proposal → Commit gate). Unknown values fall back to Auto
    /// with a `tracing::warn!`.
    pub fn build_agent(&self, _story_root: &Path) -> Agent {
        use sagaline_agent::tools::{
            AddCharacterAgeTool, AddCharacterAppearanceTool, ApproveProposalTool,
            AssignCharacterToSceneTool, AssignEnvironmentToSceneTool, CreateChapterTool,
            CreateCharacterTool, CreateEnvironmentTool, CreatePropTool, CreateSceneTool,
            CreateShotTool, GenerateImageTool, GetStoryTool, ListPendingProposalsTool,
            ListStoriesTool, ProposeChangeTool, RejectProposalTool, SearchStoryTool,
            UpdateCharacterTool, ValidateWorldTool,
        };
        use sagaline_agent::{AgentConfig, CommitPolicy};

        let commit_policy = match std::env::var_os("SAGALINE_COMMIT_POLICY") {
            Some(v) if v == "manual" => CommitPolicy::Manual,
            Some(v) if v == "auto" => CommitPolicy::Auto,
            Some(other) => {
                tracing::warn!(
                    policy = %other.to_string_lossy(),
                    "unknown SAGALINE_COMMIT_POLICY; defaulting to Auto"
                );
                CommitPolicy::Auto
            }
            None => CommitPolicy::Auto,
        };
        let mut agent = Agent::with_config(AgentConfig {
            commit_policy,
            ..AgentConfig::default()
        });

        let world = self.store.clone();

        // Read tools.
        agent.tools_mut().register(GetStoryTool::new(world.clone()));
        agent.tools_mut().register(ListStoriesTool::new(world.clone()));
        agent.tools_mut().register(SearchStoryTool::new(world.clone()));
        agent.tools_mut().register(ValidateWorldTool::new(world.clone()));
        agent.tools_mut().register(ListPendingProposalsTool::new(world.clone()));

        // Mutate tools.
        agent
            .tools_mut()
            .register(CreateCharacterTool::new(world.clone()));
        agent
            .tools_mut()
            .register(UpdateCharacterTool::new(world.clone()));
        agent
            .tools_mut()
            .register(AddCharacterAgeTool::new(world.clone()));
        agent
            .tools_mut()
            .register(AddCharacterAppearanceTool::new(world.clone()));
        agent
            .tools_mut()
            .register(CreateChapterTool::new(world.clone()));
        agent.tools_mut().register(CreateSceneTool::new(world.clone()));
        agent
            .tools_mut()
            .register(AssignCharacterToSceneTool::new(world.clone()));
        agent
            .tools_mut()
            .register(AssignEnvironmentToSceneTool::new(world.clone()));
        agent.tools_mut().register(CreateShotTool::new(world.clone()));
        agent
            .tools_mut()
            .register(CreateEnvironmentTool::new(world.clone()));
        agent.tools_mut().register(CreatePropTool::new(world.clone()));
        agent.tools_mut().register(ProposeChangeTool::new(world.clone()));
        // Hand the Approve tool a shared registry so its
        // replay loop can resolve action targets without
        // holding the agent's `&mut` borrow. We snapshot the
        // registry first (each tool is already in an `Arc`,
        // so the snapshot is cheap), then register Approve
        // against the snapshot.
        let shared_registry = agent.tools_mut().clone().into_arc();
        agent
            .tools_mut()
            .register(ApproveProposalTool::new(world.clone()).with_registry(shared_registry));
        agent
            .tools_mut()
            .register(RejectProposalTool::new(world.clone()));

        // Execute tools.
        agent.tools_mut().register(GenerateImageTool::new(
            self.registry.clone(),
            self.config.clone(),
            self.store.clone(),
        ));

        if let Some(llm) = self.try_build_llm() {
            agent = agent.with_llm(llm);
        }
        agent
    }

    /// Snapshot the current story location (the path the
    /// [`StoryStore`] writes new stories to). `None` when the
    /// user hasn't picked one yet.
    pub fn story_location(&self) -> Option<ProjectLocation> {
        self.prefs.read().expect("prefs lock poisoned").project_location.clone()
    }

    /// Build an LLM client if the user has a chat provider
    /// configured with a registered key. Returns `None` silently
    /// on any failure (missing config, missing key, build error)
    /// — the caller falls back to canned mode.
    fn try_build_llm(&self) -> Option<std::sync::Arc<dyn sagaline_agent::LlmClient>> {
        use sagaline_agent::{LlmClient, RigLlm};
        use sagaline_providers::{build_chat_with_default, Capability};
        use sagaline_store::ProviderKeyId;

        for provider in self.config.providers(Capability::Chat) {
            let cfg = match self.config.get(Capability::Chat, &provider) {
                Some(c) => c,
                None => continue,
            };
            let id = ProviderKeyId::new(&provider, "default").ok()?;
            let key = self.store.keys().get(&id).ok()?;
            let model = match build_chat_with_default(
                &cfg.base_url,
                None,
                &cfg.model,
                &key,
            ) {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!(error = %e, provider, "LLM build failed");
                    continue;
                }
            };
            return Some(std::sync::Arc::new(RigLlm::new(model))
                as std::sync::Arc<dyn LlmClient>);
        }
        None
    }
}

fn build_story_store(loc: Option<ProjectLocation>) -> Option<Arc<dyn StoryStore>> {
    loc.map(|l| Arc::new(FileStoryStore::new(l)) as Arc<dyn StoryStore>)
}

impl std::fmt::Debug for AppEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppEnv")
            .field("data_dir", &self.data_dir)
            .field("registry_providers", &self.registry.providers())
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum EnvError {
    #[error(transparent)]
    Store(#[from] sagaline_store::StoreError),
    #[error(transparent)]
    Config(#[from] ProviderConfigError),
    #[error(transparent)]
    Prefs(#[from] PrefsError),
    #[error("failed to start Tokio runtime: {0}")]
    Runtime(String),
}
