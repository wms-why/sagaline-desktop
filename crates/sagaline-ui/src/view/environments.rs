//! Environments section: card per location with a "used in" cross-ref
//! list computed by scanning every chapter/scene.

use gpui_kit::component::*;
use gpui_kit::*;
#[allow(unused_imports)]
use sagaline_core::model::*;
use super::WorkspaceView;
use super::format::{format_chapter_scene_ref, render_reference_avatars};

impl WorkspaceView {
    pub fn render_environments(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div()
            .v_flex()
            .gap_3()
            .child(div().text_xl().font_semibold().child(t!("environments.title")))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("environments.description")),
            );

        if self.state.workspace.environments.is_empty() {
            return root.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("environments.empty")),
            );
        }

        for env in &self.state.workspace.environments {
            // Cross-reference: which scenes use this environment?
            let mut used_in = Vec::new();
            for ch in &self.state.workspace.chapters {
                for sc in &ch.scenes {
                    if sc.scene.environment_id.as_ref() == Some(&env.environment.id) {
                        used_in.push(format_chapter_scene_ref(
                            ch.chapter.chapter_number,
                            sc.scene.scene_number,
                        ));
                    }
                }
            }

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
                            .child(env.environment.name.clone()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(if env.environment.description.is_empty() {
                                t!("environments.no_description").to_string()
                            } else {
                                env.environment.description.clone()
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!(
                                "environments.lighting_time",
                                lighting = if env.environment.lighting.is_empty() {
                                    t!("environments.placeholder_dash").into_owned()
                                } else {
                                    env.environment.lighting.clone()
                                },
                                time = if env.environment.time_of_day.is_empty() {
                                    t!("environments.placeholder_dash").into_owned()
                                } else {
                                    env.environment.time_of_day.clone()
                                }
                            ).into_owned()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().accent)
                            .child(t!(
                                "environments.used_in",
                                refs = if used_in.is_empty() {
                                    t!("environments.used_in_empty").into_owned()
                                } else {
                                    used_in.join(", ")
                                }
                            ).into_owned()),
                    )
                    .child(render_reference_avatars(&env.references, cx)),
            );
        }
        root
    }
}