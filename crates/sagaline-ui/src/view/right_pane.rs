//! Right pane: tab bar + active panel.
//!
//! The actual panel renderers live in `preview`, `keys`, and the
//! `activity` crate module; this file just chooses between them.

use gpui_base::{h_flex, v_flex};
use gpui_kit::*;

use crate::activity::{render_activity, AgentEventLog};
use crate::state::WorkspaceState;
use crate::view::keys::render_keys_panel;
use crate::view::preview::render_preview;
use crate::view::{RightTab, WorkspaceView};

pub(super) fn render_right_pane(
    state: &WorkspaceState,
    tab: RightTab,
    log: Option<&AgentEventLog>,
    activity_count: usize,
    view: &gpui_kit::Entity<WorkspaceView>,
    cx: &gpui_kit::App,
) -> gpui_kit::Div {
    let tabs = render_tab_bar(tab, activity_count);
    let body: gpui_kit::Div = match tab {
        RightTab::Preview => render_preview(state, cx),
        RightTab::Activity => render_activity(state, log, cx),
        RightTab::Keys => render_keys_panel(view, cx),
    };
    v_flex().flex_1().h_full().child(tabs).child(body)
}

fn render_tab_label(label: &str, tab: RightTab, active: RightTab) -> impl IntoElement {
    let weight = if active == tab {
        FontWeight::BOLD
    } else {
        FontWeight::NORMAL
    };
    let target = tab.index();
    div()
        .id(format!("tab-{target}"))
        .text_sm()
        .font_weight(weight)
        .on_click(move |_, _, cx| {
            cx.dispatch_action(&crate::actions::SwitchTab { tab: target });
        })
        .child(label.to_string())
}

fn render_tab_bar(active: RightTab, activity_count: usize) -> gpui_kit::Div {
    let activity_label = if activity_count == 0 {
        "Activity".to_string()
    } else {
        format!("Activity ({activity_count})")
    };
    h_flex()
        .w_full()
        .px_2()
        .py_1()
        .gap_3()
        .border_b_1()
        .child(render_tab_label("Preview", RightTab::Preview, active))
        .child(render_tab_label(&activity_label, RightTab::Activity, active))
        .child(render_tab_label("Keys", RightTab::Keys, active))
}