//! `KeyRepo` — typed view over the `provider_key` table. Each row
//! is an age-encrypted ciphertext under the per-machine X25519
//! identity at `data_dir/identity.age`.

use rusqlite::OptionalExtension as _;
use secrecy::{ExposeSecret as _, SecretString};

use crate::error::StoreError;
use crate::identity;
use crate::key_id::{KeyHandle, ProviderKeyId};
use crate::time_util::now_iso;
use crate::world::World;

pub struct KeyRepo<'w> {
    world: &'w World,
}

impl<'w> KeyRepo<'w> {
    pub(crate) fn new(world: &'w World) -> Self {
        Self { world }
    }

    pub fn put(
        &self,
        id: &ProviderKeyId,
        plaintext: &SecretString,
    ) -> Result<(), StoreError> {
        let recipient = identity::read_recipient(&self.world.identity_path())?;
        let ciphertext = age::encrypt(&recipient, plaintext.expose_secret().as_bytes())
            .map_err(|e| StoreError::EncryptDecrypt(e.to_string()))?;
        let conn = self.world.conn()?;
        let now = now_iso();
        conn.execute(
            "INSERT INTO provider_key (provider, key_id, ciphertext, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(provider, key_id) DO UPDATE SET
                 ciphertext = excluded.ciphertext,
                 created_at = excluded.created_at",
            rusqlite::params![id.provider, id.key_id, ciphertext, now],
        )?;
        Ok(())
    }

    pub fn get(&self, id: &ProviderKeyId) -> Result<KeyHandle, StoreError> {
        let conn = self.world.conn()?;
        let ciphertext: Option<Vec<u8>> = conn
            .query_row(
                "SELECT ciphertext FROM provider_key WHERE provider = ?1 AND key_id = ?2",
                rusqlite::params![id.provider, id.key_id],
                |row| row.get(0),
            )
            .optional()?;
        let ciphertext = ciphertext.ok_or_else(|| StoreError::NotFound {
            provider: id.provider.clone(),
            key_id: id.key_id.clone(),
        })?;
        let identity = identity::read_identity(&self.world.identity_path())?;
        let plaintext_bytes = age::decrypt(&identity, &ciphertext)
            .map_err(|e| StoreError::EncryptDecrypt(e.to_string()))?;
        let plaintext = String::from_utf8(plaintext_bytes)
            .map_err(|_| StoreError::Other("stored key is not valid UTF-8".into()))?;
Ok(KeyHandle::from_decrypted(id.clone(), plaintext))
    }

    pub fn list_ids(&self) -> Result<Vec<ProviderKeyId>, StoreError> {
        let conn = self.world.conn()?;
        let mut stmt = conn.prepare(
            "SELECT provider, key_id FROM provider_key ORDER BY provider, key_id",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows
            .into_iter()
            .map(|(provider, key_id)| ProviderKeyId { provider, key_id })
            .collect())
    }

    pub fn delete(&self, id: &ProviderKeyId) -> Result<bool, StoreError> {
        let conn = self.world.conn()?;
        let removed = conn.execute(
            "DELETE FROM provider_key WHERE provider = ?1 AND key_id = ?2",
            rusqlite::params![id.provider, id.key_id],
        )?;
        Ok(removed > 0)
    }

    /// Sanity-check helper used by integration tests.
    #[doc(hidden)]
    pub fn _count(&self) -> Result<i64, StoreError> {
        let conn = self.world.conn()?;
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM provider_key", [], |r| r.get(0))?;
        Ok(n)
    }
}
