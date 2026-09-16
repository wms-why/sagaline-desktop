//! Error types for the key store.

use thiserror::Error;

/// Errors raised by the key store. Kept narrow — anything else is a bug.
#[derive(Debug, Error)]
pub enum KeyError {
    /// The store path's parent directory could not be created or the
    /// identity file's parent was missing.
    #[error("could not prepare key store directory: {0}")]
    StoreDir(String),

    /// redb-level error. redb 2.x exposes `TransactionError`,
    /// `TableError`, `CommitError`, `StorageError`, etc. — all of
    /// which `From` into `redb::Error`, so we collapse them here.
    #[error("key store database error: {0}")]
    Database(#[from] redb::Error),

    /// redb database-creation error (filesystem-level: perms, corruption).
    #[error("key store could not open/create the database file: {0}")]
    DatabaseOpen(#[from] redb::DatabaseError),

    /// age encryption failed.
    #[error("key encryption layer error: {0}")]
    Encrypt(#[from] age::EncryptError),

    /// age decryption failed (wrong key, corrupted blob).
    #[error("key decryption layer error: {0}")]
    Decrypt(#[from] age::DecryptError),

    /// The store has no key registered under the requested provider +
    /// key id pair.
    #[error("no key for provider `{provider}` under key_id `{key_id}`")]
    NotFound { provider: String, key_id: String },

    /// A key id was supplied that the schema forbids (e.g. empty, control
    /// characters). Provider names go through the same gate.
    #[error("invalid identifier `{0}`")]
    InvalidId(String),

    /// Generic I/O during identity file I/O.
    #[error("identity file I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Stored plaintext was not UTF-8 — keys are required to be ASCII.
    #[error("stored key is not valid UTF-8 (corrupted?)")]
    NotUtf8,

    /// Other error. Avoid; prefer a specific variant.
    #[error("other: {0}")]
    Other(String),}
