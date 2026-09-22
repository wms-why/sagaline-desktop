//! Right pane — Keys tab: BYOK provider key management.

use gpui_base::{StyledExt, h_flex, v_flex};
use gpui_component::ActiveTheme;
use gpui_kit::*;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;

use sagaline_bridge::BridgeSlot;

use crate::actions::DeleteProviderKey;
use crate::state::KeyStoreSlot;
use crate::view::WorkspaceView;

/// Render the BYOK key management panel.
///
/// Two sections, top to bottom:
///
/// 1. A toolbar with the heading and an "Add key" toggle. When
///    open, the toggle becomes "Cancel" and an inline form expands
///    below — provider, key id, masked secret, Save / Cancel.
/// 2. The existing-key list. Each row has a Delete button that
///    dispatches [`DeleteProviderKey`] (the handler in
///    `register_actions` reads `KeyStoreSlot` and deletes the row
///    via the bridge).
///
/// The form is inline (not a modal) so the user can see the keys
/// already stored while entering a new one. Validation errors
/// surface below the buttons instead of as a popup.
pub(super) fn render_keys_panel(view: &gpui_kit::Entity<WorkspaceView>, cx: &gpui_kit::App) -> gpui_kit::Div {
    let muted = cx.theme().muted_foreground;
    let mut col = v_flex().gap_2().p_3().size_full();

    // ---- toolbar ----------------------------------------------------

    let show_form = view.read(cx).show_add_key_form();
    col = col.child(
        h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .child(div().text_sm().font_semibold().child("Provider API keys"))
            .child(
                div()
                    .id("add-key-toggle")
                    .text_xs()
                    .on_click({
                        let view = view.clone();
                        move |_, _, cx| {
                            let _ = view.update(cx, |v, cx| {
                                if v.show_add_key_form() {
                                    v.close_add_key_form(cx);
                                } else {
                                    v.open_add_key_form(cx);
                                }
                            });
                        }
                    })
                    .child(if show_form { "× Cancel" } else { "+ Add key" }),
            ),
    );

    // ---- inline form (open only when show_form) ---------------------

    if show_form {
        col = col.child(render_add_key_form(view, cx, muted));
    }

    // ---- existing-key list -----------------------------------------

    let store = if cx.has_global::<KeyStoreSlot>() {
        cx.global::<KeyStoreSlot>().0.clone()
    } else {
        return col.child(div().text_xs().child("Key store not installed (binary mode)."));
    };

    let ids = match store.keys().list_ids() {
        Ok(ids) => ids,
        Err(e) => {
            return col.child(
                render_delete_error_footer(view, cx)
                    .unwrap_or_else(|| div().text_xs().child(format!("failed to list keys: {e}"))),
            );
        }
    };

    if ids.is_empty() {
        col = col.child(div().text_xs().child("No keys stored yet."));
        if let Some(err) = render_delete_error_footer(view, cx) {
            col = col.child(err);
        }
        return col;
    }

    for id in &ids {
        let row = h_flex()
            .w_full()
            .justify_between()
            .gap_2()
            .child(div().text_sm().font_family("monospace").child(id.to_string()))
            .child(
                div()
                    .id(format!("del-{}", id))
                    .text_xs()
                    .on_click({
                        let id = id.clone();
                        move |_, _, cx| {
                            cx.dispatch_action(&DeleteProviderKey {
                                provider: id.provider.clone(),
                                key_id: id.key_id.clone(),
                            });
                        }
                    })
                    .child("Delete"),
            );
        col = col.child(row);
    }

    if let Some(err) = render_delete_error_footer(view, cx) {
        col = col.child(err);
    }
    col
}

/// Render the BYOK delete-failure footer as a single-row, danger-
/// coloured message. Returns `None` when there is no pending delete
/// error so the caller can omit the row entirely from the column.
/// Lives outside the add-key form (which has its own error surface)
/// because the user usually closes that form before clicking Delete
/// on a stored row.
fn render_delete_error_footer(
    view: &gpui_kit::Entity<WorkspaceView>,
    cx: &gpui_kit::App,
) -> Option<gpui_kit::Div> {
    let msg = view.read(cx).delete_key_error_msg()?.to_string();
    Some(div().text_xs().text_color(cx.theme().danger).child(msg))
}

/// Render the paste-plaintext add-key form. The three `InputState`
/// entities are owned by the [`WorkspaceView`]; this function just
/// lays them out and wires the Save / Cancel click handlers.
fn render_add_key_form(
    view: &gpui_kit::Entity<WorkspaceView>,
    cx: &gpui_kit::App,
    muted: Hsla,
) -> gpui_kit::Div {
    let view_read = view.read(cx);
    let (Some(provider), Some(key_id), Some(secret)) = (
        view_read.add_provider_input().cloned(),
        view_read.add_key_id_input().cloned(),
        view_read.add_secret_input().cloned(),
    ) else {
        // Inputs aren't ready yet — the next render frame will retry.
        return div();
    };
    let error_msg = view_read.add_key_error_msg().map(str::to_string);
    // `view_read` is a `&WorkspaceView` borrowing `cx`; the borrow
    // ends naturally at the end of this scope. The closure below
    // captures `view` (an owned `Entity<WorkspaceView>`) — not the
    // reference — so there's no conflict with the `cx` borrow.

    let mut form = v_flex()
        .gap_2()
        .p_2()
        .border_1()
        .rounded_md()
        .child(
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child("Provider (lowercase, e.g. openai)"),
                )
                .child(Input::new(&provider)),
        )
        .child(
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child("Key ID (e.g. default, prod, org-xyz)"),
                )
                .child(Input::new(&key_id)),
        )
        .child(
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child("API key — pasted once, age-encrypted on disk"),
                )
                .child(Input::new(&secret)),
        );

    // ---- action row ------------------------------------------------

    let save_btn = Button::new("add-key-save")
        .primary()
        .label("Save")
        .on_click({
            let view = view.clone();
            let provider = provider.clone();
            let key_id = key_id.clone();
            let secret = secret.clone();
            move |_, _window, cx| {
                submit_add_key(&view, &provider, &key_id, &secret, cx);
            }
        });

    let cancel_btn = Button::new("add-key-cancel")
        .ghost()
        .label("Cancel")
        .on_click({
            let view = view.clone();
            move |_, _, cx| {
                let _ = view.update(cx, |v, cx| {
                    v.close_add_key_form(cx);
                });
            }
        });

    form = form.child(
        h_flex()
            .gap_2()
            .justify_end()
            .child(cancel_btn)
            .child(save_btn),
    );

    if let Some(msg) = error_msg {
        form = form.child(div().text_xs().text_color(cx.theme().danger).child(msg));
    }

    form
}

/// Read the add-key form's three `InputState` entities on the
/// foreground thread, validate, then route the actual `KeyRepo::put`
/// (which does the age encryption + SQLite write) through the
/// bridge's blocking pool so it doesn't panic inside the gpui task.
///
/// On success the form is closed and the inputs are dropped
/// (freeing the plaintext from GPUI memory). On failure the form
/// stays open with an error message.
fn submit_add_key(
    view: &gpui_kit::Entity<WorkspaceView>,
    provider_input: &gpui_kit::Entity<gpui_kit::component::input::InputState>,
    key_id_input: &gpui_kit::Entity<gpui_kit::component::input::InputState>,
    secret_input: &gpui_kit::Entity<gpui_kit::component::input::InputState>,
    cx: &mut gpui_kit::App,
) {
    let provider = provider_input.read(cx).value().to_string();
    let key_id = key_id_input.read(cx).value().to_string();
    let plaintext = secret_input.read(cx).value().to_string();

    if let Err(msg) = validate_add_key(&provider, &key_id, &plaintext) {
        let _ = view.update(cx, |v, cx| {
            v.set_add_key_error(Some(msg), cx);
        });
        return;
    }

    let Some(store) = cx.try_global::<KeyStoreSlot>().map(|s| s.0.clone()) else {
        let _ = view.update(cx, |v, cx| {
            v.set_add_key_error(Some("Key store not available".into()), cx);
        });
        return;
    };
    let bridge = cx.global::<BridgeSlot>().0.clone();

    let view_for_ok = view.clone();
    let view_for_err = view.clone();
    cx.spawn(async move |async_cx| {
        // `tokio::task::spawn_blocking` and `rusqlite` both need a
        // non-gpui runtime; the bridge is the seam. Drop the gpui
        // task -> drop the join handle -> cancel the put.
        let result = bridge
            .spawn_blocking(move || -> Result<(), String> {
                let id = sagaline_store::ProviderKeyId::new(provider, key_id)
                    .map_err(|e| format!("invalid id: {e}"))?;
                let secret = secrecy::SecretString::new(plaintext.into_boxed_str());
                store
                    .keys()
                    .put(&id, &secret)
                    .map_err(|e| format!("store failed: {e}"))?;
                Ok(())
            })
            .await;

        match result {
            Ok(Ok(())) => {
                let _ = view_for_ok.update(async_cx, |v, cx| {
                    v.close_add_key_form(cx);
                });
            }
            Ok(Err(msg)) => {
                let _ = view_for_err.update(async_cx, |v, cx| {
                    v.set_add_key_error(Some(msg), cx);
                });
            }
            Err(join_err) => {
                let _ = view_for_err.update(async_cx, |v, cx| {
                    v.set_add_key_error(Some(format!("bridge join error: {join_err}")), cx);
                });
            }
        }
    })
    .detach();
}

/// Validate the add-key form's three fields. Mirrors the
/// `validate_create_story` shape: caller gets a sync error message
/// before any I/O is dispatched, so the user sees an immediate
/// "provider is required" instead of a silent close-and-reopen.
pub(super) fn validate_add_key(provider: &str, key_id: &str, plaintext: &str) -> Result<(), String> {
    let provider = provider.trim();
    let key_id = key_id.trim();
    let plaintext = plaintext.trim();
    if provider.is_empty() {
        return Err("provider is required".into());
    }
    if key_id.is_empty() {
        return Err("key id is required".into());
    }
    if plaintext.is_empty() {
        return Err("API key is required".into());
    }
    // The id parser validates the charset / length — try it here
    // so the user sees a clear "invalid id: …" instead of a sqlite
    // error from the store layer.
    sagaline_store::ProviderKeyId::new(provider, key_id)
        .map_err(|e| format!("invalid id: {e}"))?;
    Ok(())
}

/// Map a [`sagaline_store::StoreError`] from a delete attempt to
/// the user-facing footer message rendered under the BYOK key list.
/// Kept pure so the message wording is testable without a gpui
/// runtime; the handler passes the result through directly.
pub(super) fn format_delete_error(e: &sagaline_store::StoreError) -> String {
    use sagaline_store::StoreError;
    match e {
        StoreError::NotFound { .. } => "key not found — already deleted?".into(),
        StoreError::EncryptDecrypt(_) => {
            "delete failed: stored key cannot be decrypted — the data dir may be corrupted".into()
        }
        StoreError::Other(_) => format!("delete failed: {e}"),
        _ => format!("delete failed: {e}"),
    }
}