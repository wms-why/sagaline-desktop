//! X25519 identity file management. One identity per machine, stored
//! at `~/.sageline/identity.age` (mode 0600 on Unix). Used to encrypt
//! + decrypt provider keys in the `provider_key` table.

use std::path::Path;
use std::str::FromStr as _;

use age::x25519::{Identity, Recipient};
use secrecy::ExposeSecret as _;

use crate::error::StoreError;

/// Mint a fresh identity if one doesn't exist; otherwise validate the
/// existing file parses. Called from [`crate::World::open`].
pub fn load_or_create_identity(path: &Path) -> Result<(), StoreError> {
    if path.exists() {
        // Validate it parses; don't keep it in memory.
        let _ = read_identity(path)?;
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let identity = Identity::generate();
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

pub fn read_identity(path: &Path) -> Result<Identity, StoreError> {
    let text = std::fs::read_to_string(path)?;
    Identity::from_str(text.trim()).map_err(|e| {
        StoreError::Other(format!(
            "identity file at {} is malformed ({e}); delete it to mint a fresh one",
            path.display()
        ))
    })
}

pub fn read_recipient(path: &Path) -> Result<Recipient, StoreError> {
    Ok(read_identity(path)?.to_public())
}
