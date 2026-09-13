//! Props section: card per recurring object, with a reference-avatar row
//! sourced from the shared helper in `super::format`.

use gpui_kit::component::*;
use gpui_kit::*;
#[allow(unused_imports)]
use sagaline_core::model::*;
use super::WorkspaceView;
use super::format::render_reference_avatars;

impl WorkspaceView {
    pub fn render_props(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div()
            .v_flex()
            .gap_3()
            .child(div().text_xl().font_semibold().child(t!("props.title")))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("props.description")),
            );

        if self.state.workspace.props.is_empty() {
            return root.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("props.empty")),
            );
        }

        for p in &self.state.workspace.props {
            root = root.child(
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
                            .text_lg()
                            .font_semibold()
                            .child(p.prop.name.clone()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(if p.prop.description.is_empty() {
                                t!("props.no_description").to_string()
                            } else {
                                p.prop.description.clone()
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if p.prop.appearance.is_empty() {
                                t!("props.no_appearance").to_string()
                            } else {
                                t!("props.appearance", value = p.prop.appearance.clone()).into_owned()
                            }),
                    )
                    .child(render_reference_avatars(&p.references, cx)),
            );
        }
        root
    }
}