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

use sagaline_agent::{Agent, CommitPolicy};
use sagaline_core::{FileStoryStore, ProjectLocation, StoryStore};
use sagaline_store::World;
use sagaline_providers::{ProviderConfigError, ProviderConfigSet, ProviderRegistry};

use crate::prefs::{user_home, Prefs, PrefsError};

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
    /// Live [`CommitPolicy`]. The UI's activity-panel picker
    /// calls [`Self::set_commit_policy`] to flip this; the
    /// value is then pushed into the running [`Agent`] (if any)
    /// so its next [`sagaline_agent::Agent::dispatch_tool`]
    /// call sees the new policy without rebuilding the loop.
    /// Seeded from `SAGALINE_COMMIT_POLICY` at [`Self::open_at`].
    commit_policy: Arc<RwLock<CommitPolicy>>,
    /// Most recently built agent. The UI's
    /// [`crate::service::AppEnvProposalService`] reaches it via
    /// this handle to flip the live policy and to dispatch the
    /// `approve_proposal` / `reject_proposal` tools. `None` until
    /// [`Self::build_agent`] runs for the first time.
    agent: Arc<RwLock<Option<Arc<Agent>>>>,
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
        Self::open_with(data_dir, user_home())
    }

    /// Like [`Self::open_at`] but with an explicit home directory
    /// for the default-project-location fallback. Tests use this
    /// to:
    /// - control which path gets adopted (`Some(fake_home)` →
    ///   `<fake_home>/Documents/Sagaline Projects` lands on disk),
    /// - skip the adoption entirely (`None` → `project_location`
    ///   stays unset, so the existing "no project location →
    ///   create fails" failure path keeps working).
    ///
    /// Production code should keep using [`Self::open`] or
    /// [`Self::open_at`], which thread
    /// [`prefs::user_home()`] (read from `$HOME` / `%USERPROFILE%`)
    /// through. Passing `None` here also covers the rare
    /// "no home env var" production case (CI sandboxes) — the
    /// user still gets a usable app, they just have to pick a
    /// project location via ⌘ , like before.
    pub fn open_at_with_home(
        data_dir: PathBuf,
        home: Option<&Path>,
    ) -> Result<Self, EnvError> {
        Self::open_with(data_dir, home.map(Path::to_path_buf))
    }

    fn open_with(data_dir: PathBuf, home: Option<PathBuf>) -> Result<Self, EnvError> {
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
        let prefs = load_or_adopt_default_prefs(&data_dir, home.as_deref())?;
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
        let commit_policy = parse_commit_policy_env();
        Ok(Self {
            data_dir,
            store: Arc::new(store),
            config,
            registry,
            runtime,
            prefs: Arc::new(RwLock::new(prefs)),
            story_store: Arc::new(RwLock::new(story_store)),
            commit_policy: Arc::new(RwLock::new(commit_policy)),
            agent: Arc::new(RwLock::new(None)),
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
    /// Returns `Arc<Agent>` so the
    /// [`crate::service::AppEnvProposalService`] can reach the
    /// same instance via [`Self::agent`]. The returned agent is
    /// also stored in `self.agent` so callers that drop the
    /// return value still see the same instance.
    ///
    /// [`AgentConfig::commit_policy`] is seeded from the live
    /// [`Self::commit_policy`] slot, which was populated from
    /// `SAGALINE_COMMIT_POLICY` at startup (or by the UI's
    /// activity-panel picker since then). The UI toggle calls
    /// [`Self::set_commit_policy`] which pushes the new value
    /// into the stored agent so the next
    /// [`sagaline_agent::Agent::dispatch_tool`] call inside the
    /// same loop honours it without rebuilding the agent.
    pub fn build_agent(&self, _story_root: &Path) -> Arc<Agent> {
        use sagaline_agent::tools::{
            AddCharacterAgeTool, AddCharacterAppearanceTool, ApproveProposalTool,
            AssignCharacterToSceneTool, AssignEnvironmentToSceneTool, CreateChapterTool,
            CreateCharacterTool, CreateEnvironmentTool, CreatePropTool, CreateSceneTool,
            CreateShotTool, GenerateImageTool, GetStoryTool, ListPendingProposalsTool,
            ListStoriesTool, ProposeChangeTool, RejectProposalTool, SearchStoryTool,
            UpdateCharacterTool, ValidateWorldTool,
        };
        use sagaline_agent::AgentConfig;

        let commit_policy = *self.commit_policy.read().expect("commit_policy lock poisoned");
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
        let agent = Arc::new(agent);
        *self.agent.write().expect("agent lock poisoned") = Some(agent.clone());
        agent
    }

    /// The most recently built [`Agent`], or `None` until
    /// [`Self::build_agent`] runs for the first time. The
    /// proposal service holds the same instance to dispatch
    /// `approve_proposal` / `reject_proposal` and to flip the
    /// live [`CommitPolicy`].
    pub fn agent(&self) -> Option<Arc<Agent>> {
        self.agent.read().expect("agent lock poisoned").clone()
    }

    /// Current live [`CommitPolicy`]. Cheap; reads the
    /// [`Self::commit_policy`] slot the UI toggle also mutates.
    pub fn commit_policy(&self) -> CommitPolicy {
        *self.commit_policy.read().expect("commit_policy lock poisoned")
    }

    /// Flip the live [`CommitPolicy`]. Pushes the new value into
    /// the running [`Agent`] (if any) so its next
    /// [`sagaline_agent::Agent::dispatch_tool`] call sees it
    /// without rebuilding the loop. Also mirrors the value into
    /// `SAGALINE_COMMIT_POLICY` so a process restart inherits
    /// the same policy.
    pub fn set_commit_policy(&self, policy: CommitPolicy) {
        *self.commit_policy.write().expect("commit_policy lock poisoned") = policy;
        let raw = match policy {
            CommitPolicy::Auto => "auto",
            CommitPolicy::Manual => "manual",
        };
        // `set_var` is fine on the main process; tests should
        // not depend on this behaviour.
        std::env::set_var("SAGALINE_COMMIT_POLICY", raw);
        if let Some(agent) = self.agent.read().expect("agent lock poisoned").as_ref() {
            agent.set_commit_policy(policy);
        }
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

/// Resolve [`CommitPolicy`] from `SAGALINE_COMMIT_POLICY` at
/// startup. `auto` | `manual` | unset → defaults; unknown values
/// fall back to [`CommitPolicy::Auto`] with a `tracing::warn!`.
/// The same logic lives in [`AppEnv::set_commit_policy`] for the
/// reverse direction (live toggle → env var mirror).
fn parse_commit_policy_env() -> CommitPolicy {
    match std::env::var_os("SAGALINE_COMMIT_POLICY") {
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
    }
}

/// Load the user's [`Prefs`] and, if no project location has been
/// picked yet, adopt the default (`<home>/Documents/Sagaline
/// Projects` — created if absent) and persist it. This removes
/// the "must open ⌘ , before ⌘ N works" friction for first-run
/// users; the user can still override the default later via the
/// project-settings modal (`AppEnv::set_project_location`),
/// which writes through to `prefs.toml`.
///
/// `home_override` is the home directory to use when adopting the
/// default:
/// - `Some(home)` → adopt `<home>/Documents/Sagaline Projects`.
/// - `None` → leave `project_location` unset (user must pick one
///   via ⌘ ,). Tests use this to keep the
///   "fresh env has no project location" failure path under test
///   without racing on process env vars.
///
/// [`AppEnv::open`] and [`AppEnv::open_at`] thread
/// [`prefs::user_home()`] (which reads `$HOME` / `%USERPROFILE%`)
/// through here in production.
fn load_or_adopt_default_prefs(
    data_dir: &Path,
    home_override: Option<&Path>,
) -> Result<Prefs, PrefsError> {
    let mut prefs = Prefs::load(data_dir)?;
    if prefs.project_location.is_some() {
        return Ok(prefs);
    }
    let Some(home) = home_override else {
        warn!("no home directory; user must pick a project location via ⌘ ,");
        return Ok(prefs);
    };
    match crate::prefs::default_project_location(home) {
        Ok(default_loc) => {
            prefs.project_location = Some(default_loc);
            if let Err(e) = prefs.save(data_dir) {
                warn!(error = %e, "could not persist default project location to prefs.toml");
            }
        }
        Err(e) => {
            warn!(error = %e, "failed to create default project location");
        }
    }
    Ok(prefs)
}
