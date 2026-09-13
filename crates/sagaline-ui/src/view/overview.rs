//! Overview section: story summary, top-level counters, and the
//! Story → Chapter → Scene → Shot concept tree with cross-links.

use gpui_kit::component::Size;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::*;
use gpui_kit::*;
#[allow(unused_imports)]
use sagaline_core::model::*;
use super::WorkspaceView;
use super::format::format_char_ref_label;

impl WorkspaceView {
    pub fn render_overview(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let story = match &self.state.workspace.story {
            Some(s) => s,
            None => {
                return div()
                    .v_flex()
                    .gap_2()
                    .child(div().text_lg().font_semibold().child(t!("overview.empty_title")))
                    .child(div().text_sm().child(
                        t!("overview.empty_body"),
                    ));
            }
        };

        let chapter_count = self.state.workspace.chapters.len();
        let scene_count: usize = self
            .state
            .workspace
            .chapters
            .iter()
            .map(|c| c.scenes.len())
            .sum();
        let shot_count: usize = self
            .state
            .workspace
            .chapters
            .iter()
            .flat_map(|c| c.scenes.iter())
            .map(|s| s.shots.len())
            .sum();
        let character_count = self.state.workspace.characters.len();
        let environment_count = self.state.workspace.environments.len();
        let prop_count = self.state.workspace.props.len();

        let stat = |label: std::borrow::Cow<'static, str>, value: String| -> AnyElement {
            div()
                .v_flex()
                .gap_1()
                .p_4()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover)
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(label.into_owned()),
                )
                .child(
                    div()
                        .text_2xl()
                        .font_semibold()
                        .child(value),
                )
                .into_any_element()
        };

        div()
            .v_flex()
            .gap_6()
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_2xl()
                            .font_semibold()
                            .child(story.title.clone()),
                    )
                    .child(div().text_sm().text_color(cx.theme().muted_foreground).child(
                        if story.description.is_empty() {
                            t!("overview.no_description").to_string()
                        } else {
                            story.description.clone()
                        },
                    )),
            )
            .child(
                div()
                    .grid()
                    .grid_cols(3)
                    .gap_3()
                    .children([
                        stat(t!("overview.stat_stories"), "1".into()),
                        stat(t!("overview.stat_characters"), character_count.to_string()),
                        stat(t!("overview.stat_environments"), environment_count.to_string()),
                        stat(t!("overview.stat_props"), prop_count.to_string()),
                        stat(t!("overview.stat_chapters"), chapter_count.to_string()),
                        stat(t!("overview.stat_scenes_shots"), format!("{} / {}", scene_count, shot_count)),
                    ]),
            )
            .child(self.render_concept_tree(cx))
    }

    /// Visualize the hierarchy: Story → Chapter → Scene → Shot as a tree,
    /// with cross-links to characters / environments / props.
    pub fn render_concept_tree(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut tree = div().v_flex().gap_2();

        if self.state.workspace.chapters.is_empty() {
            tree = tree.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("overview.no_chapters")),
            );
            return tree;
        }

        for ch in &self.state.workspace.chapters {
            tree = tree.child(self.render_chapter_node(ch, cx));
        }
        tree
    }

    pub fn render_chapter_node(
        &self,
        ch: &sagaline_core::model::ChapterWithAssets,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut node = div()
            .v_flex()
            .gap_2()
            .p_4()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().accent)
                            .font_semibold()
                            .child(t!("overview.chapter_label", num = ch.chapter.chapter_number))
                    )
                    .child(
                        div()
                            .text_base()
                            .font_semibold()
                            .child(ch.chapter.title.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("overview.chapter_scenes", count = ch.scenes.len())),
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

        for sc in &ch.scenes {
            node = node.child(self.render_scene_node(sc, cx));
        }

        node
    }

    pub fn render_scene_node(
        &self,
        sc: &sagaline_core::model::SceneWithAssets,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let environment_label = sc
            .scene
            .environment_id
            .as_ref()
            .and_then(|eid| {
                self.state
                    .workspace
                    .environments
                    .iter()
                    .find(|e| e.environment.id == *eid)
                    .map(|e| e.environment.name.clone())
            })
            .unwrap_or_else(|| "—".to_string());

        let mut node = div()
            .v_flex()
            .gap_2()
            .pl_4()
            .border_l_2()
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().accent)
                            .font_semibold()
                            .child(t!("overview.scene_label", num = sc.scene.scene_number)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .child(sc.scene.title.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("overview.scene_env_duration", env = environment_label, secs = sc.scene.duration_seconds)),
                    ),
            );

        for sh in &sc.shots {
            // Cross-reference: which character + which appearance does this
            // shot use?
            let mut char_summary = Vec::new();
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
                    let app_name = c
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
                    char_summary.push(format_char_ref_label(
                        &c.character.name,
                        &age,
                        &app_name,
                    ));
                }
            }

            node = node.child(
                div()
                    .pl_4()
                    .border_l_2()
                    .border_color(cx.theme().border)
                    .v_flex()
                    .gap_1()
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
                                    .child(t!("overview.shot_label", num = sh.shot.shot_number)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .child(sh.shot.description.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("overview.shot_duration", secs = sh.shot.duration_seconds)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_1()
                            .items_center()
                            .children(if char_summary.is_empty() {
                                vec![Tag::secondary()
                                    .rounded_full()
                                    .with_size(Size::Small)
                                    .child("🎭 —")
                                    .into_any_element()]
                            } else {
                                char_summary
                                    .iter()
                                    .map(|s| {
                                        Tag::info()
                                            .rounded_full()
                                            .with_size(Size::Small)
                                            .child(format!("🎭 {}", s))
                                            .into_any_element()
                                    })
                                    .collect()
                            }),
                    )
            );
        }

        node
    }
}