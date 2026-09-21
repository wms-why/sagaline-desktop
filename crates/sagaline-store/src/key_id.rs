//! Provider key identity + decrypted handle. Lifted out of the
//! project's stable key identifier, kept compatible with the
//! original redb-backed BYOK panel.

use std::fmt;

use secrecy::SecretString;

use crate::error::StoreError;

/// Composite identifier `provider/key_id`. Validated at construction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProviderKeyId {
    pub provider: String,
    pub key_id: String,
}

impl ProviderKeyId {
    pub fn new(
        provider: impl Into<String>,
        key_id: impl Into<String>,
    ) -> Result<Self, StoreError> {
        let provider = provider.into();
        let key_id = key_id.into();
        check_id(&provider, "provider")?;
        check_id(&key_id, "key_id")?;
        Ok(Self { provider, key_id })
    }

    pub fn pk(&self) -> String {
        format!("{}/{}", self.provider, self.key_id)
    }

    pub fn from_pk(combined: &str) -> Result<Self, StoreError> {
        let (p, k) = combined.split_once('/').ok_or_else(|| {
            StoreError::Other(format!(
                "malformed provider_key id `{combined}` (expected `provider/key_id`)"
            ))
        })?;
        Self::new(p, k)
    }
}

impl fmt::Display for ProviderKeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.provider, self.key_id)
    }
}

fn check_id(s: &str, label: &str) -> Result<(), StoreError> {
    if s.is_empty() || s.len() > 64 {
        return Err(StoreError::Other(format!(
            "{label}: empty or >64 chars"
        )));
    }
    if !s
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(StoreError::Other(format!(
            "{label}: only [A-Za-z0-9_-] allowed, got {s:?}"
        )));
    }
    Ok(())
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
    /// `KeyHandle` without touching the world DB. Production code
    /// paths construct `KeyHandle` only through [`crate::World::keys`]
    /// + [`crate::repo::key::KeyRepo::get`].
    pub fn from_static_for_test(plaintext: &'static str) -> Self {
        Self {
            id: ProviderKeyId::new("test", "default").expect("static test id is valid"),
            plaintext: SecretString::new(Box::from(plaintext)),
        }
    }

    /// Crate-private constructor used by [`crate::repo::key::KeyRepo::get`]
    /// after a successful age decryption.
    pub(crate) fn from_decrypted(id: ProviderKeyId, plaintext: String) -> Self {
        Self {
            id,
            plaintext: SecretString::new(plaintext.into_boxed_str()),
        }
    }
}
