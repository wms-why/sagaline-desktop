//! Story Bible section: world memory surfaced as labelled cards.
//! Pure presentational — no state mutation.

use gpui_kit::component::*;
use gpui_kit::*;
#[allow(unused_imports)]
use sagaline_core::model::*;
use super::WorkspaceView;
impl WorkspaceView {
    pub fn render_story_bible(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let bible = match &self.state.workspace.bible {
            Some(b) => b,
            None => {
                return div()
                    .v_flex()
                    .gap_2()
                    .child(div().text_lg().font_semibold().child(t!("story_bible.empty_title")))
                    .child(div().text_sm().child(t!("story_bible.empty_body")));
            }
        };

        let block = |title: &str, body: &str| -> AnyElement {
            div()
                .v_flex()
                .gap_2()
                .p_4()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover)
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(cx.theme().accent)
                        .child(title.to_string()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(if body.is_empty() {
                            t!("story_bible.block_empty").to_string()
                        } else {
                            body.to_string()
                        }),
                )
                .into_any_element()
        };

        div()
            .v_flex()
            .gap_3()
            .child(
                div()
                    .text_xl()
                    .font_semibold()
                    .child(t!("story_bible.title")),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("story_bible.description")),
            )
            .child(block(t!("story_bible.block_world").as_ref(), &bible.world))
            .child(block(t!("story_bible.block_rules").as_ref(), &bible.rules))
            .child(block(t!("story_bible.block_timeline").as_ref(), &bible.timeline))
            .child(block(t!("story_bible.block_lore").as_ref(), &bible.lore))
    }
}