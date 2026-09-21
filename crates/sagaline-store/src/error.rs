//! Errors raised by the world store. Narrow on purpose — anything
//! else is a bug. Collapses the former `KeyError` (BYOK encrypt /
//! decrypt / not-found) since the key store now lives here.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("world db path is not inside an existing directory: {0}")]
    BadDataDir(String),

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("connection pool error: {0}")]
    Pool(#[from] r2d2::Error),

    #[error("migration error: {0}")]
    Migrate(String),

    #[error("schema migration was attempted on a database already at or above the target version")]
    AlreadyApplied,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// The store has no row registered under the requested
    /// provider + key_id pair (or job_id for `jobs`).
    #[error("not found: `{provider}` / `{key_id}`")]
    NotFound { provider: String, key_id: String },

    /// age encryption failed.
    #[error("key encryption layer error: {0}")]
    EncryptDecrypt(String),

    #[error("other: {0}")]
    Other(String),
}
