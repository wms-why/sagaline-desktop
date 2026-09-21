//! New-story modal: title + project location picker, dispatched
//! from the `CreateStory` action.

use gpui_base::{StyledExt, v_flex};
use gpui_component::ActiveTheme;
use gpui_component::WindowExt;
use gpui_kit::*;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};

use sagaline_bridge::BridgeSlot;

use crate::state::{StoryService, StoryServiceSlot};
use crate::view::WorkspaceView;

/// Validate the inputs from the new-story dialog. The Create button
/// in the dialog is always rendered enabled, so this runs in the
/// click handler before the dialog closes. Returning `Err(msg)`
/// keeps the dialog open with the error visible.
pub(super) fn validate_create_story(
    project_location_display: Option<&str>,
    title: &str,
) -> Result<(), String> {
    if project_location_display.is_none() {
        return Err("Pick a project location first (⌘ ,).".into());
    }
    if title.trim().is_empty() {
        return Err("Story title must not be empty.".into());
    }
    Ok(())
}

/// Open the new-story modal via `Window::open_dialog`. Idempotent.
#[allow(clippy::too_many_arguments)]
pub(super) fn maybe_open_new_story_dialog(
    view: gpui_kit::Entity<WorkspaceView>,
    show_dialog: bool,
    dialog_already_open: bool,
    title_input: gpui_kit::Entity<InputState>,
    project_location_display: Option<String>,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    if !show_dialog {
        return;
    }
    if dialog_already_open {
        return;
    }
    if window.has_active_dialog(cx) {
        return;
    }

    let title_for_content = title_input.clone();
    let view_for_cancel = view.clone();
    let title_for_cancel = title_input.clone();
    let view_for_create = view.clone();
    let title_for_create = title_input.clone();
    let view_for_marker = view.clone();

    let view_for_error = view.clone();

    window.open_dialog(cx, move |dialog, _window, _cx| {
        // The dialog builder is `Fn + 'static` — clone per consumer.
        // The content closure needs the project location for its
        // hint text; the Create click handler needs it for
        // validation.
        let project_location_for_content = project_location_display.clone();
        let project_location_for_create = project_location_display.clone();
        let view_for_content_err = view_for_error.clone();
        let title_for_content = title_for_content.clone();
        dialog
            .title("Create a new story")
            .content(move |content, _window, cx| {
                // Read the live error from the view each render
                // instead of freezing a snapshot at dialog-open
                // time. Validation errors set after the dialog is
                // already up would never show up.
                let error_msg: Option<String> = view_for_content_err
                    .read_with(&*cx, |v, _app| {
                        v.new_story_error_msg().map(str::to_string)
                    });
                let mut content = content.child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .child("Story title"),
                        )
                        .child(Input::new(&title_for_content)),
                );
                match &project_location_for_content {
                    Some(loc) => {
                        content = content.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("A new project will be created in {loc}.")),
                        );
                    }
                    None => {
                        content = content.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    "Pick a project location to keep new stories. Press ⌘ , to set one.",
                                ),
                        );
                    }
                }
                if let Some(msg) = error_msg {
                    content = content.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().danger)
                            .child(msg),
                    );
                }
                content
            })
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("new-story-cancel")
                            .ghost()
                            .label("Cancel")
                            .on_click({
                                let view_for_cancel = view_for_cancel.clone();
                                let title_for_cancel = title_for_cancel.clone();
                                move |_, window, cx| {
                                    let _ = view_for_cancel.update(cx, |v, cx| {
                                        v.close_new_story_dialog(cx);
                                    });
                                    let _ = title_for_cancel.read(cx);
                                    window.close_dialog(cx);
                                }
                            }),
                    )
                    .child(
                        Button::new("new-story-create")
                            .primary()
                            .label("Create")
                            .on_click({
                                let view_for_create = view_for_create.clone();
                                let title_for_create = title_for_create.clone();
                                let project_location_for_click = project_location_for_create.clone();
                                move |_, window, cx| {
                                    let title = title_for_create.read(cx).value().to_string();
                                    let view_for_async = view_for_create.clone();
                                    let view_for_async_err = view_for_create.clone();
                                    // Capture the StoryService on
                                    // the foreground thread; the
                                    // async task moves it.
                                    let svc: Box<dyn StoryService> =
                                        match cx
                                            .try_global::<StoryServiceSlot>()
                                            .cloned()
                                        {
                                            Some(slot) => slot.0,
                                            None => {
                                                let _ = view_for_create.update(cx, |v, cx| {
                                                    v.set_new_story_error(
                                                        Some("Story service not available".into()),
                                                        cx,
                                                    );
                                                });
                                                window.close_dialog(cx);
                                                return;
                                            }
                                        };
                                    // Validate the dialog inputs
                                    // synchronously so the user
                                    // gets immediate feedback instead
                                    // of a silent close + reopen.
                                    if let Err(msg) = validate_create_story(
                                        project_location_for_click.as_deref(),
                                        &title,
                                    ) {
                                        let _ = view_for_create.update(cx, |v, cx| {
                                            v.set_new_story_error(Some(msg), cx);
                                        });
                                        return;
                                    }
                                    window.close_dialog(cx);
                                    let _ = view_for_create.update(cx, |v, cx| {
                                        v.set_new_story_error(None, cx);
                                    });
                                    // Route through the Tokio bridge so
                                    // the sync `StoryService::create_story`
                                    // (which touches the filesystem) runs
                                    // on the agent's blocking pool instead
                                    // of panicking inside this gpui task.
                                    let bridge = cx
                                        .global::<BridgeSlot>()
                                        .0
                                        .clone();
                                    cx.spawn(async move |_async_cx| {
                                        let res = bridge
                                            .spawn_blocking(move || {
                                                svc.create_story(&title)
                                            })
                                            .await;
                                        match res {
                                            Ok(Ok(story)) => {
                                                let _ = view_for_async.update(
                                                    _async_cx,
                                                    |v, cx| {
                                                        v.close_new_story_dialog(cx);
                                                        v.open_story(story, cx);
                                                    },
                                                );
                                            }
                                            Ok(Err(msg)) => {
                                                let _ = view_for_async_err.update(
                                                    _async_cx,
                                                    |v, cx| {
                                                        v.set_new_story_error(Some(msg), cx);
                                                    },
                                                );
                                            }
                                            Err(join_err) => {
                                                let msg = format!(
                                                    "create failed: {join_err}"
                                                );
                                                let _ = view_for_async_err.update(
                                                    _async_cx,
                                                    |v, cx| {
                                                        v.set_new_story_error(Some(msg), cx);
                                                    },
                                                );
                                            }
                                        }
                                    })
                                    .detach();
                                }
                            }),
                    ),
            )
            .on_cancel({
                let view_for_cancel = view_for_cancel.clone();
                let title_for_cancel = title_for_cancel.clone();
                move |_, window, cx| {
                    let _ = view_for_cancel.update(cx, |v, cx| {
                        v.close_new_story_dialog(cx);
                    });
                    let _ = title_for_cancel.read(cx);
                    window.close_dialog(cx);
                    true
                }
            })
            .on_close({
                let view_for_close = view_for_cancel.clone();
                let view_for_marker = view_for_marker.clone();
                move |_, _window, cx| {
                    let _ = view_for_close.update(cx, |v, cx| {
                        v.close_new_story_dialog(cx);
                    });
                    let _ = view_for_marker.update(cx, |v, cx| {
                        v.mark_new_story_dialog_opened(cx);
                    });
                }
            })
    });
}