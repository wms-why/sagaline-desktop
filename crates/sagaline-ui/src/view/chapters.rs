//! Chapters section: two-pane layout (chapter list on the left,
//! selected chapter's scenes + shots on the right). Status badges use the
//! shared `chapter_status_tag` helper from `super::format`.

use gpui_kit::component::Size;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::*;
use gpui_kit::*;
#[allow(unused_imports)]
use sagaline_core::model::*;
use super::WorkspaceView;
use super::format::{chapter_status_tag, chapter_status_tooltip, format_char_ref_label};
impl WorkspaceView {
    pub fn render_chapters(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div()
            .v_flex()
            .gap_3()
            .child(div().text_xl().font_semibold().child(t!("chapters.title")))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("chapters.description")),
            );

        // Two-column: left chapter list, right detail pane.
        let mut left = div().v_flex().gap_2();
        for ch in &self.state.workspace.chapters {
            let id_owned = ch.chapter.id.0.clone();
            let is_active = self
                .state
                .selected_chapter_id
                .as_ref()
                .map(|c| c.0 == id_owned.as_str())
                .unwrap_or(false);
            let title = t!(
                "chapters.chapter_item_label",
                num = ch.chapter.chapter_number : {:02},
                title = ch.chapter.title.clone()
            ).into_owned();
            let chapter_id = ch.chapter.id.clone();
            let id_for_element = ch.chapter.id.0.clone();
            let status = ch.chapter.status;
            let tooltip_text = chapter_status_tooltip(status).to_string();
            left = left.child(
                div()
                    .id(id_for_element)
                    .px_3()
                    .py_2()
                    .rounded_sm()
                    .border_1()
                    .border_color(if is_active {
                        cx.theme().accent
                    } else {
                        cx.theme().border
                    })
                    .bg(if is_active {
                        cx.theme().accent.opacity(0.12)
                    } else {
                        cx.theme().popover
                    })
                    .text_sm()
                    .cursor_pointer()
                    .tooltip(move |window, cx| {
                        Tooltip::new(tooltip_text.clone()).build(window, cx)
                    })
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(title)
                            .child(chapter_status_tag(status)),
                    )
                    .on_click(cx.listener(move |view, _, _, _| {
                        view.state.select_chapter(&chapter_id);
                    })),
            );
        }
        let left_pane = div()
            .w(px(220.))
            .v_flex()
            .gap_2()
            .child(left);

        let right_pane = self.render_chapter_detail(cx);
        root = root.child(
            div()
                .flex()
                .flex_row()
                .gap_4()
                .child(left_pane)
                .child(div().flex_1().child(right_pane)),
        );
        root
    }

    pub fn render_chapter_detail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let ch = self
            .state
            .selected_chapter_id
            .as_ref()
            .and_then(|id| {
                self.state
                    .workspace
                    .chapters
                    .iter()
                    .find(|c| &c.chapter.id == id)
                    .cloned()
            });
        let ch = match ch {
            Some(c) => c,
            None => {
                return div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("chapters.select_chapter"))
                    .into_any_element();
            }
        };

        let status = ch.chapter.status;
        let tooltip_text = chapter_status_tooltip(status).to_string();
        let mut body = div()
            .v_flex()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_lg()
                            .font_semibold()
                            .child(t!(
                                "chapters.chapter_item_label",
                                num = ch.chapter.chapter_number : {:02},
                                title = ch.chapter.title.clone()
                            ).into_owned()),
                    )
                    .child(
                        div()
                            .id("chapter-detail-status")
                            .tooltip(move |window, cx| {
                                Tooltip::new(tooltip_text.clone()).build(window, cx)
                            })
                            .child(chapter_status_tag(status)),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(if ch.chapter.summary.is_empty() {
                        t!("chapters.no_summary").to_string()
                    } else {
                        ch.chapter.summary.clone()
                    }),
            );

        body = body.child(self.render_scene_list(&ch, cx));
        body = body.child(self.render_shot_detail(&ch, cx));
        body.into_any_element()
    }

    pub fn render_scene_list(
        &mut self,
        ch: &sagaline_core::model::ChapterWithAssets,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut list = div().v_flex().gap_2();
        for sc in &ch.scenes {
            let id = sc.scene.id.clone();
            let id_for_element = id.0.clone();
            let is_active = self
                .state
                .selected_scene_id
                .as_ref()
                .map(|s| s == &id)
                .unwrap_or(false);
            list = list.child(
                div()
                    .id(id_for_element)
                    .px_3()
                    .py_2()
                    .rounded_sm()
                    .border_1()
                    .border_color(if is_active {
                        cx.theme().accent
                    } else {
                        cx.theme().border
                    })
                    .bg(if is_active {
                        cx.theme().accent.opacity(0.12)
                    } else {
                        cx.theme().popover
                    })
                    .text_sm()
                    .cursor_pointer()
                    .child(t!(
                        "chapters.scene_list_item",
                        num = sc.scene.scene_number : {:02},
                        title = sc.scene.title.clone(),
                        shots = sc.shots.len()
                    ).into_owned())
                    .on_click(cx.listener(move |view, _, _, _| {
                        view.state.select_scene(id.clone());
                    })),
            );
        }
        list.into_any_element()
    }

    pub fn render_shot_detail(
        &mut self,
        ch: &sagaline_core::model::ChapterWithAssets,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sc = self
            .state
            .selected_scene_id
            .as_ref()
            .and_then(|id| ch.scenes.iter().find(|s| &s.scene.id == id));

        let sc = match sc {
            Some(s) => s,
            None => {
                return div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("chapters.select_scene"))
                    .into_any_element();
            }
        };

        let mut list = div().v_flex().gap_2();
        list = list.child(
            div()
                .text_sm()
                .font_semibold()
                .text_color(cx.theme().accent)
                .child(t!("chapters.shots_in_scene", title = sc.scene.title.clone()).into_owned()),
        );

        for sh in &sc.shots {
            // Build the cross-reference list: which character × age ×
            // appearance appears in this shot?
            let mut refs: Vec<String> = Vec::new();
            for sc_entry in &sh.characters {
                if let Some(c) = self
                    .state
                    .workspace
                    .characters
                    .iter()
                    .find(|c| c.character.id == sc_entry.character_id)
                {
                    let age = c
                        .ages
                        .iter()
                        .find(|a| a.age.id == sc_entry.age_id)
                        .map(|a| format!("{}岁", a.age.age))
                        .unwrap_or_default();
                    let app = c
                        .ages
                        .iter()
                        .find(|a| a.age.id == sc_entry.age_id)
                        .and_then(|a| {
                            a.appearances
                                .iter()
                                .find(|x| x.id == sc_entry.appearance_id)
                        })
                        .map(|x| x.name.clone())
                        .unwrap_or_default();
                    refs.push(format_char_ref_label(&c.character.name, &age, &app));
                }
            }

            let shot_id = sh.shot.id.clone();
            list = list.child(
                div()
                    .v_flex()
                    .gap_1()
                    .p_3()
                    .rounded_sm()
                    .border_1()
                    .border_color(cx.theme().border)
                    .cursor_pointer()
                    .id(shot_id.0.clone())
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().accent)
                                    .font_semibold()
                                .child(t!("overview.shot_label", num = sh.shot.shot_number).into_owned()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .child(sh.shot.description.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!(
                                "chapters.scene_shot_camera",
                                camera = sh.shot.camera.clone(),
                                movement = sh.shot.camera_movement.clone(),
                                secs = sh.shot.duration_seconds
                            ).into_owned())
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_1()
                            .items_center()
                            .children(if refs.is_empty() {
                                vec![Tag::secondary()
                                    .rounded_full()
                                    .with_size(Size::Small)
                                    .child(t!("chapters.no_refs").into_owned())
                                    .into_any_element()]
                            } else {
                                refs.iter()
                                    .map(|r| {
                                        Tag::info()
                                            .rounded_full()
                                            .with_size(Size::Small)
                                            .child(r.clone())
                                            .into_any_element()
                                    })
                                    .collect()
                            }),
                    )
                    .on_click(cx.listener(move |view, _, _, _| {
                        view.state.select_shot(shot_id.clone());
                    })),
            );
        }
        list.into_any_element()
    }
}