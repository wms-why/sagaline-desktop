//! Top-level gpui view for an open story.
//!
//! Two-pane layout with a tabbed right pane:
//!
//! - **Left:** a `gpui_base::Tree` widget showing the story
//!   hierarchy grouped by entity type (Bible / Characters /
//!   Environments / Props / Chapters). Each chapter is followed
//!   directly by its scenes — no "Scenes in <slug>" sub-folder.
//! - **Right:** tabbed. The default **Preview** tab shows a domain
//!   field list for the selected entity. The **Activity** tab
//!   shows a live scrollback of the agent's `AgentEvent` stream
//!   (read from the [`AgentEventLog`] global). The **Keys** tab
//!   lists the BYOK provider keys.
//!
//! The view emits a [`StoryOpened`] event every time a story
//! successfully opens; the app shell subscribes via
//! `cx.subscribe(&view, ...)` to kick off the agent loop.
//!
//! Storage vocabulary is deliberately absent from the surface: the
//! user types a story title, picks a project location once via
//! Project Settings, and otherwise navigates the tree in domain
//! terms. The `StoryService` global is the only seam the UI
//! crosses into the binary crate.
//!
//! The implementation is split across several sibling modules:
//!
//! - [`tree`] — `TreeItem` construction from the graph.
//! - [`banner`], [`left_pane`], [`right_pane`], [`onboarding`],
//!   [`preview`], [`keys`] — the renderable panes.
//! - [`new_story_dialog`], [`project_settings_dialog`] — the modal
//!   dialogs opened from the `CreateStory` and `OpenProjectSettings`
//!   actions.
//! - [`actions`] — `register_actions` + `open_story_at_path` (the
//!   handler that turns a picked directory into a [`Story`]).
//! - [`tests`] — validation-only unit tests; render plumbing is
//!   covered by the `tests/` integration tests.

mod actions;
mod banner;
mod keys;
mod left_pane;
mod new_story_dialog;
mod onboarding;
mod preview;
mod project_settings_dialog;
mod right_pane;
#[cfg(test)]
mod tests;
mod tree;

pub use actions::register_actions;

use std::sync::Arc;

use gpui_base::{TreeState, h_flex, v_flex};
use gpui_component::WindowExt;
use gpui_kit::*;
use gpui_kit::component::input::InputState;

use crate::activity::AgentEventLog;
use crate::state::WorkspaceState;
use crate::view::banner::render_banner;
use crate::view::left_pane::render_left_pane;
use crate::view::new_story_dialog::maybe_open_new_story_dialog;
use crate::view::onboarding::render_onboarding;
use crate::view::project_settings_dialog::maybe_open_project_settings_dialog;
use crate::view::right_pane::render_right_pane;
use crate::view::tree::build_tree_items;

use sagaline_core::{EntityId as StoryEntityId, Story};

/// Emitted by [`WorkspaceView`] after a successful `open_story` /
/// `reload`. The app shell uses this to trigger the agent loop.
#[derive(Debug, Clone)]
pub struct StoryOpened {
    pub story: Arc<Story>,
}

/// Which tab is active in the right pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightTab {
    #[default]
    Preview,
    Activity,
    /// BYOK provider key management. Always available — keys are
    /// scoped to the user's data dir, not the open story.
    Keys,
}

impl RightTab {
    /// Index used by the `SwitchTab` action payload. Stable; the
    /// tab order is encoded in the tab bar.
    pub fn index(self) -> u32 {
        match self {
            RightTab::Preview => 0,
            RightTab::Activity => 1,
            RightTab::Keys => 2,
        }
    }

    /// Inverse of [`Self::index`]. Out-of-range values fall back
    /// to Preview — defensive against malformed action payloads.
    pub fn from_index(i: u32) -> Self {
        match i {
            0 => RightTab::Preview,
            1 => RightTab::Activity,
            2 => RightTab::Keys,
            _ => RightTab::Preview,
        }
    }
}

/// Top-level view. Holds [`WorkspaceState`] directly; mutations call
/// `cx.notify()` to trigger a redraw.
pub struct WorkspaceView {
    state: WorkspaceState,
    tree_state: Option<Entity<TreeState>>,
    right_tab: RightTab,
    show_new_story_dialog: bool,
    new_story_title_input: Option<Entity<InputState>>,
    new_story_error: Option<String>,
    new_story_dialog_open: bool,
    show_project_settings_dialog: bool,
    project_settings_dialog_open: bool,
    /// BYOK key-entry form on the Keys tab. Inline (not a modal) so
    /// the user can see existing keys while entering a new one.
    /// Inputs are created lazily the first time the form opens;
    /// closing the form drops the `InputState` entities so the
    /// plaintext doesn't linger in GPUI's memory between sessions
    /// of use.
    show_add_key_form: bool,
    add_provider_input: Option<Entity<InputState>>,
    add_key_id_input: Option<Entity<InputState>>,
    add_secret_input: Option<Entity<InputState>>,
    add_key_error: Option<String>,
}

impl EventEmitter<StoryOpened> for WorkspaceView {}

impl WorkspaceView {
    pub fn new() -> Self {
        Self {
            state: WorkspaceState::new(),
            tree_state: None,
            right_tab: RightTab::default(),
            show_new_story_dialog: false,
            new_story_title_input: None,
            new_story_error: None,
            new_story_dialog_open: false,
            show_project_settings_dialog: false,
            project_settings_dialog_open: false,
            show_add_key_form: false,
            add_provider_input: None,
            add_key_id_input: None,
            add_secret_input: None,
            add_key_error: None,
        }
    }

    pub fn new_with_tree(cx: &mut gpui_kit::App) -> Self {
        let tree_state = cx.new(|cx| TreeState::new(cx));
        Self {
            state: WorkspaceState::new(),
            tree_state: Some(tree_state),
            right_tab: RightTab::default(),
            show_new_story_dialog: false,
            new_story_title_input: None,
            new_story_error: None,
            new_story_dialog_open: false,
            show_project_settings_dialog: false,
            project_settings_dialog_open: false,
            show_add_key_form: false,
            add_provider_input: None,
            add_key_id_input: None,
            add_secret_input: None,
            add_key_error: None,
        }
    }

    pub fn state(&self) -> &WorkspaceState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut WorkspaceState {
        &mut self.state
    }

    pub fn right_tab(&self) -> RightTab {
        self.right_tab
    }

    pub fn set_right_tab(&mut self, tab: RightTab, cx: &mut Context<Self>) {
        if self.right_tab != tab {
            self.right_tab = tab;
            cx.notify();
        }
    }

    /// Adopt a freshly-opened [`Story`]. Emits [`StoryOpened`] on
    /// the first successful open of the session.
    pub fn open_story(&mut self, story: Arc<Story>, cx: &mut Context<Self>) {
        let was_loaded = self.state.story().is_some();
        self.state.open_story(story.clone());
        self.rebuild_tree(cx);
        if self.state.story().is_some() {
            self.close_new_story_dialog(cx);
        }
        cx.notify();
        if self.state.story().is_some() && !was_loaded {
            cx.emit(StoryOpened { story });
        }
    }

    /// Re-walk the currently-open story and refresh the graph. Emits
    /// [`StoryOpened`] every time so the agent loop re-runs.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if self.state.story().is_none() {
            return;
        }
        self.state.reload();
        self.rebuild_tree(cx);
        cx.notify();
        if let Some(story) = self.state.story().cloned() {
            cx.emit(StoryOpened { story });
        }
    }

    // ---- new-story dialog state --------------------------------------

    pub fn open_new_story_dialog(&mut self, cx: &mut Context<Self>) {
        self.show_new_story_dialog = true;
        self.new_story_error = None;
        cx.notify();
    }

    pub fn set_new_story_error(&mut self, msg: Option<String>, cx: &mut Context<Self>) {
        self.new_story_error = msg;
        cx.notify();
    }

    pub fn close_new_story_dialog(&mut self, cx: &mut Context<Self>) {
        self.show_new_story_dialog = false;
        self.new_story_title_input = None;
        self.new_story_error = None;
        self.new_story_dialog_open = false;
        cx.notify();
    }

    pub fn mark_new_story_dialog_opened(&mut self, cx: &mut Context<Self>) {
        self.new_story_dialog_open = true;
        cx.notify();
    }

    pub fn new_story_error_msg(&self) -> Option<&str> {
        self.new_story_error.as_deref()
    }

    pub fn new_story_title_input(&self) -> Option<&Entity<InputState>> {
        self.new_story_title_input.as_ref()
    }

    pub fn ensure_new_story_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.new_story_title_input.is_none() {
            self.new_story_title_input = Some(cx.new(|inner_cx| {
                InputState::new(window, inner_cx)
                    .placeholder("e.g. The Hollow Star")
            }));
        }
    }

    pub fn open_project_settings_dialog(&mut self, cx: &mut Context<Self>) {
        self.show_project_settings_dialog = true;
        cx.notify();
    }

    pub fn close_project_settings_dialog(&mut self, cx: &mut Context<Self>) {
        self.show_project_settings_dialog = false;
        self.project_settings_dialog_open = false;
        cx.notify();
    }

    pub fn mark_project_settings_dialog_opened(&mut self, cx: &mut Context<Self>) {
        self.project_settings_dialog_open = true;
        cx.notify();
    }

    // ---- BYOK key-entry form (inline, Keys tab) ---------------------

    /// Open the paste-plaintext add-key form. Inputs are created
    /// lazily on the next `Render::render` call so the construction
    /// stays on the foreground thread.
    pub fn open_add_key_form(&mut self, cx: &mut Context<Self>) {
        self.show_add_key_form = true;
        self.add_key_error = None;
        cx.notify();
    }

    /// Close the add-key form and drop the input entities so the
    /// plaintext doesn't sit in GPUI's memory while the form is
    /// hidden. The next `open_add_key_form` call rebuilds fresh
    /// `InputState`s.
    pub fn close_add_key_form(&mut self, cx: &mut Context<Self>) {
        self.show_add_key_form = false;
        self.add_provider_input = None;
        self.add_key_id_input = None;
        self.add_secret_input = None;
        self.add_key_error = None;
        cx.notify();
    }

    /// Surface a validation / store error under the form. Cleared
    /// by the next successful save or by `close_add_key_form`.
    pub fn set_add_key_error(&mut self, msg: Option<String>, cx: &mut Context<Self>) {
        self.add_key_error = msg;
        cx.notify();
    }

    pub fn add_key_error_msg(&self) -> Option<&str> {
        self.add_key_error.as_deref()
    }

    pub fn show_add_key_form(&self) -> bool {
        self.show_add_key_form
    }

    pub fn add_provider_input(&self) -> Option<&Entity<InputState>> {
        self.add_provider_input.as_ref()
    }

    pub fn add_key_id_input(&self) -> Option<&Entity<InputState>> {
        self.add_key_id_input.as_ref()
    }

    pub fn add_secret_input(&self) -> Option<&Entity<InputState>> {
        self.add_secret_input.as_ref()
    }

    /// Idempotently create the three `InputState` entities the
    /// add-key form needs. The provider / key_id fields are plain
    /// text inputs; the secret field is masked so a shoulder-surfer
    /// can't read the API key while it's typed.
    pub fn ensure_add_key_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.add_provider_input.is_none() {
            self.add_provider_input = Some(cx.new(|inner_cx| {
                InputState::new(window, inner_cx).placeholder("e.g. openai")
            }));
        }
        if self.add_key_id_input.is_none() {
            self.add_key_id_input = Some(cx.new(|inner_cx| {
                InputState::new(window, inner_cx).placeholder("e.g. default")
            }));
        }
        if self.add_secret_input.is_none() {
            self.add_secret_input = Some(cx.new(|inner_cx| {
                InputState::new(window, inner_cx)
                    .placeholder("paste API key — stored age-encrypted")
                    .masked(true)
            }));
        }
    }

    pub(super) fn rebuild_tree(&mut self, cx: &mut Context<Self>) {
        let Some(tree_state) = self.tree_state.clone() else {
            return;
        };
        let items = build_tree_items(self.state.graph());
        tree_state.update(cx, |state, cx| {
            state.set_items(items, cx);
        });
    }
}

impl Default for WorkspaceView {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(tree_state) = &self.tree_state {
            let tree_sel = tree_state.read(cx).selected_item().cloned();
            let desired = tree_sel.map(|item| StoryEntityId(item.id.to_string()));
            if desired != self.state.selected {
                self.state.select(desired);
            }
        }
        let activity_log: Option<AgentEventLog> = if cx.has_global::<AgentEventLog>() {
            Some(cx.global::<AgentEventLog>().clone())
        } else {
            None
        };
        let activity_count = activity_log
            .as_ref()
            .map(|l| l.events.len())
            .unwrap_or(0);

        if self.show_new_story_dialog && self.new_story_title_input.is_none() {
            self.ensure_new_story_inputs(window, cx);
        }

        if self.show_add_key_form && self.right_tab == RightTab::Keys {
            self.ensure_add_key_inputs(window, cx);
        }

        if let Some(title_input) = self.new_story_title_input.as_ref() {
            let view_handle = cx.entity().clone();
            maybe_open_new_story_dialog(
                view_handle,
                self.show_new_story_dialog,
                self.new_story_dialog_open,
                title_input.clone(),
                self.state.project_location_summary(),
                window,
                cx,
            );
        }

        if !self.show_new_story_dialog
            && self.new_story_dialog_open
            && window.has_active_dialog(cx)
        {
            window.close_dialog(cx);
        }

        if self.show_project_settings_dialog {
            let view_handle = cx.entity().clone();
            let location_display = self.state.project_location_summary();
            maybe_open_project_settings_dialog(
                view_handle,
                self.show_project_settings_dialog,
                self.project_settings_dialog_open,
                location_display,
                window,
                cx,
            );
        }
        if !self.show_project_settings_dialog
            && self.project_settings_dialog_open
            && window.has_active_dialog(cx)
        {
            window.close_dialog(cx);
        }

        let banner = render_banner(
            &self.state,
            self.show_new_story_dialog,
            self.show_project_settings_dialog,
            self.state.graph().is_none(),
        );

        let body: gpui_kit::Div = if self.state.graph().is_none() {
            let view_for_onboarding = cx.entity().clone();
            render_onboarding(view_for_onboarding, cx)
        } else {
            let view_for_keys = cx.entity();
            h_flex()
                .size_full()
                .child(render_left_pane(
                    self.state.graph(),
                    self.tree_state.as_ref(),
                ))
                .child(render_right_pane(
                    &self.state,
                    self.right_tab,
                    activity_log.as_ref(),
                    activity_count,
                    &view_for_keys,
                    cx,
                ))
        };

        v_flex().size_full().child(banner).child(body)
    }
}