//! Action wiring: binds every UI-side action to a handler that
//! operates on the `WorkspaceView`.

use std::path::PathBuf;
use std::sync::Arc;

use sagaline_bridge::BridgeSlot;
use sagaline_core::{Story, StoryHandle};

use crate::activity::commit_policy_from_index;
use crate::actions::{
    ApproveProposal, DeleteProviderKey, ImportProviderKeyFromFile, OpenProjectSettings, OpenStory,
    RefreshPendingProposals, RejectProposal, ReloadStory, SetCommitPolicy, SwitchTab,
};
use crate::state::{KeyStoreSlot, ProposalServiceSlot};
use crate::view::{RightTab, WorkspaceView};

pub fn register_actions(view: gpui_kit::Entity<WorkspaceView>, cx: &mut gpui_kit::App) {
    let view_for_open = view.clone();
    cx.on_action::<OpenStory>(move |_, cx| {
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open a story".into()),
        });
        let view_for_async = view_for_open.downgrade();
        cx.spawn(async move |cx| {
            let result = receiver.await;
            let paths = match result {
                Ok(Ok(Some(paths))) => paths,
                _ => return,
            };
            if let Some(path) = paths.into_iter().next() {
                let _ = view_for_async.update(cx, |view, cx| {
                    open_story_at_path(view, path, cx);
                });
            }
        })
        .detach();
    });

    let view_for_create = view.clone();
    cx.on_action::<crate::actions::CreateStory>(move |_, cx| {
        let _ = view_for_create.update(cx, |view, cx| {
            view.open_new_story_dialog(cx);
        });
    });

    let view_for_reload = view.clone();
    cx.on_action::<ReloadStory>(move |_, cx| {
        let _ = view_for_reload.update(cx, |view, cx| {
            view.reload(cx);
        });
    });

    let view_for_tab = view.clone();
    cx.on_action::<SwitchTab>(move |action, cx| {
        let target = RightTab::from_index(action.tab);
        let _ = view_for_tab.update(cx, |view, cx| {
            view.set_right_tab(target, cx);
        });
        // When the user switches to the Activity tab, refresh
        // the pending-proposals queue so the cards reflect the
        // latest agent output. The picker header's Refresh
        // button dispatches the same action manually.
        if target == RightTab::Activity {
            cx.dispatch_action(&RefreshPendingProposals);
        }
    });

    let view_for_settings = view.clone();
    cx.on_action::<OpenProjectSettings>(move |_, cx| {
        let _ = view_for_settings.update(cx, |view, cx| {
            view.open_project_settings_dialog(cx);
        });
    });

    let view_for_delete = view.clone();
    cx.on_action::<DeleteProviderKey>(move |action, cx| {
        let Some(store) = cx.try_global::<KeyStoreSlot>().map(|s| s.0.clone()) else {
            return;
        };
        let provider = action.provider.clone();
        let key_id = action.key_id.clone();
        let view_for_refresh = view_for_delete.clone();
        cx.spawn(async move |cx| {
            let result = match sagaline_store::ProviderKeyId::new(provider, key_id) {
                Ok(id) => store.keys().delete(&id).map(|_| ()),
                Err(e) => Err(sagaline_store::StoreError::Other(e.to_string())),
            };
            if let Err(e) = result {
                tracing::warn!(error = %e, "delete provider key failed");
            }
            let _ = view_for_refresh.update(cx, |_view, cx| cx.notify());
        })
        .detach();
    });
    let view_for_import = view.clone();
    cx.on_action::<ImportProviderKeyFromFile>(move |action, cx| {
        let Some(store) = cx.try_global::<KeyStoreSlot>().map(|s| s.0.clone()) else {
            return;
        };
        let provider = action.provider.clone();
        let key_id = action.key_id.clone();
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select file containing the API key plaintext".into()),
        });
        let view_for_refresh = view_for_import.clone();
        let store_for_task = store.clone();
        // Read the bridge out of the App *before* entering the
        // spawn closure — inside, `cx` is `&mut AsyncApp`, which
        // does not implement `ReadGlobal`.
        let bridge = cx.global::<BridgeSlot>().0.clone();
        cx.spawn(async move |cx| {
            let result = receiver.await;
            let paths = match result {
                Ok(Ok(Some(paths))) => paths,
                _ => return,
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            // `tokio::fs::read` requires a Tokio runtime; route
            // through the bridge so the read runs on the agent's
            // blocking pool instead of panicking inside the gpui
            // task. Drop the gpui task -> drop the join handle ->
            // cancel the read.
            let path_for_read = path.clone();
            let bytes = match bridge
                .spawn_blocking(move || std::fs::read(&path_for_read))
                .await
            {
                Ok(Ok(b)) => b,
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, ?path, "import key: read failed");
                    return;
                }
                Err(join_err) => {
                    tracing::warn!(error = %join_err, ?path, "import key: bridge task panicked");
                    return;
                }
            };
            let plaintext = match String::from_utf8(bytes) {
                Ok(s) => s.trim().to_string(),
                Err(_) => {
                    tracing::warn!(?path, "import key: file is not UTF-8");
                    return;
                }
            };
            let id = match sagaline_store::ProviderKeyId::new(provider, key_id) {
                Ok(id) => id,
                Err(e) => {
                    tracing::warn!(error = %e, "import key: invalid id");
                    return;
                }
            };
            let secret = secrecy::SecretString::new(plaintext.into_boxed_str());
            if let Err(e) = store_for_task.keys().put(&id, &secret) {
                tracing::warn!(error = %e, "import key: store failed");
                return;
            }
            let _ = view_for_refresh.update(cx, |_view, cx| cx.notify());
        })
        .detach();
    });

    // ---- proposal picker -------------------------------------------------
    //
    // Every handler below:
    //   1. grabs `ProposalServiceSlot` from the gpui globals (set by
    //      `sagaline::install_env`),
    //   2. routes the actual work through `bridge.spawn_blocking` so
    //      the sync trait methods don't block the gpui executor,
    //   3. on completion pushes the result back into
    //      `WorkspaceState` + `cx.notify()`.
    //
    // The bridge is captured *before* entering the async closure —
    // `cx` becomes `&mut AsyncApp` inside `spawn`, which does not
    // implement `ReadGlobal`.

    let view_for_set_policy = view.clone();
    cx.on_action::<SetCommitPolicy>(move |action, cx| {
        let policy = commit_policy_from_index(action.policy);
        let Some(svc) = cx.try_global::<ProposalServiceSlot>().cloned() else {
            return;
        };
        let bridge = cx.global::<BridgeSlot>().0.clone();
        let view_for_async = view_for_set_policy.clone();
        cx.spawn(async move |async_cx| {
            // The service call is sync but cheap (single
            // RwLock write + env var set). Routing through
            // `spawn_blocking` keeps the gpui executor off the
            // lock for fairness with the agent's own writes.
            let svc_for_task = svc.0.clone_service();
            let _ = bridge
                .spawn_blocking(move || {
                    svc_for_task.set_commit_policy(policy);
                })
                .await;
            let _ = view_for_async.update(async_cx, |view, cx| {
                view.state_mut().set_commit_policy(policy);
                cx.notify();
            });
        })
        .detach();
    });

    let view_for_refresh_proposals = view.clone();
    cx.on_action::<RefreshPendingProposals>(move |_, cx| {
        let Some(svc) = cx.try_global::<ProposalServiceSlot>().cloned() else {
            return;
        };
        let bridge = cx.global::<BridgeSlot>().0.clone();
        let view_for_async = view_for_refresh_proposals.clone();
        cx.spawn(async move |async_cx| {
            let svc_for_task = svc.0.clone_service();
            let result = bridge
                .spawn_blocking(move || {
                    svc_for_task.list_pending_proposals(None)
                })
                .await;
            let Ok(Ok(proposals)) = result else {
                let err = match result {
                    Ok(Err(e)) => e,
                    Err(join_err) => format!("refresh join error: {join_err}"),
                    _ => String::new(),
                };
                if !err.is_empty() {
                    let _ = view_for_async.update(async_cx, |view, cx| {
                        view.state_mut().proposal_error = Some(err);
                        cx.notify();
                    });
                }
                return;
            };
            // Pull each proposal's queued actions in the same
            // worker task so we don't fan out N tasks for what is
            // typically a small queue.
            let svc_for_actions = svc.0.clone_service();
            let proposals_for_actions = proposals.clone();
            let actions_result = bridge
                .spawn_blocking(move || {
                    let mut out = std::collections::BTreeMap::new();
                    for p in &proposals_for_actions {
                        match svc_for_actions.list_proposal_actions(&p.id) {
                            Ok(a) => {
                                out.insert(p.id.clone(), a);
                            }
                            Err(_) => {
                                out.insert(p.id.clone(), Vec::new());
                            }
                        }
                    }
                    out
                })
                .await;
            let actions = match actions_result {
                Ok(map) => map,
                Err(_) => std::collections::BTreeMap::new(),
            };
            let _ = view_for_async.update(async_cx, |view, cx| {
                view.state_mut().proposal_error = None;
                view.state_mut()
                    .set_pending_proposals(proposals, actions);
                cx.notify();
            });
        })
        .detach();
    });

    let view_for_approve = view.clone();
    cx.on_action::<ApproveProposal>(move |action, cx| {
        let Some(svc) = cx.try_global::<ProposalServiceSlot>().cloned() else {
            return;
        };
        let proposal_id = action.proposal_id.clone();
        let bridge = cx.global::<BridgeSlot>().0.clone();
        let view_for_async = view_for_approve.clone();
        cx.spawn(async move |async_cx| {
            let svc_for_task = svc.0.clone_service();
            let pid = proposal_id.clone();
            let result = bridge
                .spawn_blocking(move || svc_for_task.approve_proposal(&pid, "user"))
                .await;
            match result {
                Ok(Ok(_n)) => {
                    let _ = view_for_async.update(async_cx, |view, cx| {
                        view.state_mut().proposal_error = None;
                        // Refresh the queue after a successful approve.
                        cx.dispatch_action(&RefreshPendingProposals);
                    });
                }
                Ok(Err(e)) => {
                    let msg = e.clone();
                    let _ = view_for_async.update(async_cx, |view, cx| {
                        view.state_mut().proposal_error = Some(msg);
                        cx.notify();
                    });
                }
                Err(join_err) => {
                    let msg = format!("approve join error: {join_err}");
                    let _ = view_for_async.update(async_cx, |view, cx| {
                        view.state_mut().proposal_error = Some(msg);
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    });

    let view_for_reject = view.clone();
    cx.on_action::<RejectProposal>(move |action, cx| {
        let Some(svc) = cx.try_global::<ProposalServiceSlot>().cloned() else {
            return;
        };
        let proposal_id = action.proposal_id.clone();
        let bridge = cx.global::<BridgeSlot>().0.clone();
        let view_for_async = view_for_reject.clone();
        cx.spawn(async move |async_cx| {
            let svc_for_task = svc.0.clone_service();
            let pid = proposal_id.clone();
            let result = bridge
                .spawn_blocking(move || svc_for_task.reject_proposal(&pid, "user"))
                .await;
            match result {
                Ok(Ok(())) => {
                    let _ = view_for_async.update(async_cx, |view, cx| {
                        view.state_mut().proposal_error = None;
                        cx.dispatch_action(&RefreshPendingProposals);
                    });
                }
                Ok(Err(e)) => {
                    let msg = e.clone();
                    let _ = view_for_async.update(async_cx, |view, cx| {
                        view.state_mut().proposal_error = Some(msg);
                        cx.notify();
                    });
                }
                Err(join_err) => {
                    let msg = format!("reject join error: {join_err}");
                    let _ = view_for_async.update(async_cx, |view, cx| {
                        view.state_mut().proposal_error = Some(msg);
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    });
}

pub(super) fn open_story_at_path(
    view: &mut WorkspaceView,
    path: PathBuf,
    cx: &mut gpui_kit::Context<WorkspaceView>,
) {
    let root = match sagaline_core::StoryRoot::new(path.clone()) {
        Ok(r) => r,
        Err(e) => {
            view.state_mut().last_error = Some(format!(
                "failed to open {}: {e}",
                path.display()
            ));
            cx.notify();
            return;
        }
    };
    let now = sagaline_core::story_root::rfc3339_now_public();
    let story = Arc::new(Story::new_public(
        StoryHandle::new(path),
        root.story_id().clone(),
        root.title().to_string(),
        now.clone(),
        now,
    ));
    view.open_story(story, cx);
}