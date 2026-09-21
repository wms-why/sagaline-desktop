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
use std::path::PathBuf;
use std::sync::Arc;

use gpui_base::{StyledExt, Tree, TreeItem, TreeState, h_flex, v_flex};
use gpui_component::ActiveTheme;
use gpui_component::WindowExt;
use gpui_kit::*;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::StatefulInteractiveElement;

use crate::actions::{
    CreateStory, DeleteProviderKey, ImportProviderKeyFromFile, OpenProjectSettings, OpenStory,
    ReloadStory, SwitchTab,
};
use crate::activity::AgentEventLog;
use crate::state::{StoryService, StoryServiceSlot, WorkspaceState};

use sagaline_bridge::BridgeSlot;

use sagaline_core::{
    EntityId as StoryEntityId, EntityType as StoryEntityType, ParsedEntity, ProjectLocation,
    Story, StoryHandle, render_preview_lines,
};

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
        }
    }

    pub fn new_with_tree(cx: &mut App) -> Self {
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

    fn rebuild_tree(&mut self, cx: &mut Context<Self>) {
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
                    cx,
                ))
        };

        v_flex().size_full().child(banner).child(body)
    }
}

// ---------------------------------------------------------------------------
// Tree building
// ---------------------------------------------------------------------------

/// Build the `TreeItem` hierarchy from a `StoryGraph`.
///
/// The five top-level groups are domain types (Bible / Characters /
/// Environments / Props / Chapters). Each chapter is followed
/// directly by its scenes — there is no nested "Scenes in <slug>"
/// folder, no leading-whitespace indenting (the tree widget handles
/// indentation), and no slug in the label.
fn build_tree_items(graph: Option<&sagaline_core::StoryGraph>) -> Vec<TreeItem> {
    let Some(g) = graph else {
        return Vec::new();
    };

    let mut root_items: Vec<TreeItem> = Vec::new();
    root_items.push(TreeItem::new(
        g.story.id.0.clone(),
        g.story
            .title()
            .unwrap_or_else(|| g.story.id.0.clone()),
    ));

    let sections: &[(StoryEntityType, &str)] = &[
        (StoryEntityType::Bible, "Bible"),
        (StoryEntityType::Character, "Characters"),
        (StoryEntityType::Environment, "Environments"),
        (StoryEntityType::Prop, "Props"),
        (StoryEntityType::Chapter, "Chapters"),
    ];

    for (ty, label) in sections.iter() {
        let entries = g.entities_by_type(*ty);
        if entries.is_empty() {
            continue;
        }
        let group = match ty {
            StoryEntityType::Chapter => {
                let mut group = TreeItem::new(format!("__group_{label}"), label.to_string())
                    .expanded(true);
                for ch in &entries {
                    group = group.child(TreeItem::new(ch.id.0.clone(), entity_label(ch)));
                    let scenes_under = g.entities_by_type(StoryEntityType::Scene);
                    for sc in scenes_under
                        .into_iter()
                        .filter(|s| s.path.starts_with(format!("chapters/{}/", ch.slug).as_str()))
                    {
                        group = group.child(TreeItem::new(sc.id.0.clone(), entity_label(sc)));
                    }
                }
                group
            }
            _ => {
                let mut group = TreeItem::new(format!("__group_{label}"), label.to_string())
                    .expanded(true);
                for e in &entries {
                    group = group.child(TreeItem::new(e.id.0.clone(), entity_label(e)));
                }
                group
            }
        };
        root_items.push(group);
    }

    root_items
}

fn entity_label(e: &ParsedEntity) -> String {
    e.title().unwrap_or_else(|| e.id.0.clone())
}

// ---------------------------------------------------------------------------
// Action wiring
// ---------------------------------------------------------------------------

pub fn register_actions(view: Entity<WorkspaceView>, cx: &mut App) {
    use crate::state::KeyStoreSlot;

    let view_for_open = view.clone();
    cx.on_action::<OpenStory>(move |_, cx| {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open a story".into()),
        });
        let view_for_async = view_for_open.downgrade();
        cx.spawn(async move |cx| {
            let result = receiver.await;
            let paths = match result {
                Ok(Ok(Some(paths))) => paths,
                _ => return,
            };
            if let Some(path) = paths.into_iter().next() {
                let _ = view_for_async.update(cx, |view, cx| {
                    open_story_at_path(view, path, cx);
                });
            }
        })
        .detach();
    });

    let view_for_create = view.clone();
    cx.on_action::<CreateStory>(move |_, cx| {
        let _ = view_for_create.update(cx, |view, cx| {
            view.open_new_story_dialog(cx);
        });
    });

    let view_for_reload = view.clone();
    cx.on_action::<ReloadStory>(move |_, cx| {
        let _ = view_for_reload.update(cx, |view, cx| {
            view.reload(cx);
        });
    });

    let view_for_tab = view.clone();
    cx.on_action::<SwitchTab>(move |action, cx| {
        let target = RightTab::from_index(action.tab);
        let _ = view_for_tab.update(cx, |view, cx| {
            view.set_right_tab(target, cx);
        });
    });

    let view_for_settings = view.clone();
    cx.on_action::<OpenProjectSettings>(move |_, cx| {
        let _ = view_for_settings.update(cx, |view, cx| {
            view.open_project_settings_dialog(cx);
        });
    });

    let view_for_delete = view.clone();
    cx.on_action::<DeleteProviderKey>(move |action, cx| {
        let Some(store) = cx.try_global::<KeyStoreSlot>().map(|s| s.0.clone()) else {
            return;
        };
        let provider = action.provider.clone();
        let key_id = action.key_id.clone();
        let view_for_refresh = view_for_delete.clone();
        cx.spawn(async move |cx| {
            let result = match sagaline_store::ProviderKeyId::new(provider, key_id) {
                Ok(id) => store.keys().delete(&id).map(|_| ()),
                Err(e) => Err(sagaline_store::StoreError::Other(e.to_string())),
            };
            if let Err(e) = result {
                tracing::warn!(error = %e, "delete provider key failed");
            }
            let _ = view_for_refresh.update(cx, |_view, cx| cx.notify());
        })
        .detach();
    });
    cx.on_action::<ImportProviderKeyFromFile>(move |action, cx| {
        let Some(store) = cx.try_global::<KeyStoreSlot>().map(|s| s.0.clone()) else {
            return;
        };
        let provider = action.provider.clone();
        let key_id = action.key_id.clone();
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select file containing the API key plaintext".into()),
        });
        let view_for_refresh = view.clone();
        let store_for_task = store.clone();
        // Read the bridge out of the App *before* entering the
        // spawn closure — inside, `cx` is `&mut AsyncApp`, which
        // does not implement `ReadGlobal`.
        let bridge = cx.global::<BridgeSlot>().0.clone();
        cx.spawn(async move |cx| {
            let result = receiver.await;
            let paths = match result {
                Ok(Ok(Some(paths))) => paths,
                _ => return,
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            // `tokio::fs::read` requires a Tokio runtime; route
            // through the bridge so the read runs on the agent's
            // blocking pool instead of panicking inside the gpui
            // task. Drop the gpui task -> drop the join handle ->
            // cancel the read.
            let path_for_read = path.clone();
            let bytes = match bridge
                .spawn_blocking(move || std::fs::read(&path_for_read))
                .await
            {
                Ok(Ok(b)) => b,
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, ?path, "import key: read failed");
                    return;
                }
                Err(join_err) => {
                    tracing::warn!(error = %join_err, ?path, "import key: bridge task panicked");
                    return;
                }
            };
            let plaintext = match String::from_utf8(bytes) {
                Ok(s) => s.trim().to_string(),
                Err(_) => {
                    tracing::warn!(?path, "import key: file is not UTF-8");
                    return;
                }
            };
            let id = match sagaline_store::ProviderKeyId::new(provider, key_id) {
                Ok(id) => id,
                Err(e) => {
                    tracing::warn!(error = %e, "import key: invalid id");
                    return;
                }
            };
            let secret = secrecy::SecretString::new(plaintext.into_boxed_str());
            if let Err(e) = store_for_task.keys().put(&id, &secret) {
                tracing::warn!(error = %e, "import key: store failed");
                return;
            }
            let _ = view_for_refresh.update(cx, |_view, cx| cx.notify());
        })
        .detach();
    });
}

fn open_story_at_path(
    view: &mut WorkspaceView,
    path: PathBuf,
    cx: &mut Context<WorkspaceView>,
) {
    let root = match sagaline_core::StoryRoot::new(path.clone()) {
        Ok(r) => r,
        Err(e) => {
            view.state_mut().last_error = Some(format!(
                "failed to open {}: {e}",
                path.display()
            ));
            cx.notify();
            return;
        }
    };
    let now = sagaline_core::story_root::rfc3339_now_public();
    let story = Arc::new(Story::new_public(
        StoryHandle::new(path),
        root.story_id().clone(),
        root.title().to_string(),
        now.clone(),
        now,
    ));
    view.open_story(story, cx);
}

// ---------------------------------------------------------------------------
// Banner
// ---------------------------------------------------------------------------

fn render_banner(
    state: &WorkspaceState,
    dialog_open: bool,
    settings_open: bool,
    is_onboarding: bool,
) -> gpui_kit::Div {
    let title = match state.story() {
        Some(s) => format!("Sagaline — {}", s.title),
        None => "Sagaline".to_string(),
    };

    let mut row = h_flex()
        .w_full()
        .px_4()
        .py_2()
        .border_b_1()
        .child(div().text_lg().font_semibold().child(title));

    if let Some(err) = &state.last_error {
        row = row.child(div().ml_4().text_sm().child(format!("⚠ {err}")));
    }

    let any_dialog = dialog_open || settings_open;
    if is_onboarding && !any_dialog && state.last_error.is_none() {
        // intentionally empty — onboarding explains the actions
    } else if state.graph().is_none() && state.last_error.is_none() {
        row = row.child(
            div()
                .ml_4()
                .text_sm()
                .child("⌘ N new story   ⌘ O open story   ⌘ , project settings"),
        );
    }

    row
}

// ---------------------------------------------------------------------------
// Left pane: tree
// ---------------------------------------------------------------------------

fn render_left_pane(
    graph: Option<&sagaline_core::StoryGraph>,
    tree_state: Option<&Entity<TreeState>>,
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

// ---------------------------------------------------------------------------
// Right pane: tab bar + active panel
// ---------------------------------------------------------------------------

fn render_right_pane(
    state: &WorkspaceState,
    tab: RightTab,
    log: Option<&AgentEventLog>,
    activity_count: usize,
    cx: &App,
) -> gpui_kit::Div {
    let tabs = render_tab_bar(tab, activity_count);
    let body: gpui_kit::Div = match tab {
        RightTab::Preview => render_preview(state, cx),
        RightTab::Activity => match log {
            Some(log) => crate::activity::render_activity(log),
            None => div()
                .flex_1()
                .h_full()
                .p_2()
                .child("Activity log not installed (binary mode)."),
        },
        RightTab::Keys => render_keys_panel(cx),
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
            cx.dispatch_action(&SwitchTab { tab: target });
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

/// Render the BYOK key management panel.
fn render_keys_panel(cx: &App) -> gpui_kit::Div {
    use crate::state::KeyStoreSlot;
    let mut col = v_flex().gap_2().p_3().size_full();

    col = col.child(div().text_sm().font_semibold().child("Provider API keys"));

    let store = if cx.has_global::<KeyStoreSlot>() {
        cx.global::<KeyStoreSlot>().0.clone()
    } else {
        return col.child(
            div()
                .text_xs()
                .child("Key store not installed (binary mode)."),
        );
    };

    let ids = match store.keys().list_ids() {
        Ok(ids) => ids,
        Err(e) => {
            return col.child(
                div()
                    .text_xs()
                    .child(format!("failed to list keys: {e}")),
            );
        }
    };

    if ids.is_empty() {
        col = col.child(div().text_xs().child("No keys stored yet."));
        return col;
    }

    for id in &ids {
        let row = h_flex()
            .w_full()
            .justify_between()
            .gap_2()
            .child(div().text_sm().font_family("monospace").child(id.to_string()))
            .child(
                div()
                    .id(format!("del-{}", id))
                    .text_xs()
                    .on_click({
                        let id = id.clone();
                        move |_, _, cx| {
                            cx.dispatch_action(&DeleteProviderKey {
                                provider: id.provider.clone(),
                                key_id: id.key_id.clone(),
                            });
                        }
                    })
                    .child("Delete"),
            );
        col = col.child(row);
    }

    col
}


// ---------------------------------------------------------------------------
// Right pane: preview
// ---------------------------------------------------------------------------

fn render_preview(state: &WorkspaceState, cx: &App) -> gpui_kit::Div {
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

fn find_entity<'a>(
    graph: &'a sagaline_core::StoryGraph,
    id: &StoryEntityId,
) -> Option<&'a ParsedEntity> {
    if graph.story.id.as_str() == id.0 {
        return Some(&graph.story);
    }
    graph.entity(id)
}

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

// ---------------------------------------------------------------------------
// Onboarding (empty-state) panel
// ---------------------------------------------------------------------------

fn render_onboarding(_view: Entity<WorkspaceView>, cx: &App) -> gpui_kit::Div {
    let muted = cx.theme().muted_foreground;

    let mut col = v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_4()
        .p_8();

    col = col.child(div().text_3xl().font_bold().child("Sagaline"));
    col = col.child(
        div()
            .text_base()
            .text_color(muted)
            .child("Your AI agent for structured video production."),
    );
    col = col.child(
        div()
            .text_sm()
            .text_color(muted)
            .max_w(px(560.))
            .child(
                "Sagaline is an AI agent for structured video production. Create a story, set the goal, and the agent plans and animates scene by scene.",
            ),
    );

    col = col.child(
        Button::new("onboarding-create")
            .primary()
            .label("Create new story")
            .on_click(|_, _, cx| {
                cx.dispatch_action(&CreateStory);
            }),
    );

    col = col.child(
        Button::new("onboarding-settings")
            .ghost()
            .label("Project settings")
            .on_click(|_, _, cx| {
                cx.dispatch_action(&OpenProjectSettings);
            }),
    );

    col = col.child(
        div()
            .text_xs()
            .text_color(muted)
            .child("⌘ N new story   ⌘ O open story   ⌘ , project settings"),
    );

    col
}

// ---------------------------------------------------------------------------
// New-story modal
// ---------------------------------------------------------------------------

/// Validate the inputs from the new-story dialog. The Create button
/// in the dialog is always rendered enabled, so this runs in the
/// click handler before the dialog closes. Returning `Err(msg)`
/// keeps the dialog open with the error visible.
fn validate_create_story(
    project_location_display: Option<&str>,
    title: &str,
) -> Result<(), String> {
    if project_location_display.is_none() {
        return Err("Pick a project location first (⌘ ,).".into());
    }
    if title.trim().is_empty() {
        return Err("Story title must not be empty.".into());
    }
    Ok(())
}

/// Open the new-story modal via `Window::open_dialog`. Idempotent.
#[allow(clippy::too_many_arguments)]
fn maybe_open_new_story_dialog(
    view: Entity<WorkspaceView>,
    show_dialog: bool,
    dialog_already_open: bool,
    title_input: Entity<InputState>,
    project_location_display: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    if !show_dialog {
        return;
    }
    if dialog_already_open {
        return;
    }
    if window.has_active_dialog(cx) {
        return;
    }

    let title_for_content = title_input.clone();
    let view_for_cancel = view.clone();
    let title_for_cancel = title_input.clone();
    let view_for_create = view.clone();
    let title_for_create = title_input.clone();
    let view_for_marker = view.clone();

    let view_for_error = view.clone();

    window.open_dialog(cx, move |dialog, _window, _cx| {
        // The dialog builder is `Fn + 'static` — clone per consumer.
        // The content closure needs the project location for its
        // hint text; the Create click handler needs it for
        // validation.
        let project_location_for_content = project_location_display.clone();
        let project_location_for_create = project_location_display.clone();
        let view_for_content_err = view_for_error.clone();
        let title_for_content = title_for_content.clone();
        dialog
            .title("Create a new story")
            .content(move |content, _window, cx| {
                // Read the live error from the view each render
                // instead of freezing a snapshot at dialog-open
                // time. Validation errors set after the dialog is
                // already up would never show up.
                let error_msg: Option<String> = view_for_content_err
                    .read_with(&*cx, |v, _app| {
                        v.new_story_error_msg().map(str::to_string)
                    });
                let mut content = content.child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .child("Story title"),
                        )
                        .child(Input::new(&title_for_content)),
                );
                match &project_location_for_content {
                    Some(loc) => {
                        content = content.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("A new project will be created in {loc}.")),
                        );
                    }
                    None => {
                        content = content.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    "Pick a project location to keep new stories. Press ⌘ , to set one.",
                                ),
                        );
                    }
                }
                if let Some(msg) = error_msg {
                    content = content.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().danger)
                            .child(msg),
                    );
                }
                content
            })
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("new-story-cancel")
                            .ghost()
                            .label("Cancel")
                            .on_click({
                                let view_for_cancel = view_for_cancel.clone();
                                let title_for_cancel = title_for_cancel.clone();
                                move |_, window, cx| {
                                    let _ = view_for_cancel.update(cx, |v, cx| {
                                        v.close_new_story_dialog(cx);
                                    });
                                    let _ = title_for_cancel.read(cx);
                                    window.close_dialog(cx);
                                }
                            }),
                    )
                    .child(
                        Button::new("new-story-create")
                            .primary()
                            .label("Create")
                            .on_click({
                                let view_for_create = view_for_create.clone();
                                let title_for_create = title_for_create.clone();
                                let project_location_for_click = project_location_for_create.clone();
                                move |_, window, cx| {
                                    let title = title_for_create.read(cx).value().to_string();
                                    let view_for_async = view_for_create.clone();
                                    let view_for_async_err = view_for_create.clone();
                                    // Capture the StoryService on
                                    // the foreground thread; the
                                    // async task moves it.
                                    let svc: Box<dyn StoryService> =
                                        match cx
                                            .try_global::<StoryServiceSlot>()
                                            .cloned()
                                        {
                                            Some(slot) => slot.0,
                                            None => {
                                                let _ = view_for_create.update(cx, |v, cx| {
                                                    v.set_new_story_error(
                                                        Some("Story service not available".into()),
                                                        cx,
                                                    );
                                                });
                                                window.close_dialog(cx);
                                                return;
                                            }
                                        };
                                    // Validate the dialog inputs
                                    // synchronously so the user
                                    // gets immediate feedback instead
                                    // of a silent close + reopen.
                                    if let Err(msg) = validate_create_story(
                                        project_location_for_click.as_deref(),
                                        &title,
                                    ) {
                                        let _ = view_for_create.update(cx, |v, cx| {
                                            v.set_new_story_error(Some(msg), cx);
                                        });
                                        return;
                                    }
                                    window.close_dialog(cx);
                                    let _ = view_for_create.update(cx, |v, cx| {
                                        v.set_new_story_error(None, cx);
                                    });
                                    // Route through the Tokio bridge so
                                    // the sync `StoryService::create_story`
                                    // (which touches the filesystem) runs
                                    // on the agent's blocking pool instead
                                    // of panicking inside this gpui task.
                                    let bridge = cx
                                        .global::<BridgeSlot>()
                                        .0
                                        .clone();
                                    cx.spawn(async move |_async_cx| {
                                        let res = bridge
                                            .spawn_blocking(move || {
                                                svc.create_story(&title)
                                            })
                                            .await;
                                        match res {
                                            Ok(Ok(story)) => {
                                                let _ = view_for_async.update(
                                                    _async_cx,
                                                    |v, cx| {
                                                        v.close_new_story_dialog(cx);
                                                        v.open_story(story, cx);
                                                    },
                                                );
                                            }
                                            Ok(Err(msg)) => {
                                                let _ = view_for_async_err.update(
                                                    _async_cx,
                                                    |v, cx| {
                                                        v.set_new_story_error(Some(msg), cx);
                                                    },
                                                );
                                            }
                                            Err(join_err) => {
                                                let msg = format!(
                                                    "create failed: {join_err}"
                                                );
                                                let _ = view_for_async_err.update(
                                                    _async_cx,
                                                    |v, cx| {
                                                        v.set_new_story_error(Some(msg), cx);
                                                    },
                                                );
                                            }
                                        }
                                    })
                                    .detach();
                                }
                            }),
                    ),
            )
            .on_cancel({
                let view_for_cancel = view_for_cancel.clone();
                let title_for_cancel = title_for_cancel.clone();
                move |_, window, cx| {
                    let _ = view_for_cancel.update(cx, |v, cx| {
                        v.close_new_story_dialog(cx);
                    });
                    let _ = title_for_cancel.read(cx);
                    window.close_dialog(cx);
                    true
                }
            })
            .on_close({
                let view_for_close = view_for_cancel.clone();
                let view_for_marker = view_for_marker.clone();
                move |_, _window, cx| {
                    let _ = view_for_close.update(cx, |v, cx| {
                        v.close_new_story_dialog(cx);
                    });
                    let _ = view_for_marker.update(cx, |v, cx| {
                        v.mark_new_story_dialog_opened(cx);
                    });
                }
            })
    });
}

// ---------------------------------------------------------------------------
// Project settings modal
// ---------------------------------------------------------------------------

fn maybe_open_project_settings_dialog(
    view: Entity<WorkspaceView>,
    show_dialog: bool,
    dialog_already_open: bool,
    location_display: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    if !show_dialog {
        return;
    }
    if dialog_already_open {
        return;
    }
    if window.has_active_dialog(cx) {
        return;
    }

    let view_for_close = view.clone();
    let view_for_cancel = view.clone();
    let view_for_marker = view.clone();

    window.open_dialog(cx, move |dialog, _window, _cx| {
        let location_display = location_display.clone();
        dialog
            .title("Project settings")
            .content(move |content, _window, cx| {
                let mut content = content.child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .child("Where to keep new stories"),
                );
                match &location_display {
                    Some(loc) => {
                        content = content.child(
                            div()
                                .text_sm()
                                .font_family("monospace")
                                .text_color(cx.theme().muted_foreground)
                                .child(loc.clone()),
                        );
                    }
                    None => {
                        content = content.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("Not set yet. Choose a folder to keep new stories."),
                        );
                    }
                }
                content
            })
            .footer({
                let view_for_close = view_for_close.clone();
                DialogFooter::new()
                    .child(
                        Button::new("project-settings-choose")
                            .primary()
                            .label("Choose…")
                            .on_click({
                                let view_for_close = view_for_close.clone();
                                move |_, _window, cx| {
                                    let receiver = cx.prompt_for_paths(PathPromptOptions {
                                        files: false,
                                        directories: true,
                                        multiple: false,
                                        prompt: Some(
                                            "Where would you like to keep new stories?".into(),
                                        ),
                                    });
                                    let view_for_async = view_for_close.clone();
                                    cx.spawn(async move |async_cx| {
                                        let result = receiver.await;
                                        let paths = match result {
                                            Ok(Ok(Some(paths))) => paths,
                                            _ => return,
                                        };
                                        if let Some(path) = paths.into_iter().next() {
                                            let _ = view_for_async.update(
                                                async_cx,
                                                |v, cx| {
                                                    apply_project_location(v, path, cx);
                                                },
                                            );
                                        }
                                    })
                                    .detach();
                                }
                            }),
                    )
                    .child(
                        Button::new("project-settings-cancel")
                            .ghost()
                            .label("Done")
                            .on_click({
                                let view_for_cancel = view_for_cancel.clone();
                                move |_, window, cx| {
                                    let _ = view_for_cancel.update(cx, |v, cx| {
                                        v.close_project_settings_dialog(cx);
                                    });
                                    window.close_dialog(cx);
                                }
                            }),
                    )
            })
            .on_cancel({
                let view_for_cancel = view_for_cancel.clone();
                move |_, window, cx| {
                    let _ = view_for_cancel.update(cx, |v, cx| {
                        v.close_project_settings_dialog(cx);
                    });
                    window.close_dialog(cx);
                    true
                }
            })
            .on_close({
                let view_for_close = view_for_close.clone();
                let view_for_marker = view_for_marker.clone();
                move |_, _window, cx| {
                    let _ = view_for_close.update(cx, |v, cx| {
                        v.close_project_settings_dialog(cx);
                    });
                    let _ = view_for_marker.update(cx, |v, cx| {
                        v.mark_project_settings_dialog_opened(cx);
                    });
                }
            })
    });
}

fn apply_project_location(
    view: &mut WorkspaceView,
    path: PathBuf,
    cx: &mut Context<WorkspaceView>,
) {
    if let Some(slot) = cx.try_global::<StoryServiceSlot>().cloned() {
        let svc = slot.0;
        if let Err(e) = svc.set_project_location(path.clone()) {
            view.state_mut().last_error = Some(format!("set project location failed: {e}"));
            cx.notify();
            return;
        }
        view.state_mut().set_recent_stories(svc.recent_stories());
    }
    view.state_mut()
        .set_project_location(ProjectLocation::new(path));
    cx.notify();
}

#[cfg(test)]
mod tests {
    use super::validate_create_story;

    #[test]
    fn rejects_missing_project_location() {
        let err = validate_create_story(None, "Hollow Star").unwrap_err();
        assert!(
            err.contains("project location"),
            "expected project-location hint, got: {err}"
        );
    }

    #[test]
    fn rejects_empty_title() {
        let err = validate_create_story(Some("/tmp/x"), "").unwrap_err();
        assert!(
            err.contains("empty"),
            "expected empty-title message, got: {err}"
        );
    }

    #[test]
    fn rejects_whitespace_only_title() {
        let err = validate_create_story(Some("/tmp/x"), "   ").unwrap_err();
        assert!(
            err.contains("empty"),
            "expected empty-title message for whitespace, got: {err}"
        );
    }

    #[test]
    fn accepts_well_formed_input() {
        assert!(validate_create_story(Some("/tmp/x"), "Hollow Star").is_ok());
    }

    #[test]
    fn location_check_runs_before_title_check() {
        // Regression guard: when both inputs are bad, the user
        // sees the location error first — that is the more
        // actionable fix (⌘ ,) and matches the dialog copy.
        let err = validate_create_story(None, "").unwrap_err();
        assert!(
            err.contains("project location"),
            "location error must take priority, got: {err}"
        );
    }
}
