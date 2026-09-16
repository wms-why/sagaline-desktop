//! redb-backed BYOK store, age-encrypted.
//!
//! Owns the unified [`SagalineStore`] — a single `redb::Database` at
//! `~/.sageline/data/keys.db` that holds **both** the encrypted key
//! table (`provider_key`) and the job table (`jobs`). The previous
//! design opened two `Database` handles against the same file, which
//! redb forbids; the unified store sidesteps that with one handle and
//! gives callers typed accessors: [`SagalineStore::keys`] (returns a
//! [`KeyStore`]) and [`SagalineStore::jobs`] (returns a [`JobStore`]).
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr as _;

use redb::{Database, ReadableTable as _, TableDefinition};
use secrecy::{ExposeSecret as _, SecretString};

use crate::error::KeyError;
use crate::jobs::JobStore;

/// Key table. Key = `<provider>/<key_id>`, value = age ciphertext.
pub(crate) const KEY_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("provider_key");

/// Job table. Key = `job_id`, value = JSON-encoded [`crate::Job`].
pub(crate) const JOB_TABLE: TableDefinition<&str, &str> = TableDefinition::new("jobs");

/// The unified Sagaline data store: one redb file, two tables, one
/// X25519 identity for the encrypted key column. Hold one of these
/// per process; it owns the only `Database` handle.
pub struct SagalineStore {
    db: Database,
    /// Path to the X25519 identity file. The secret identity is
    /// re-read from disk on every `keys().get(...)` so it never sits
    /// in memory long-term and doesn't pollute `Debug` output.
    identity_path: PathBuf,
    /// Cached db path for `db_path()`.
    db_path: PathBuf,
}

impl SagalineStore {
    /// Open or create the unified store at `~/.sageline/data/`.
    pub fn open(data_dir: &Path) -> Result<Self, KeyError> {
        std::fs::create_dir_all(data_dir).map_err(|e| {
            KeyError::StoreDir(format!("{}: {e}", data_dir.display()))
        })?;

        let identity_path = data_dir.join("identity.age");
        load_or_create_identity(&identity_path)?;

        let db_path = data_dir.join("keys.db");
        let db = Database::create(&db_path)?;

        // Initialize both tables; subsequent opens reuse them.
        let txn = db.begin_write().map_err(|e| KeyError::Database(e.into()))?;
        {
            txn.open_table(KEY_TABLE).map_err(|e| KeyError::Database(e.into()))?;
            txn.open_table(JOB_TABLE).map_err(|e| KeyError::Database(e.into()))?;
        }
        txn.commit().map_err(|e| KeyError::Database(e.into()))?;

        Ok(Self {
            db,
            identity_path,
            db_path,
        })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Typed accessor for the encrypted key column.
    pub fn keys(&self) -> KeyStore<'_> {
        KeyStore { store: self }
    }

    /// Typed accessor for the job column.
    pub fn jobs(&self) -> JobStore<'_> {
        JobStore { store: self }
    }

    pub(crate) fn db(&self) -> &Database {
        &self.db
    }

    pub(crate) fn identity_path(&self) -> &Path {
        &self.identity_path
    }
}

impl fmt::Debug for SagalineStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SagalineStore")
            .field("db_path", &self.db_path)
            .field("identity_path", &self.identity_path)
            .finish_non_exhaustive()
    }
}

/// Composite identifier `provider/key_id`. Validated at construction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProviderKeyId {
    pub provider: String,
    pub key_id: String,
}

impl ProviderKeyId {
    pub fn new(provider: impl Into<String>, key_id: impl Into<String>) -> Result<Self, KeyError> {
        let provider = provider.into();
        let key_id = key_id.into();
        check_id(&provider, "provider")?;
        check_id(&key_id, "key_id")?;
        Ok(Self { provider, key_id })
    }

    fn pk(&self) -> String {
        format!("{}/{}", self.provider, self.key_id)
    }
}

impl fmt::Display for ProviderKeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.provider, self.key_id)
    }
}

/// A decrypted key. Plaintext is exposed only via [`SecretString`].
#[derive(Debug)]
pub struct KeyHandle {
    id: ProviderKeyId,
    plaintext: SecretString,
}

impl KeyHandle {
    pub fn reveal(&self) -> &SecretString {
        &self.plaintext
    }

    pub fn id(&self) -> &ProviderKeyId {
        &self.id
    }

    /// Test-only constructor. Wraps a `&'static str` in a
    /// `KeyHandle` without touching the redb store. The name
    /// encodes intent: production code paths construct `KeyHandle`
    /// only through [`KeyStore::get`]. There is no `cfg(test)` gate
    /// because integration tests in downstream crates
    /// (e.g. `sagaline-providers`) need the same entrypoint.
    pub fn from_static_for_test(plaintext: &'static str) -> Self {
        Self {
            id: ProviderKeyId::new("test", "default").expect("static test id is valid"),
            plaintext: SecretString::new(Box::from(plaintext)),
        }
    }
}

/// Typed view over the key column of a [`SagalineStore`]. Cheap to
/// construct; borrow the store and call `.keys()`.
pub struct KeyStore<'a> {
    store: &'a SagalineStore,
}

impl<'a> KeyStore<'a> {
    pub fn db_path(&self) -> &Path {
        self.store.db_path()
    }

    pub fn put(
        &self,
        id: &ProviderKeyId,
        plaintext: &SecretString,
    ) -> Result<(), KeyError> {
        let recipient = read_recipient(self.store.identity_path())?;
        let ciphertext = age::encrypt(&recipient, plaintext.expose_secret().as_bytes())?;
        let pk = id.pk();
        let txn = self.store.db().begin_write().map_err(|e| KeyError::Database(e.into()))?;
        {
            let mut table = txn.open_table(KEY_TABLE).map_err(|e| KeyError::Database(e.into()))?;
            table
                .insert(pk.as_str(), ciphertext.as_slice())
                .map_err(|e| KeyError::Database(e.into()))?;
        }
        txn.commit().map_err(|e| KeyError::Database(e.into()))?;
        Ok(())
    }

    pub fn get(&self, id: &ProviderKeyId) -> Result<KeyHandle, KeyError> {
        let pk = id.pk();
        let ciphertext: Vec<u8> = {
            let txn = self.store.db().begin_read().map_err(|e| KeyError::Database(e.into()))?;
            let table = txn.open_table(KEY_TABLE).map_err(|e| KeyError::Database(e.into()))?;
            let guard = table
                .get(pk.as_str())
                .map_err(|e| KeyError::Database(e.into()))?
                .ok_or_else(|| KeyError::NotFound {
                    provider: id.provider.clone(),
                    key_id: id.key_id.clone(),
                })?;
            guard.value().to_vec()
        };

        let identity = read_identity(self.store.identity_path())?;
        let plaintext_bytes = age::decrypt(&identity, &ciphertext)?;
        let plaintext = String::from_utf8(plaintext_bytes).map_err(|_| KeyError::NotUtf8)?;

        Ok(KeyHandle {
            id: id.clone(),
            plaintext: SecretString::new(plaintext.into_boxed_str()),
        })
    }

    pub fn list_ids(&self) -> Result<Vec<ProviderKeyId>, KeyError> {
        let txn = self.store.db().begin_read().map_err(|e| KeyError::Database(e.into()))?;
        let table = txn.open_table(KEY_TABLE).map_err(|e| KeyError::Database(e.into()))?;
        let mut out = Vec::new();
        for entry in table.iter().map_err(|e| KeyError::Database(e.into()))? {
            let (key, _) = entry.map_err(|e| KeyError::Database(e.into()))?;
            let key_str = key.value();
            if let Some((p, k)) = key_str.split_once('/') {
                out.push(ProviderKeyId {
                    provider: p.to_string(),
                    key_id: k.to_string(),
                });
            }
        }
        // redb iterates by sorted key; `provider/key_id` sorts alphabetically
        // within provider. No re-sort needed.
        Ok(out)
    }

    pub fn delete(&self, id: &ProviderKeyId) -> Result<bool, KeyError> {
        let txn = self.store.db().begin_write().map_err(|e| KeyError::Database(e.into()))?;
        // `remove` returns `Result<Option<AccessGuard<V>>>` where the guard
        // borrows the table. Scope the call inside a block so the guard is
        // dropped before we touch the transaction again.
        let removed: bool = {
            let pk = id.pk();
            let mut table = txn.open_table(KEY_TABLE).map_err(|e| KeyError::Database(e.into()))?;
            let result = table
                .remove(pk.as_str())
                .map_err(|e| KeyError::Database(e.into()))?;
            matches!(result, Some(_))
        };
        txn.commit().map_err(|e| KeyError::Database(e.into()))?;
        Ok(removed)
    }
}

fn read_identity(path: &Path) -> Result<age::x25519::Identity, KeyError> {
    let text = std::fs::read_to_string(path)?;
    age::x25519::Identity::from_str(text.trim()).map_err(|e| {
        KeyError::StoreDir(format!(
            "identity file at {} is malformed ({e}); delete it to mint a fresh one",
            path.display()
        ))
    })
}

fn read_recipient(path: &Path) -> Result<age::x25519::Recipient, KeyError> {
    Ok(read_identity(path)?.to_public())
}

fn load_or_create_identity(path: &Path) -> Result<(), KeyError> {
    if path.exists() {
        let _ = read_identity(path)?;
        return Ok(());
    }

    let identity = age::x25519::Identity::generate();
    let secret = identity.to_string(); // SecretString
    std::fs::write(path, secret.expose_secret().as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}

fn check_id(s: &str, label: &str) -> Result<(), KeyError> {
    if s.is_empty() || s.len() > 64 {
        return Err(KeyError::InvalidId(format!("{label}: empty or >64 chars")));
    }
    if !s
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(KeyError::InvalidId(format!(
            "{label}: only [A-Za-z0-9_-] allowed, got {s:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Job, JobStatus};
    use secrecy::SecretString;
    use tempfile::tempdir;

    fn fresh() -> (tempfile::TempDir, KeyStore<'static>) {
        let dir = tempdir().unwrap();
        let store: &'static SagalineStore =
            Box::leak(Box::new(SagalineStore::open(dir.path()).unwrap()));
        (dir, store.keys())
    }

    #[test]
    fn round_trip_a_key() {
        let (_dir, s) = fresh();
        let id = ProviderKeyId::new("minimax", "work-laptop").unwrap();
        s.put(
            &id,
            &SecretString::new("sk-test-1234567890".to_string().into_boxed_str()),
        )
        .unwrap();
        let handle = s.get(&id).unwrap();
        assert_eq!(handle.reveal().expose_secret(), "sk-test-1234567890");
    }

    #[test]
    fn get_missing_returns_not_found() {
        let (_dir, s) = fresh();
        let id = ProviderKeyId::new("minimax", "missing").unwrap();
        let err = s.get(&id).unwrap_err();
        assert!(matches!(err, KeyError::NotFound { .. }));
    }

    #[test]
    fn put_overwrites_existing() {
        let (_dir, s) = fresh();
        let id = ProviderKeyId::new("minimax", "k").unwrap();
        s.put(&id, &SecretString::new("old".to_string().into_boxed_str())).unwrap();
        s.put(&id, &SecretString::new("new".to_string().into_boxed_str())).unwrap();
        assert_eq!(s.get(&id).unwrap().reveal().expose_secret(), "new");
    }

    #[test]
    fn list_ids_orders_by_provider_then_key() {
        let (_dir, s) = fresh();
        for (p, k) in [("openai", "z"), ("minimax", "a"), ("minimax", "b")] {
            let id = ProviderKeyId::new(p, k).unwrap();
            s.put(&id, &SecretString::new("x".to_string().into_boxed_str())).unwrap();
        }
        let ids = s.list_ids().unwrap();
        let names: Vec<String> = ids.iter().map(ToString::to_string).collect();
        assert_eq!(names, vec!["minimax/a", "minimax/b", "openai/z"]);
    }

    #[test]
    fn delete_removes_entry() {
        let (_dir, s) = fresh();
        let id = ProviderKeyId::new("minimax", "k").unwrap();
        s.put(&id, &SecretString::new("x".to_string().into_boxed_str())).unwrap();
        assert!(s.delete(&id).unwrap());
        assert!(matches!(
            s.get(&id).unwrap_err(),
            KeyError::NotFound { .. }
        ));
        assert!(!s.delete(&id).unwrap());
    }

    #[test]
    fn invalid_ids_are_rejected() {
        for bad in ["", "with space", "with/slash", "with.dot", "x".repeat(65).as_str()] {
            assert!(ProviderKeyId::new("ok", bad).is_err(), "should reject {bad:?}");
            assert!(ProviderKeyId::new(bad, "ok").is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn reopen_uses_existing_identity() {
        let dir = tempdir().unwrap();
        let id = ProviderKeyId::new("minimax", "k").unwrap();
        {
            let s1 = SagalineStore::open(dir.path()).unwrap();
            s1.keys().put(&id, &SecretString::new("persistent".to_string().into_boxed_str())).unwrap();
        }
        let s2 = SagalineStore::open(dir.path()).unwrap();
        assert_eq!(
            s2.keys().get(&id).unwrap().reveal().expose_secret(),
            "persistent"
        );
    }

    #[test]
    fn ciphertext_does_not_contain_plaintext() {
        let dir = tempdir().unwrap();
        let s = SagalineStore::open(dir.path()).unwrap();
        let id = ProviderKeyId::new("minimax", "k").unwrap();
        let secret = "sk-very-secret-token";
        s.keys()
            .put(&id, &SecretString::new(secret.to_string().into_boxed_str()))
            .unwrap();

        let db_bytes = std::fs::read(s.db_path()).unwrap();
        let needle_present = db_bytes.windows(secret.len()).any(|w| w == secret.as_bytes());
        assert!(!needle_present, "plaintext must not appear in db file");
    }

    /// Smoke test for the unified store: opening one SagalineStore
    /// gives you both columns, and the two views can be used
    /// against the same DB without contention. This is the fix for
    /// the "two `Database` handles on one file" redb error that the
    /// previous design carried.
    #[test]
    fn sagaline_store_unifies_keys_and_jobs() {
        let dir = tempdir().unwrap();
        let s = SagalineStore::open(dir.path()).unwrap();
        // write a key
        let key_id = ProviderKeyId::new("minimax", "k").unwrap();
        s.keys()
            .put(&key_id, &SecretString::new("sk-x".to_string().into_boxed_str()))
            .unwrap();
        // write a job
        let job = Job {
            job_id: "j1".into(),
            shot_id: "s1".into(),
            capability: "image".into(),
            provider: "minimax".into(),
            model_id: "image-01".into(),
            provider_task_id: None,
            status: JobStatus::Queued,
            started_at: None,
            finished_at: None,
            asset_path: None,
            attempt: 1,
            error: None,
        };
        s.jobs().put(&job).unwrap();
        // both readable through the same handle
        assert_eq!(s.keys().list_ids().unwrap().len(), 1);
        assert_eq!(s.jobs().list_by_status(JobStatus::Queued).unwrap().len(), 1);
    }
}

// `JobStore` lives in `crate::jobs`; the type lives in a different
// module for clarity, but it borrows from `SagalineStore` too.
