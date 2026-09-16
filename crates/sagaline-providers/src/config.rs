//! `~/.sageline/data/config.toml` parser.
//!
//! Per-provider public config (base_url, default model). **No keys**
//! live here — those are in `~/.sageline/data/keys.db` via
//! [`sagaline_keys::KeyStore`].
//!
//! ## File shape
//!
//! ```toml
//! [chat.openai]
//! base_url = "https://api.openai.com/v1"
//! model    = "gpt-4o-mini"
//!
//! [chat.deepseek]
//! base_url = "https://api.deepseek.com/v1"
//! model    = "deepseek-chat"
//!
//! [chat.minimax]
//! base_url = "https://api.minimax.chat/v1"
//! model    = "MiniMax-Text-01"
//!
//! [image.minimax]
//! base_url = "https://api.minimax.chat/v1"
//! model    = "image-01"
//!
//! [image.openai]
//! base_url = "https://api.openai.com/v1"
//! model    = "gpt-image-1"
//!
//! [tts.minimax]
//! base_url = "https://api.minimax.chat/v1"
//! model    = "speech-2.8-hd"
//! ```
//!
//! Each `[<capability>.<provider>]` table binds a logical provider to
//! a base_url + default model. Capabilities: `chat`, `image`,
//! `image_to_video`, `tts`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::Capability;

/// Logical key in the config file: `(capability, provider)`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConfigKey {
    pub capability: Capability,
    pub provider: String,
}

/// Per-capability, per-provider public config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Base URL of the API (no trailing slash). For OpenAI-compatible
    /// chat backends, this is e.g. `https://api.openai.com/v1`.
    pub base_url: String,
    /// Default model id (e.g. `"gpt-4o-mini"`, `"image-01"`).
    pub model: String,
}

/// The whole config set: keyed by `(capability, provider)`.
#[derive(Debug, Clone, Default)]
pub struct ProviderConfigSet {
    inner: BTreeMap<ConfigKey, ProviderConfig>,
}

impl ProviderConfigSet {
    /// Load from a TOML file. Missing file → empty set.
    pub fn load(path: &Path) -> Result<Self, ProviderConfigError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)?;
        let raw: RawConfig = toml::from_str(&text)?;
        let mut inner = BTreeMap::new();
        for (cap_str, by_provider) in raw.0 {
            let capability = parse_capability(&cap_str)?;
            for (provider, cfg) in by_provider {
                inner.insert(
                    ConfigKey {
                        capability,
                        provider: provider.clone(),
                    },
                    cfg,
                );
            }
        }
        Ok(Self { inner })
    }

    /// Look up a config entry. Returns `None` if the (capability,
    /// provider) pair has no entry.
    pub fn get(&self, capability: Capability, provider: &str) -> Option<&ProviderConfig> {
        self.inner.get(&ConfigKey {
            capability,
            provider: provider.to_string(),
        })
    }

    /// Number of entries (testing / debug).
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

/// Raw config file shape. Outer map: capability name → provider map.
/// Inner map: provider name → `ProviderConfig`.
#[derive(Debug, Deserialize)]
struct RawConfig(BTreeMap<String, BTreeMap<String, ProviderConfig>>);

fn parse_capability(s: &str) -> Result<Capability, ProviderConfigError> {
    match s {
        "chat" => Ok(Capability::Chat),
        "image" => Ok(Capability::Image),
        "image_to_video" => Ok(Capability::ImageToVideo),
        "text_to_video" => Ok(Capability::TextToVideo),
        "tts" => Ok(Capability::Tts),
        "vision_critique" => Ok(Capability::VisionCritique),
        other => Err(ProviderConfigError::UnknownCapability(other.into())),
    }
}

/// Config-loading errors.
#[derive(Debug, Error)]
pub enum ProviderConfigError {
    #[error("config.toml I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("config.toml parse error: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("unknown capability in config.toml: `{0}`")]
    UnknownCapability(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_config() {
        let toml = r#"
[chat.openai]
base_url = "https://api.openai.com/v1"
model    = "gpt-4o-mini"

[image.minimax]
base_url = "https://api.minimax.chat/v1"
model    = "image-01"
"#;
        let raw: RawConfig = toml::from_str(toml).unwrap();
        assert_eq!(raw.0.len(), 2);
    }

    #[test]
    fn load_missing_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let set = ProviderConfigSet::load(&dir.path().join("nope.toml")).unwrap();
        assert!(set.is_empty());
    }

    #[test]
    fn round_trip_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(
            &p,
            r#"
[image.minimax]
base_url = "https://api.minimax.chat/v1"
model    = "image-01"
"#,
        )
        .unwrap();
        let set = ProviderConfigSet::load(&p).unwrap();
        let cfg = set.get(Capability::Image, "minimax").unwrap();
        assert_eq!(cfg.model, "image-01");
        assert!(set.get(Capability::Image, "openai").is_none());
    }

    #[test]
    fn unknown_capability_errors() {
        let toml = r#"
[wat.openai]
base_url = "x"
model = "y"
"#;
        // The toml deserializer accepts any capability key (it just
        // sees `String`); `parse_capability` is what catches unknown
        // names. Round-trip through the full loader.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, toml).unwrap();
        let err = ProviderConfigSet::load(&path).unwrap_err();
}
}