//! Top-of-window banner row: app title, current story title, and
//! inline error / shortcut hint.

use gpui_base::{StyledExt, h_flex};
use gpui_kit::*;

use crate::state::WorkspaceState;

pub(super) fn render_banner(
    state: &WorkspaceState,
    dialog_open: bool,
    settings_open: bool,
    is_onboarding: bool,
) -> gpui_kit::Div {
    let title = match state.story() {
        Some(s) => format!("Sagaline — {}", s.title),
        None => "Sagaline".to_string(),
    };

    let mut row = h_flex()
        .w_full()
        .px_4()
        .py_2()
        .border_b_1()
        .child(div().text_lg().font_semibold().child(title));

    if let Some(err) = &state.last_error {
        row = row.child(div().ml_4().text_sm().child(format!("⚠ {err}")));
    }

    let any_dialog = dialog_open || settings_open;
    if is_onboarding && !any_dialog && state.last_error.is_none() {
        // intentionally empty — onboarding explains the actions
    } else if state.graph().is_none() && state.last_error.is_none() {
        row = row.child(
            div()
                .ml_4()
                .text_sm()
                .child("⌘ N new story   ⌘ O open story   ⌘ , project settings"),
        );
    }

    row
}