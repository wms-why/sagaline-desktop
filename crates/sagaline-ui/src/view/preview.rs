//! Right pane — Preview tab: domain field list for the selected entity.

use gpui_base::{StyledExt, h_flex, v_flex};
use gpui_component::ActiveTheme;
use gpui_kit::*;

use sagaline_core::{EntityId as StoryEntityId, ParsedEntity, StoryGraph, render_preview_lines};

use crate::state::WorkspaceState;

/// Render the preview pane from a domain `(label, value)` list.
/// No raw YAML, no Markdown body — the in-memory graph is the
/// source of truth, but the user sees a labelled field list.
fn render_entity_preview(e: &ParsedEntity, muted: Hsla) -> gpui_kit::Div {
    let mut col = v_flex().gap_3().size_full().p_4();

    let title = e.title().unwrap_or_else(|| e.id.0.clone());
    col = col.child(
        v_flex()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(e.type_.as_str().to_string()),
            )
            .child(div().text_xl().font_bold().child(title)),
    );

    let rows = render_preview_lines(e);
    if rows.is_empty() {
        return col;
    }
    let mut list = v_flex().gap_2();
    for (label, value) in rows {
        if label == "Type" {
            // Already shown as the caption above the heading.
            continue;
        }
        list = list.child(
            h_flex()
                .gap_3()
                .items_start()
                .child(
                    div()
                        .w(px(120.))
                        .text_sm()
                        .text_color(muted)
                        .child(label.to_string()),
                )
                .child(div().text_sm().flex_1().child(value)),
        );
    }
    col = col.child(list);
    col
}

fn find_entity<'a>(
    graph: &'a StoryGraph,
    id: &StoryEntityId,
) -> Option<&'a ParsedEntity> {
    if graph.story.id.as_str() == id.0 {
        return Some(&graph.story);
    }
    graph.entity(id)
}

pub(super) fn render_preview(state: &WorkspaceState, cx: &gpui_kit::App) -> gpui_kit::Div {
    let muted = cx.theme().muted_foreground;
    let inner: gpui_kit::Div = match (state.graph(), state.selected.as_ref()) {
        (Some(graph), Some(sel)) => match find_entity(graph, sel) {
            Some(e) => render_entity_preview(e, muted),
            None => div().child("Selected entity is no longer in the graph."),
        },
        (Some(_), None) => div().child("Select an entity on the left to preview it."),
        (None, _) => div().child("No story loaded."),
    };

    v_flex().flex_1().h_full().child(inner)
}