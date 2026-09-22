//! Sagaline desktop binary — app shell.
//!
//! Owns the long-lived [`AppEnv`] (provider registry, config, encrypted
//! key store, job store, prefs, story store) and the gpui-side event
//! plumbing that ferries the agent's `AgentEvent` stream into the UI's
//! chain-of-thought panel.
//!
//! The actual `main` lives in `main.rs`; it boots the gpui app,
//! installs the [`AppEnv`] as a global, opens the window, and hands
//! off to `sagaline-ui`'s `WorkspaceView`.

pub mod env;
pub mod prefs;
pub mod service;

pub use env::{AppEnv, EnvError};
pub use service::AppEnvStoryService;
pub type AppEnvSlot = sagaline_ui::EnvSlot<AppEnv>;

use std::sync::Arc;
use gpui_kit::*;
use tracing::info;
use sagaline_core::Story;
use sagaline_ui::AgentEventLog;

pub fn install_env(cx: &mut App) -> Result<Arc<AppEnv>, EnvError> {
    if cx.has_global::<AppEnvSlot>() {
        return Ok(cx.global::<AppEnvSlot>().0.clone());
    }
    let env = Arc::new(AppEnv::open()?);
    cx.set_global::<AppEnvSlot>(sagaline_ui::EnvSlot(env.clone()));
    cx.set_global::<AgentEventLog>(AgentEventLog::default());
    // The BYOK panel reads the encrypted key store out of this
    // global. Same `Arc<SagalineStore>` is held by `AppEnv`; the
    // global is the convenience handle the view layer reaches
    // without a dependency on this binary crate.
    cx.set_global::<sagaline_ui::KeyStoreSlot>(sagaline_ui::KeyStoreSlot(
        env.store.clone(),
    ));
    // The UI's new-story / project-settings flows call into
    // `StoryService`. The binary owns the concrete impl; the UI
    // only sees the trait object through the newtype global.
    cx.set_global::<sagaline_ui::StoryServiceSlot>(
        sagaline_ui::StoryServiceSlot(Box::new(AppEnvStoryService::new(env.clone()))),
    );
    // The activity panel's commit-policy picker + approve /
    // reject buttons call into `ProposalService`. The binary
    // owns the concrete impl (which holds `Arc<AppEnv>` and
    // routes through the agent's tool registry); the UI only
    // sees the trait object.
    cx.set_global::<sagaline_ui::ProposalServiceSlot>(
        sagaline_ui::ProposalServiceSlot(Box::new(
            crate::service::AppEnvProposalService::new(env.clone()),
        )),
    );
    // The Tokio ↔ GPUI bridge. GPUI tasks run on their own
    // scheduler (not Tokio), so anything that touches `tokio::fs`
    // / `tokio::task::spawn_blocking` / `reqwest` must route
    // through `env.runtime` via this bridge — see
    // `sagaline-bridge` for the drop / cancellation contract.
    cx.set_global::<sagaline_bridge::BridgeSlot>(sagaline_bridge::BridgeSlot(
        sagaline_bridge::TokioBridge::for_app(env.tokio_handle(), cx),
    ));
    info!(data_dir = %env.data_dir.display(), "AppEnv installed");
    Ok(env)
}

/// Spawn the agent's `OBSERVE → PLAN → ACT → REFLECT` loop against
/// the story behind `story` and route every event into
/// [`AgentEventLog`].
///
/// Two tasks:
///
/// - **Stream consumer** runs on the Tokio runtime via
///   [`sagaline_bridge::TokioBridge::spawn`]. The agent returns a
///   `Stream<Item = AgentEvent>` whose `poll_next` does real I/O
///   (`tokio::fs::read`, future reqwest calls), so it must be
///   driven from a Tokio context. The consumer forwards each
///   event through an mpsc channel.
///
/// - **Forwarder** lives on GPUI's executor (`cx.spawn`). It
///   polls the channel's receiver (safe from any executor
///   because `mpsc` queues internally and wakes its waker on
///   send) and pushes each event into [`AgentEventLog`],
///   refreshing the windows.
///
/// Dropping the gpui window (or calling this function twice) will
/// drop both tasks; the Tokio side cancels at the next stream
/// yield, the gpui side drains the channel and exits.
pub fn run_agent(env: Arc<AppEnv>, story: Arc<Story>, cx: &mut App) {
    let bridge = cx.global::<sagaline_bridge::BridgeSlot>().0.clone();

    // Phase 2.5: the loop drives the SQLite `World`, not the
    // Markdown `Story`. We resolve the world DB row by slug
    // (the path's last component is the story's slug); if no
    // row exists we still wire the stream so the UI surfaces a
    // clean `Done { reason: "load failed: ..." }` instead of
    // crashing.
    let story_path = story.handle().path().to_path_buf();
    let agent = env.build_agent(&story_path);
    let slug = story_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let story_id = match env.store.stories().get_by_slug(&slug) {
        Ok(Some(row)) => row.id,
        _ => String::new(),
    };
    let (mut rx, _stream_task) = bridge.forward_stream(agent.run_stream(env.store.clone(), &story_id));

    // Forwarder: gpui task drains the channel and updates the log.
    cx.spawn(async move |async_cx: &mut AsyncApp| {
        while let Some(event) = rx.recv().await {
            async_cx.update_global::<AgentEventLog, _>(|log, app| {
                log.events.push(event);
                app.refresh_windows();
            });
        }
    })
    .detach();
}
