//! Sagaline desktop binary — app shell.
//!
//! Owns the long-lived [`AppEnv`] (provider registry, config, encrypted
//! key store, job store) and the gpui-side event plumbing that ferries
//! the agent's `AgentEvent` stream into the UI's chain-of-thought
//! panel.
//!
//! The actual `main` lives in `main.rs`; it boots the gpui app,
//! installs the [`AppEnv`] as a global, opens the window, and hands
//! off to `sagaline-ui`'s `WorkspaceView`.

pub mod env;
pub mod sink;

pub use env::{AppEnv, EnvError};
pub use sink::ChannelSink;

use std::sync::Arc;

use gpui_kit::*;
use tracing::{error, info};
use sagaline_agent::AgentEvent;
use sagaline_ui::AgentEventLog;

use tokio::sync::mpsc;
/// sealed trait, so we can't impl it for `Arc<AppEnv>` directly.
pub struct AppEnvSlot(pub Arc<AppEnv>);

impl Global for AppEnvSlot {}

pub fn install_env(cx: &mut App) -> Result<Arc<AppEnv>, EnvError> {
    if cx.has_global::<AppEnvSlot>() {
        return Ok(cx.global::<AppEnvSlot>().0.clone());
    }
    let env = Arc::new(AppEnv::open()?);
    cx.set_global::<AppEnvSlot>(AppEnvSlot(env.clone()));
    cx.set_global::<AgentEventLog>(AgentEventLog::default());
    // The BYOK panel reads the encrypted key store out of this
    // global. Same `Arc<SagalineStore>` is held by `AppEnv`; the
    // global is the convenience handle the view layer reaches
    // without a dependency on this binary crate.
    cx.set_global::<sagaline_ui::KeyStoreSlot>(sagaline_ui::KeyStoreSlot(
        env.store.clone(),
    ));
    info!(data_dir = %env.data_dir.display(), "AppEnv installed");
    Ok(env)
}

/// Spawn the agent's `OBSERVE → PLAN → ACT → REFLECT` loop against
/// `story_path` and route every event into [`AgentEventLog`].
///
/// On each event the forwarder calls `cx.refresh_windows()`. UI views
/// that want to re-render on new events should
/// `cx.observe_global::<AgentEventLog>()` and call `cx.notify()` from
/// the closure.
pub fn run_agent(env: Arc<AppEnv>, story_path: std::path::PathBuf, cx: &mut App) {
    let (tx, rx) = mpsc::unbounded_channel::<AgentEvent>();
    let mut sink = ChannelSink { tx };

    // Forwarder: drain the channel on the gpui async executor and
    // append to the global event log, then trigger a redraw of all
    // windows.
    cx.spawn(async move |async_cx: &mut AsyncApp| {
        let mut rx = rx;
        while let Some(event) = rx.recv().await {
            async_cx.update_global::<AgentEventLog, _>(|log, app| {
                log.events.push(event);
                app.refresh_windows();
            });
        }
    })
    .detach();

    // Agent loop: run on the gpui async executor. The agent owns
    // Arcs only; we move it into the task and never touch it from
    // the UI thread again.
    let env_for_task = env.clone();
    cx.spawn(async move |_async_cx: &mut AsyncApp| {
        let agent = env_for_task.build_agent(&story_path);
        let result = agent.run(&story_path, &mut sink).await;
        match result {
            Ok(outcome) => info!("agent finished: {outcome:?}"),
            Err(e) => error!("agent run failed: {e}"),
        }
    })
    .detach();
}
