//! Left pane: domain tree of the currently-open story.

use gpui_base::{Tree, TreeState, v_flex};
use gpui_kit::*;

use sagaline_core::StoryGraph;

pub(super) fn render_left_pane(
    graph: Option<&StoryGraph>,
    tree_state: Option<&gpui_kit::Entity<TreeState>>,
) -> gpui_kit::Div {
    let col = v_flex().w_72().h_full().border_r_1().overflow_hidden();

    let (Some(_), Some(tree)) = (graph, tree_state) else {
        return col.p_2().child(
            div()
                .text_sm()
                .child("No story loaded. ⌘ O to open one."),
        );
    };

    col.child(Tree::new(tree).size_full())
}