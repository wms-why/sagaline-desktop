//! The provider registry.
//!
//! Maps `(provider, capability)` to a concrete backend. The agent
//! asks the registry for a backend by name; the registry picks
//! from the registered backends.
//!
//! ## Storage
//!
//! The registry holds two parallel maps:
//!
//! - `(provider, Capability::Image)` → `Arc<dyn ImageGen>`
//! - `(provider, _)`                  → `Arc<dyn ModelAdapter>` (any cap)
//!
//! Adding a new capability means adding a new map + a `pick_<cap>`
//! method. The maps are filled at registration by inspecting the
//! backend's `capabilities()`.
//!
//! ## Chat
//!
//! Chat backends are *not* registered here. The chat factory lives
//! in [`crate::openai_compat`] and returns a concrete
//! `Arc<ChatModel>` from `(base_url, model_id, &KeyHandle)`. The
//! app shell constructs chat models per call, using config.toml
//! + KeyStore for resolution. Storing chat models in this
//! registry would require `Arc<dyn CompletionModel>` — but rig's
//! [`CompletionModel`] trait is not dyn-compatible (`impl Trait`
//! in return position), so we keep chat construction explicit at
//! the call site instead.

use std::collections::HashMap;
use std::sync::Arc;

use crate::adapter::{Capability, ModelAdapter};
use crate::error::ProviderError;
use crate::image_gen::ImageGen;

/// One registered backend (the model-adapter view).
#[derive(Clone)]
struct Entry {
    adapter: Arc<dyn ModelAdapter>,
    capabilities: Vec<Capability>,
}

/// The registry. Cheap to clone.
#[derive(Default, Clone)]
pub struct ProviderRegistry {
    entries: HashMap<String, Entry>,
    image_backends: HashMap<String, Arc<dyn ImageGen>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a backend. The provider's `provider_name()` becomes
    /// the lookup key. If the backend implements `ImageGen`
    /// (and advertises `Capability::Image`), the typed map is
    /// populated automatically.
    pub fn register<A: ModelAdapter + 'static>(&mut self, adapter: A) {
        let capabilities = adapter.capabilities().to_vec();
        let provider_name = adapter.provider_name().to_string();

        // Only backends that actually implement ImageGen land in
        // the typed map. We do the downcast-by-trait via the helper
        // `register_image` so callers stay explicit.
        let entry = Entry {
            capabilities,
            adapter: Arc::new(adapter),
        };
        self.entries.insert(provider_name, entry);
    }

    /// Register an image backend (also populates the generic
    /// entries map). Convenience for backends that only implement
    /// `ImageGen`.
    pub fn register_image<A: ModelAdapter + ImageGen + 'static>(&mut self, backend: A) {
        let provider_name = backend.provider_name().to_string();
        let capabilities = backend.capabilities().to_vec();
        let image: Arc<dyn ImageGen> = Arc::new(backend);
        let entry = Entry {
            capabilities,
            adapter: image.clone(),
        };
        self.entries.insert(provider_name.clone(), entry);
        self.image_backends.insert(provider_name, image);
    }

    /// Generic pick: returns the model adapter (any capability).
    pub fn pick(
        &self,
        provider: &str,
        capability: Capability,
    ) -> Result<Arc<dyn ModelAdapter>, ProviderError> {
        let entry = self
            .entries
            .get(provider)
            .ok_or_else(|| ProviderError::Unknown(provider.to_string()))?;
        if !entry.capabilities.contains(&capability) {
            return Err(ProviderError::Unsupported {
                provider: provider.to_string(),
                capability,
            });
        }
        Ok(entry.adapter.clone())
    }

    /// Typed pick: `Arc<dyn ImageGen>`. Requires the backend was
    /// registered through [`Self::register_image`].
    pub fn pick_image(&self, provider: &str) -> Result<Arc<dyn ImageGen>, ProviderError> {
        self.image_backends
            .get(provider)
            .cloned()
            .ok_or_else(|| match self.entries.get(provider) {
                None => ProviderError::Unknown(provider.to_string()),
                Some(_) => ProviderError::Unsupported {
                    provider: provider.to_string(),
                    capability: Capability::Image,
                },
            })
    }

    /// All registered provider names (sorted, for debug / UI).
    pub fn providers(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.entries.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Resolved provider bundle for the agent: which backend to use,
/// its name + id, and where to find its key. Produced by the
/// registry after combining config + key store.
pub struct ResolvedProvider {
    pub provider_name: String,
    pub model_id: String,
    pub backend: Arc<dyn ModelAdapter>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use crate::adapter::{GenerationOutput, GenerationRequest};

    /// A trivial test backend that implements only ImageGen.
    #[derive(Clone)]
    struct ImageStub;

    #[async_trait]
    impl ModelAdapter for ImageStub {
        fn id(&self) -> &str {
            "stub-v0"
        }
        fn provider_name(&self) -> &str {
            "stub"
        }
        fn capabilities(&self) -> &[Capability] {
            &[Capability::Image]
        }
    }

    #[async_trait]
    impl ImageGen for ImageStub {
        async fn generate(
            &self,
            _req: GenerationRequest<'_>,
        ) -> Result<GenerationOutput, ProviderError> {
            Ok(GenerationOutput {
                bytes: vec![0u8; 4],
                mime: "image/png",
                provider_job_id: None,
            })
        }
    }

    #[test]
    fn register_image_routes_correctly() {
        let mut r = ProviderRegistry::new();
        r.register_image(ImageStub);

        assert_eq!(r.providers(), vec!["stub"]);
        assert!(r.pick_image("stub").is_ok());
        assert!(matches!(
            r.pick_image("nope"),
            Err(ProviderError::Unknown(_))
        ));
        // Chat is intentionally not in this registry.
        assert!(r.pick("stub", Capability::Chat).is_err());
    }
}
