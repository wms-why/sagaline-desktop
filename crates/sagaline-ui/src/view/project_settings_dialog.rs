//! Project-settings modal: pick the directory new stories are
//! created under. Dispatched from the `OpenProjectSettings` action.

use std::path::PathBuf;

use gpui_base::StyledExt;
use gpui_component::ActiveTheme;
use gpui_component::WindowExt;
use gpui_kit::*;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;

use sagaline_core::ProjectLocation;

use crate::state::StoryServiceSlot;
use crate::view::WorkspaceView;

/// Open the project-settings modal. Idempotent.
pub(super) fn maybe_open_project_settings_dialog(
    view: gpui_kit::Entity<WorkspaceView>,
    show_dialog: bool,
    dialog_already_open: bool,
    location_display: Option<String>,
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

    let view_for_close = view.clone();
    let view_for_cancel = view.clone();
    let view_for_marker = view.clone();

    window.open_dialog(cx, move |dialog, _window, _cx| {
        let location_display = location_display.clone();
        dialog
            .title("Project settings")
            .content(move |content, _window, cx| {
                let mut content = content.child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .child("Where to keep new stories"),
                );
                match &location_display {
                    Some(loc) => {
                        content = content.child(
                            div()
                                .text_sm()
                                .font_family("monospace")
                                .text_color(cx.theme().muted_foreground)
                                .child(loc.clone()),
                        );
                    }
                    None => {
                        content = content.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("Not set yet. Choose a folder to keep new stories."),
                        );
                    }
                }
                content
            })
            .footer({
                let view_for_close = view_for_close.clone();
                DialogFooter::new()
                    .child(
                        Button::new("project-settings-choose")
                            .primary()
                            .label("Choose…")
                            .on_click({
                                let view_for_close = view_for_close.clone();
                                move |_, _window, cx| {
                                    let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
                                        files: false,
                                        directories: true,
                                        multiple: false,
                                        prompt: Some(
                                            "Where would you like to keep new stories?".into(),
                                        ),
                                    });
                                    let view_for_async = view_for_close.clone();
                                    cx.spawn(async move |async_cx| {
                                        let result = receiver.await;
                                        let paths = match result {
                                            Ok(Ok(Some(paths))) => paths,
                                            _ => return,
                                        };
                                        if let Some(path) = paths.into_iter().next() {
                                            let _ = view_for_async.update(
                                                async_cx,
                                                |v, cx| {
                                                    apply_project_location(v, path, cx);
                                                },
                                            );
                                        }
                                    })
                                    .detach();
                                }
                            }),
                    )
                    .child(
                        Button::new("project-settings-cancel")
                            .ghost()
                            .label("Done")
                            .on_click({
                                let view_for_cancel = view_for_cancel.clone();
                                move |_, window, cx| {
                                    let _ = view_for_cancel.update(cx, |v, cx| {
                                        v.close_project_settings_dialog(cx);
                                    });
                                    window.close_dialog(cx);
                                }
                            }),
                    )
            })
            .on_cancel({
                let view_for_cancel = view_for_cancel.clone();
                move |_, window, cx| {
                    let _ = view_for_cancel.update(cx, |v, cx| {
                        v.close_project_settings_dialog(cx);
                    });
                    window.close_dialog(cx);
                    true
                }
            })
            .on_close({
                let view_for_close = view_for_close.clone();
                let view_for_marker = view_for_marker.clone();
                move |_, _window, cx| {
                    let _ = view_for_close.update(cx, |v, cx| {
                        v.close_project_settings_dialog(cx);
                    });
                    let _ = view_for_marker.update(cx, |v, cx| {
                        v.mark_project_settings_dialog_opened(cx);
                    });
                }
            })
    });
}

pub(super) fn apply_project_location(
    view: &mut WorkspaceView,
    path: PathBuf,
    cx: &mut gpui_kit::Context<WorkspaceView>,
) {
    if let Some(slot) = cx.try_global::<StoryServiceSlot>().cloned() {
        let svc = slot.0;
        if let Err(e) = svc.set_project_location(path.clone()) {
            view.state_mut().last_error = Some(format!("set project location failed: {e}"));
            cx.notify();
            return;
        }
        view.state_mut().set_recent_stories(svc.recent_stories());
    }
    view.state_mut()
        .set_project_location(ProjectLocation::new(path));
    cx.notify();
}