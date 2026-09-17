//! The desktop app's long-lived shared state.
//!
//! [`AppEnv`] is constructed once at startup and stored as a gpui
//! global. It owns:
//!
//! - the unified [`SagalineStore`] (encrypted keys + job records),
//! - the public per-provider [`ProviderConfigSet`] (parsed from
//!   `~/.sageline/data/config.toml`; missing file → empty set),
//! - the [`ProviderRegistry`] (currently empty; a future phase will
//!   pre-register `MinimaxImage` and OpenAI's gpt-image-1).
//!
//! Tools take `Arc`s of these three — see
//! [`sagaline_agent::tools::GenerateImageTool`] — so the agent
//! built by [`AppEnv::build_agent`] just clones the Arcs and
//! registers the tools. No I/O on the agent path beyond tool
//! execution.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use thiserror::Error;
use tracing::warn;

use sagaline_agent::Agent;
use sagaline_keys::SagalineStore;
use sagaline_providers::{ProviderConfigError, ProviderConfigSet, ProviderRegistry};

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
    /// Unified encrypted store. `keys()` / `jobs()` borrow from this.
    pub store: Arc<SagalineStore>,
    /// Public per-provider config (no secrets).
    pub config: Arc<ProviderConfigSet>,
    /// Provider registry. Pre-populated in later phases (image
    /// backends); empty for now.
    pub registry: Arc<ProviderRegistry>,
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
        let store = SagalineStore::open(&data_dir)?;
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
        Ok(Self {
            data_dir,
            store: Arc::new(store),
            config,
            registry,
        })
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
    pub fn build_agent(&self, story_root: &Path) -> Agent {
        use sagaline_agent::tools::{
            FindTool, GenerateImageTool, ListDirTool, ReadFileTool, ValidateStoryTool,
            WriteFileTool,
        };

        let root = story_root.to_path_buf();
        let mut agent = Agent::new();
        agent.tools_mut().register(ValidateStoryTool);
        agent.tools_mut().register(ReadFileTool::new(&root));
        agent.tools_mut().register(WriteFileTool::new(root.clone()));
        agent.tools_mut().register(ListDirTool::new(root.clone()));
        agent.tools_mut().register(FindTool::new(root));
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

    /// Build an LLM client if the user has a chat provider
    /// configured with a registered key. Returns `None` silently
    /// on any failure (missing config, missing key, build error)
    /// — the caller falls back to canned mode.
    fn try_build_llm(&self) -> Option<std::sync::Arc<dyn sagaline_agent::LlmClient>> {
        use sagaline_agent::{LlmClient, RigLlm};
        use sagaline_providers::{build_chat_with_default, Capability};
        use sagaline_keys::ProviderKeyId;

        // First provider that has both `[chat.<provider>]` in
        // config.toml AND a registered `default` key wins.
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
    Key(#[from] sagaline_keys::KeyError),
    #[error(transparent)]
    Config(#[from] ProviderConfigError),
}
