//! `ProviderRegistry`: the unique-id store. Per project rule, each
//! provider id is registered at most once. Re-inserting an existing
//! id is an error, not a silent overwrite.

use std::collections::HashMap;
use std::sync::Arc;

use sagaline_core::provider::{ModelProvider, ProviderCapabilities, ProviderError, ProviderId};

use crate::key::ApiKey;
use crate::minimax::{MiniMaxConfig, MiniMaxProvider};
use crate::mixed::{MixedConfig, MixedProvider};
use crate::provider::Provider;
/// Holds a `(ProviderId → Arc<Provider>)` map. Lookup is by
/// `&ProviderId`. Insertion enforces the "at most one per id" rule.
pub struct ProviderRegistry {
    by_id: HashMap<ProviderId, Arc<Provider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self { Self { by_id: HashMap::new() } }

    /// Register a MiniMax provider under the given id.
    pub fn register_minimax(
        &mut self,
        id: impl Into<ProviderId>,
        cfg: MiniMaxConfig,
        key: ApiKey,
    ) -> Result<Arc<Provider>, ProviderError> {
        let id = id.into();
        if self.by_id.contains_key(&id) {
            return Err(ProviderError::Config(format!(
                "provider id {:?} already registered", id
            )));
        }
        let p = MiniMaxProvider::new(id.clone(), cfg, key)?;
        let arc = Provider::MiniMax(p).arc();
        self.by_id.insert(id, Arc::clone(&arc));
        Ok(arc)
    }

    /// Register a Mixed provider under the given id. The three slots
    /// in `cfg` must point to providers already in the registry.
    pub fn register_mixed(
        &mut self,
        id: impl Into<ProviderId>,
        cfg: MixedConfig,
    ) -> Result<Arc<Provider>, ProviderError> {
        let id = id.into();
        if self.by_id.contains_key(&id) {
            return Err(ProviderError::Config(format!(
                "provider id {:?} already registered", id
            )));
        }
        let p = MixedProvider::new(id.clone(), cfg, self)?;
        let arc = Provider::Mixed(p).arc();
        self.by_id.insert(id, Arc::clone(&arc));
        Ok(arc)
    }

    /// Look up a provider by id.
    pub fn get(&self, id: &ProviderId) -> Option<Arc<Provider>> {
        self.by_id.get(id).cloned()
    }

    /// Iterate registered provider ids (sorted by insertion order is
    /// not guaranteed; HashMap is randomized).
    pub fn ids(&self) -> impl Iterator<Item = &ProviderId> { self.by_id.keys() }

    /// Total count (for diagnostics / UI).
    pub fn len(&self) -> usize { self.by_id.len() }
    pub fn is_empty(&self) -> bool { self.by_id.is_empty() }

    /// Helper for MixedConfig: resolve a slot's `provider_id` against
    /// the registry and validate the provider supports the required
    /// capability.
    pub(crate) fn resolve_slot(
        &self,
        slot: &crate::mixed::MixedSlot,
        need: ProviderCapabilities,
    ) -> Result<Arc<Provider>, ProviderError> {
        let p = self.by_id.get(&slot.provider_id).ok_or_else(|| {
            ProviderError::Config(format!(
                "mixed slot refers to unknown provider {:?}",
                slot.provider_id
            ))
        })?;
        let cap = p.capabilities();
        if !matches!(need, ProviderCapabilities { text_to_text: false, .. }) && !cap.text_to_text && need.text_to_text {
            return Err(ProviderError::Unsupported {
                provider: slot.provider_id.clone(),
                capability: "text_to_text",
            });
        }
        if need.text_to_image && !cap.text_to_image {
            return Err(ProviderError::Unsupported {
                provider: slot.provider_id.clone(),
                capability: "text_to_image",
            });
        }
        if need.text_image_to_video && !cap.text_image_to_video {
            return Err(ProviderError::Unsupported {
                provider: slot.provider_id.clone(),
                capability: "text_image_to_video",
            });
        }
        Ok(Arc::clone(p))
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self { Self::new() }
}