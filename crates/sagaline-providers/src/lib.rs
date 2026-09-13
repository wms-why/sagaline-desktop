//! Sagaline model providers.
//!
//! Concrete implementations of [`sagaline_core::provider`] traits. BYOK
//! keys are supplied at construction time — providers never read keys
//! from disk; that responsibility lives in the app shell so the
//! provider crate stays pure.
//!
//! Top-level API:
//!
//! ```no_run
//! use sagaline_providers::{ApiKey, ProviderRegistry, MiniMaxConfig};
//! use sagaline_core::provider::{TextRequest, ModelProvider, TextToText};
//!
//! # async fn demo() -> anyhow::Result<()> {
//! let mut reg = ProviderRegistry::new();
//! reg.register_minimax("minimax",
//!     MiniMaxConfig::default(),
//!     ApiKey::from_env_var("MiniMax_API_KEY")?,
//! )?;
//!
//! let p = reg.get("minimax").expect("just inserted");
//! let resp = p.generate_text(TextRequest {
//!     prompt: "hi".into(),
//!     ..Default::default()
//! }).await?;
//! println!("{}", resp.text);
//! # Ok(()) }
//! ```

mod http;
mod minimax;
mod mixed;
mod provider;
mod registry;

pub mod error;
pub mod key;

pub use provider::Provider;
pub use registry::ProviderRegistry;

pub use minimax::{MiniMaxConfig, MiniMaxProvider};
pub use mixed::{MixedConfig, MixedProvider, MixedSlot};

pub use key::ApiKey;