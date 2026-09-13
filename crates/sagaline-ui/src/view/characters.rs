//! Characters section: card per character, per-age appearance grid, and
//! the trailing Relationships sub-section. The avatar row for each age
//! is sourced from `super::format::render_reference_avatars` (used by
//! Environments and Props too).

use gpui_kit::component::*;
use gpui_kit::*;
#[allow(unused_imports)]
use sagaline_core::model::*;
use super::WorkspaceView;
use super::format::render_reference_avatars;

impl WorkspaceView {
    pub fn render_characters(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div()
            .v_flex()
            .gap_3()
            .child(div().text_xl().font_semibold().child(t!("characters.title")))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("characters.description")),
            );

        if self.state.workspace.characters.is_empty() {
            root = root.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("characters.empty")),
            );
            return root;
        }

        for ch in &self.state.workspace.characters {
            root = root.child(self.render_character_card(ch, cx));
        }
        root = root.child(self.render_relationships(cx));
        root
    }

    pub fn render_character_card(
        &self,
        ch: &sagaline_core::model::CharacterWithAssets,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut card = div()
            .v_flex()
            .gap_3()
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
                            .text_lg()
                            .font_semibold()
                            .child(ch.character.name.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().accent)
                            .child(t!("characters.ages_count", count = ch.ages.len())),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(if ch.character.profile.is_empty() {
                        t!("characters.no_profile").to_string()
                    } else {
                        ch.character.profile.clone()
                    }),
            );

        for age in &ch.ages {
            card = card.child(self.render_age_card(ch, age, cx));
        }
        card
    }

    pub fn render_age_card(
        &self,
        ch: &sagaline_core::model::CharacterWithAssets,
        age: &sagaline_core::model::CharacterAgeWithAssets,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut age_block = div()
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
                            .font_semibold()
                            .text_color(cx.theme().accent)
                            .child(t!("characters.age_label", age = age.age.age)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if age.age.description.is_empty() {
                                String::new()
                            } else {
                                t!("characters.age_with_description", description = age.age.description.clone()).into_owned()
                            }),
                    ),
            );

        if age.appearances.is_empty() {
            age_block = age_block.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("characters.no_appearances")),
            );
        } else {
            // Render appearances in a small grid so the parent-child
            // relationship is visible.
            let apps: Vec<AnyElement> = age
                .appearances
                .iter()
                .map(|a| {
                    div()
                        .v_flex()
                        .gap_1()
                        .p_2()
                        .rounded_sm()
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().popover)
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .text_color(cx.theme().accent)
                                .child(a.name.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(if a.clothing.is_empty() {
                                    t!("characters.no_clothing").to_string()
                                } else {
                                    a.clothing.clone()
                                }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(if a.emotion.is_empty() {
                                    String::new()
                                } else {
                                    t!("characters.mood_prefix", emotion = a.emotion.clone()).into_owned()
                                }),
                        )
                        .into_any_element()
                })
                .collect();

            age_block = age_block.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!(
                        "characters.appearances_summary",
                        name = ch.character.name.clone(),
                        count = age.appearances.len(),
                        plural = if age.appearances.len() == 1 { "" } else { "s" }
                    ).into_owned()),
            );
            age_block = age_block.child(
                div()
                    .grid()
                    .grid_cols(3)
                    .gap_2()
                    .children(apps),
            );
        }
        age_block = age_block.child(render_reference_avatars(&age.references, cx));
        age_block
    }

    /// Relationships sub-section rendered at the bottom of the Characters
    /// view. Lists every `Relationship` row for the active story, with a
    /// coloured status badge (Draft / Active / Broken / Ended / Unknown).
    pub fn render_relationships(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div()
            .v_flex()
            .gap_2()
            .child(
                div()
                    .text_lg()
                    .font_semibold()
                    .child(t!("characters.relationships_title")),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("characters.relationships_description")),
            );

        let rels = &self.state.workspace.relationships;
        if rels.is_empty() {
            return root.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("characters.relationships_empty")),
            );
        }

        // Look up the name of a character by id, falling back to the id when
        // the character is missing (defensive — the FK should keep these in
        // sync, but the UI shouldn't crash on a stray id).
        let name_for = |cid: &sagaline_core::model::CharacterId| -> String {
            self.state
                .workspace
                .characters
                .iter()
                .find(|c| &c.character.id == cid)
                .map(|c| c.character.name.clone())
                .unwrap_or_else(|| cid.0.clone())
        };

        for rel in rels {
            let from_name = name_for(&rel.from_character);
            let to_name = name_for(&rel.to_character);
            let (badge_text, badge_color) = match rel.status {
                sagaline_core::model::RelationshipStatus::Active => {
                    ("active", cx.theme().success)
                }
                sagaline_core::model::RelationshipStatus::Broken => {
                    ("broken", cx.theme().danger)
                }
                sagaline_core::model::RelationshipStatus::Ended => {
                    ("ended", cx.theme().warning)
                }
                sagaline_core::model::RelationshipStatus::Unknown => {
                    ("unknown", cx.theme().info)
                }
            };
            root = root.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .p_3()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().popover)
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .child(t!("characters.relationship_arrow", from = from_name, to = to_name).into_owned()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(rel.kind.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .border_1()
                            .border_color(badge_color)
                            .text_color(badge_color)
                            .child(badge_text.to_string()),
                    )
                    .child(if rel.note.is_empty() {
                        div()
                    } else {
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(rel.note.clone())
                    }),
            );
        }
        root
    }
}