//! The [`World`] is the handle every other crate talks to. It owns
//! the SQLite connection pool, the X25519 identity path (for BYOK
//! encryption), and runs the embedded `refinery` migrations on
//! first open.
//!
//! On first open the legacy `~/.sageline/data/keys.db` (if any) is
//! **deleted** rather than migrated — see the Phase 1 decision in
//! `client/AGENTS.md`. Users re-enter provider keys through the
//! BYOK panel; the rows land in the new SQLite `provider_key` table.

use std::path::{Path, PathBuf};

use refinery::embed_migrations;

use crate::error::StoreError;
use crate::identity;
use crate::pool::{open_pool, in_memory_pool, Pool, PooledConn};
use crate::repo::agent_action::AgentActionRepo;
use crate::repo::character::CharacterRepo;
use crate::repo::environment::EnvironmentRepo;
use crate::repo::prop::PropRepo;
use crate::repo::job::JobRepo;
use crate::repo::key::KeyRepo;
use crate::repo::proposal::ProposalRepo;
use crate::repo::proposal_action::ProposalActionRepo;
use crate::repo::scene::SceneRepo;
use crate::repo::story::StoryRepo;

embed_migrations!("migrations");

/// The single world-state handle. Clone-able (the pool is `Arc`
/// inside `r2d2::Pool`), cheaply.
#[derive(Clone)]
pub struct World {
    pool: Pool,
    db_path: PathBuf,
    /// Path to the X25519 identity file. Always inside the data
    /// dir; the secret identity is re-read from disk on every
    /// key operation so it doesn't sit in memory long-term.
    identity_path: PathBuf,
}

impl World {
    /// Open or create the world DB at `~/.sageline/data/world.db`.
    /// Runs any pending migrations before returning. Mints the
    /// X25519 identity file (mode 0600 on Unix) on first run, and
    /// deletes any legacy `keys.db` from the pre-SQLite era.
    /// Build the world DB at `~/.sageline/data/world.db`. Runs any
    /// pending migrations, mints the X25519 identity, and removes
    /// any legacy `keys.db` (pre-SQLite install).
    pub fn open(data_dir: &Path) -> Result<Self, StoreError> {
        if !data_dir.exists() {
            return Err(StoreError::BadDataDir(data_dir.display().to_string()));
        }
        std::fs::create_dir_all(data_dir)?;
        Self::open_at(&data_dir.join("world.db"))
    }

    /// Open a specific file path. Used by tests + headless tooling.
    /// If a sibling `keys.db` exists next to `db_path`, it is
    /// deleted (legacy redb file from pre-SQLite installs).
    pub fn open_at(db_path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
            let legacy = parent.join("keys.db");
            if legacy.exists() {
                let _ = std::fs::remove_file(&legacy);
            }
        }
        let pool = open_pool(db_path)?;
        let identity_path = db_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("identity.age");
        identity::load_or_create_identity(&identity_path)?;
        let world = Self {
            pool,
            db_path: db_path.to_path_buf(),
            identity_path,
        };
        world.run_migrations()?;
        Ok(world)
    }

    /// Fresh in-memory world for tests. Migrations still run so the
    /// schema is identical to on-disk. The identity file lives in a
    /// leaked `tempfile::TempDir` (cleaned up at process exit) so
    /// subsequent `keys()` operations see a stable identity.
    pub fn in_memory() -> Result<Self, StoreError> {
        let pool = in_memory_pool()?;
        // Box::leak the tempdir so the identity file outlives the
        // `World` handle. `in_memory()` is test-only; process exit
        // reclaims the directory.
        let dir: &'static tempfile::TempDir =
            Box::leak(Box::new(tempfile::tempdir().map_err(StoreError::Io)?));
        let identity_path = dir.path().join("identity.age");
        identity::load_or_create_identity(&identity_path)?;
        let world = Self {
            pool,
            db_path: PathBuf::from(":memory:"),
            identity_path,
        };
        world.run_migrations()?;
        Ok(world)
    }

    fn run_migrations(&self) -> Result<(), StoreError> {
        // Refinery's `Migrate` impl targets `rusqlite::Connection`.
        // `PooledConnection<SqliteConnectionManager>` derefs to
        // `Connection`, so we can borrow through it. Doing it via
        // the pool matters for in-memory tests: a raw
        // `Connection::open(":memory:")` would land on a separate
        // private in-memory DB and the pool's connections would
        // never see the schema.
        let mut pooled = self.conn()?;
        let conn: &mut rusqlite::Connection = &mut pooled;
        migrations::runner()
            .run(conn)
            .map_err(|e| StoreError::Migrate(e.to_string()))?;
        Ok(())
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn conn(&self) -> Result<PooledConn, StoreError> {
        Ok(self.pool.get()?)
    }

    /// Borrow a raw pool reference (for callers that need to hold
    /// a connection across an `await`).
    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    pub fn identity_path(&self) -> &Path {
        &self.identity_path
    }

    // --- Repos --------------------------------------------------------------

    pub fn stories(&self) -> StoryRepo<'_> {
        StoryRepo::new(self)
    }

    pub fn characters(&self) -> CharacterRepo<'_> {
        CharacterRepo::new(self)
    }

    pub fn environments(&self) -> EnvironmentRepo<'_> {
        EnvironmentRepo::new(self)
    }

    pub fn props(&self) -> PropRepo<'_> {
        PropRepo::new(self)
    }

    pub fn scenes(&self) -> SceneRepo<'_> {
        SceneRepo::new(self)
    }

    pub fn keys(&self) -> KeyRepo<'_> {
        KeyRepo::new(self)
    }

    pub fn jobs(&self) -> JobRepo<'_> {
        JobRepo::new(self)
    }

    pub fn proposals(&self) -> ProposalRepo<'_> {
        ProposalRepo::new(self)
    }

    pub fn proposal_actions(&self) -> ProposalActionRepo<'_> {
        ProposalActionRepo::new(self)
    }

    pub fn agent_actions(&self) -> AgentActionRepo<'_> {
        AgentActionRepo::new(self)
    }
}

impl std::fmt::Debug for World {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("World")
            .field("db_path", &self.db_path)
            .field("identity_path", &self.identity_path)
            .finish()
    }
}
