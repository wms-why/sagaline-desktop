//! References section: every reference image the BYOK pipeline can reach,
//! grouped by `ReferenceKind`. `collect_references` is also called from
//! the sidebar to render the count badge, so it lives here (and the
//! shell pulls it in via `super::references::collect_references`).

use gpui_kit::component::avatar::Avatar;
use gpui_kit::component::*;
use gpui_kit::*;
#[allow(unused_imports)]
use sagaline_core::model::*;
use super::WorkspaceView;
use super::format::image_source_for;

impl WorkspaceView {
    /// Flatten every reference image the workspace knows about: those that
    /// hang off a character age, environment, prop, or shot. Returned in a
    /// stable insertion order (character ages first, then environments,
    /// then props, then shots). Cheap — references are already aggregated
    /// onto the workspace by `db::load_workspace`.
    pub fn collect_references(&self) -> Vec<&sagaline_core::model::ReferenceImage> {
        let mut out = Vec::new();
        for ch in &self.state.workspace.characters {
            for age in &ch.ages {
                out.extend(age.references.iter());
            }
        }
        for env in &self.state.workspace.environments {
            out.extend(env.references.iter());
        }
        for p in &self.state.workspace.props {
            out.extend(p.references.iter());
        }
        for ch in &self.state.workspace.chapters {
            for sc in &ch.scenes {
                for sh in &sc.shots {
                    out.extend(sh.references.iter());
                }
            }
        }
        out
    }

    pub fn render_references(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let refs = self.collect_references();
        let mut root = div()
            .v_flex()
            .gap_3()
            .child(div().text_xl().font_semibold().child(t!("references.title")))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        t!("references.description"),
                    ),
            );

        if refs.is_empty() {
            return root.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("references.empty")),
            );
        }

        for kind in [
            sagaline_core::model::ReferenceKind::Character,
            sagaline_core::model::ReferenceKind::Environment,
            sagaline_core::model::ReferenceKind::Prop,
            sagaline_core::model::ReferenceKind::Shot,
        ] {
            let of_kind: Vec<&&sagaline_core::model::ReferenceImage> =
                refs.iter().filter(|r| r.kind == kind).collect();
            if of_kind.is_empty() {
                continue;
            }
            let kind_label = match kind {
                sagaline_core::model::ReferenceKind::Character => t!("references.group_characters"),
                sagaline_core::model::ReferenceKind::Environment => t!("references.group_environments"),
                sagaline_core::model::ReferenceKind::Prop => t!("references.group_props"),
                sagaline_core::model::ReferenceKind::Shot => t!("references.group_shots"),
            };
            let mut group = div()
                .v_flex()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(cx.theme().accent)
                        .child(kind_label.to_string()),
                );
            for r in of_kind {
                group = group.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .p_2()
                        .rounded_sm()
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().popover)
                        .child(self.avatar_for_reference(r))
                        .child(
                            div()
                                .v_flex()
                                .gap_1()
                                .child(div().text_sm().font_semibold().child(r.label.clone()))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                    .child(if r.source.is_empty() {
                                        t!("references.no_source").into_owned()
                                    } else {
                                        r.source.clone()
                                    }),
                                ),
                        ),
                );
            }
            root = root.child(group);
        }
        root
    }

    /// Build an Avatar element for a reference row. When the source string
    /// is non-empty we wire it as an `ImageSource`; otherwise we fall back
    /// to the placeholder icon so the label still renders as initials.
    pub fn avatar_for_reference(
        &self,
        r: &sagaline_core::model::ReferenceImage,
    ) -> Avatar {
        let mut av = Avatar::new().name(r.label.clone());
        if let Some(src) = image_source_for(&r.source) {
            av = av.src(src);
        }
        av
    }
}