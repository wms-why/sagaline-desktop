//! Empty-state onboarding panel (shown when no story is loaded).

use gpui_base::{StyledExt, v_flex};
use gpui_component::ActiveTheme;
use gpui_kit::*;
use gpui_kit::component::button::{Button, ButtonVariants as _};

use crate::actions::{CreateStory, OpenProjectSettings};
use crate::view::WorkspaceView;

pub(super) fn render_onboarding(_view: gpui_kit::Entity<WorkspaceView>, cx: &gpui_kit::App) -> gpui_kit::Div {
    let muted = cx.theme().muted_foreground;

    let mut col = v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_4()
        .p_8();

    col = col.child(div().text_3xl().font_bold().child("Sagaline"));
    col = col.child(
        div()
            .text_base()
            .text_color(muted)
            .child("Your AI agent for structured video production."),
    );
    col = col.child(
        div()
            .text_sm()
            .text_color(muted)
            .max_w(px(560.))
            .child(
                "Sagaline is an AI agent for structured video production. Create a story, set the goal, and the agent plans and animates scene by scene.",
            ),
    );

    col = col.child(
        Button::new("onboarding-create")
            .primary()
            .label("Create new story")
            .on_click(|_, _, cx| {
                cx.dispatch_action(&CreateStory);
            }),
    );

    col = col.child(
        Button::new("onboarding-settings")
            .ghost()
            .label("Project settings")
            .on_click(|_, _, cx| {
                cx.dispatch_action(&OpenProjectSettings);
            }),
    );

    col = col.child(
        div()
            .text_xs()
            .text_color(muted)
            .child("⌘ N new story   ⌘ O open story   ⌘ , project settings"),
    );

    col
}