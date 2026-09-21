//! Thin Tokio <-> GPUI bridge.
//!
//! GPUI's task executor is *not* Tokio. `cx.spawn` (and
//! `BackgroundExecutor::spawn`) run their futures on GPUI's own
//! scheduler (`gpui-pre-scheduler`), backed by dedicated threads.
//! Calling Tokio APIs like `tokio::fs::read` or
//! `tokio::task::spawn_blocking` from such a future panics at
//! runtime because no Tokio reactor / blocking pool is in scope.
//!
//! [`TokioBridge`] is the seam between the two worlds, modelled on
//! Zed's `gpui_tokio`:
//!
//! 1. The caller hands a Tokio future (or a `FnOnce` closure) to
//!    [`TokioBridge::spawn`] / [`TokioBridge::spawn_blocking`].
//! 2. The bridge schedules it on the underlying Tokio runtime via
//!    [`tokio::runtime::Handle::spawn`], getting a `JoinHandle`.
//! 3. The returned [`gpui_kit::Task`] polls the `JoinHandle` on
//!    GPUI's background executor, so the result lands back in the
//!    gpui world.
//! 4. Dropping the GPUI task drops the inner future, which drops
//!    the `JoinHandle`, which aborts the Tokio task.
//!
//! The bridge itself is cheap to clone (the `Handle` is an `Arc`
//! bump and the `BackgroundExecutor` is `Clone`).

use std::future::Future;

use gpui_kit::{App, BackgroundExecutor, Global, Task};
use futures::Stream;
use tokio::runtime::Handle;
use tokio::task::JoinError;

/// Spawn a Tokio future from GPUI's side; await its result on a
/// GPUI background task; cancel it on drop.
#[derive(Clone)]
pub struct TokioBridge {
    handle: Handle,
    executor: BackgroundExecutor,
}

impl TokioBridge {
    /// Build a bridge from a Tokio runtime handle and a GPUI
    /// background executor (typically `cx.background_executor()`).
    pub fn new(handle: Handle, executor: BackgroundExecutor) -> Self {
        Self { handle, executor }
    }

    /// Convenience: build a bridge for the GPUI app `cx`.
    pub fn for_app(handle: Handle, cx: &App) -> Self {
        Self::new(handle, cx.background_executor().clone())
    }

    /// The underlying Tokio handle. Cheap to clone.
    pub fn handle(&self) -> &Handle {
        &self.handle
    }

    /// Spawn `fut` on the Tokio runtime. The returned `Task` is
    /// polled on the GPUI background executor; dropping it aborts
    /// the Tokio future.
    ///
    /// The body's `Future` runs entirely in the Tokio world — it
    /// does NOT receive a `&mut AsyncApp`. To update gpui state
    /// from inside, drain results via a channel (see
    /// `ChannelSink`) or wrap the body in another `cx.spawn` after
    /// awaiting this task.
    pub fn spawn<F, T>(&self, fut: F) -> Task<Result<T, JoinError>>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let join = self.handle.spawn(fut);
        // `JoinHandle::Drop` aborts the underlying task; simply
        // holding it inside the await is enough to wire GPUI
        // drop-semantics to Tokio cancellation.
        self.executor.spawn(async move { join.await })
    }

    /// Run `f` on the Tokio blocking thread pool. Same drop /
    /// cancellation contract as [`Self::spawn`].
    pub fn spawn_blocking<F, T>(&self, f: F) -> Task<Result<T, JoinError>>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let join = self.handle.spawn_blocking(f);
        self.executor.spawn(async move { join.await })
    }

    /// Forward every item of `stream` to `tx`. The stream is
    /// polled on this bridge's Tokio runtime (so a stream that
    /// calls `tokio::fs::*` from inside `poll_next` works). The
    /// consumer stops when the stream ends or the receiver is
    /// dropped; the returned `Task` resolves to the number of
    /// items forwarded.
    ///
    /// This is the seam `sagaline::run_agent` uses to bridge
    /// `Agent::run_stream` (Tokio-side) into a gpui UI that
    /// polls an mpsc receiver on its own executor.
    pub fn forward_to<T>(
        &self,
        mut stream: std::pin::Pin<Box<dyn Stream<Item = T> + Send>>,
        tx: tokio::sync::mpsc::UnboundedSender<T>,
    ) -> Task<Result<usize, JoinError>>
    where
        T: Send + 'static,
    {
        let join = self.handle.spawn(async move {
            use futures::StreamExt;
            let mut count = 0usize;
            while let Some(item) = stream.next().await {
                count += 1;
                // Receiver dropped -> stop. The Tokio task winds
                // down cleanly; the returned Task resolves to
                // `Ok(count)`.
                if tx.send(item).is_err() {
                    break;
                }
            }
            count
        });
        self.executor.spawn(async move { join.await })
    }

    /// Convenience wrapper around [`Self::forward_to`]: spawn
    /// the stream on Tokio, create the mpsc pair, and hand the
    /// receiver back to the caller. The caller is expected to
    /// poll the receiver from whichever executor they want (the
    /// gpui side, in `run_agent`'s case).
    pub fn forward_stream<T, S>(
        &self,
        stream: S,
    ) -> (tokio::sync::mpsc::UnboundedReceiver<T>, Task<Result<usize, JoinError>>)
    where
        S: Stream<Item = T> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let task = self.forward_to(Box::pin(stream), tx);
        (rx, task)
    }
}

/// gpui global wrapping a [`TokioBridge`]. The binary crate
/// (`sagaline`) installs one of these in `install_env` so that any
/// view layer (e.g. `sagaline-ui`) can `cx.global::<BridgeSlot>()`
/// without depending on the binary crate.
pub struct BridgeSlot(pub TokioBridge);

impl Clone for BridgeSlot {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl Global for BridgeSlot {}
