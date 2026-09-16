//! Top-level gpui view for an open story.
//!
//! Two-pane layout with a tabbed right pane:
//!
//! - **Left:** a `gpui_base::Tree` widget showing the story hierarchy.
//! - **Right:** tabbed. The default **Preview** tab shows the
//!   selected entity's YAML front matter + Markdown body. The
//!   **Activity** tab shows a live scrollback of the agent's
//!   `AgentEvent` stream (read from the [`AgentEventLog`] global).
//!
//! The view emits a [`StoryOpened`] event every time a story
//! directory is successfully loaded; the app shell subscribes via
//! `cx.subscribe(&view, ...)` to kick off the agent loop.

use std::path::PathBuf;

use gpui_base::{StyledExt, Tree, TreeItem, TreeState, h_flex, v_flex};
use gpui_kit::*;

use crate::actions::{OpenStory, ReloadStory};
use crate::activity::AgentEventLog;
use crate::state::WorkspaceState;
use sagaline_core::{EntityId as StoryEntityId, EntityType as StoryEntityType, ParsedEntity};

/// Emitted by [`WorkspaceView`] after a successful `open_story` /
/// `reload`. The app shell uses this to trigger the agent loop.
#[derive(Debug, Clone)]
pub struct StoryOpened {
    pub path: PathBuf,
}

/// Which tab is active in the right pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightTab {
    #[default]
    Preview,
    Activity,
}

/// Top-level view. Holds [`WorkspaceState`] directly; mutations call
/// `cx.notify()` to trigger a redraw.
pub struct WorkspaceView {
    state: WorkspaceState,
    tree_state: Option<Entity<TreeState>>,
    right_tab: RightTab,
}

impl EventEmitter<StoryOpened> for WorkspaceView {}

impl WorkspaceView {
    pub fn new() -> Self {
        Self {
            state: WorkspaceState::new(),
            tree_state: None,
            right_tab: RightTab::default(),
        }
    }

    pub fn new_with_tree(cx: &mut App) -> Self {
        let tree_state = cx.new(|cx| TreeState::new(cx));
        Self {
            state: WorkspaceState::new(),
            tree_state: Some(tree_state),
            right_tab: RightTab::default(),
        }
    }

    pub fn state(&self) -> &WorkspaceState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut WorkspaceState {
        &mut self.state
    }

    /// Currently-active right-pane tab.
    pub fn right_tab(&self) -> RightTab {
        self.right_tab
    }

    /// Switch the right pane to a different tab.
    pub fn set_right_tab(&mut self, tab: RightTab, cx: &mut Context<Self>) {
        if self.right_tab != tab {
            self.right_tab = tab;
            cx.notify();
        }
    }

    /// Open a story at the given path and rebuild the tree. Emits
    /// [`StoryOpened`] on success.
    pub fn open_story(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let was_loaded = self.state.graph().is_some();
        self.state.open_story(path.clone());
        self.rebuild_tree(cx);
        cx.notify();
        if self.state.graph().is_some() && !was_loaded {
            cx.emit(StoryOpened { path });
        }
    }

    /// Re-walk the currently-open story directory and refresh the
    /// graph. No-op if no story is open.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.state.root_path_owned() {
            self.state.reload();
            self.rebuild_tree(cx);
            cx.notify();
            cx.emit(StoryOpened { path });
        }
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let banner = render_banner(&self.state);

        // Sync workspace selection from the tree's selected item. The
        // Tree widget owns its click handler; we observe the resulting
        // selection each render.
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

        let body = h_flex()
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
            ));

        v_flex().size_full().child(banner).child(body)
    }
}

// ---------------------------------------------------------------------------
// Tree building
// ---------------------------------------------------------------------------

/// Build the `TreeItem` hierarchy from a `StoryGraph`.
///
/// Layout (mirrors the on-disk directory layout):
///
/// ```text
/// story
/// ├─ Bible (folder, expanded) — bibles
/// ├─ Characters (folder) — characters
/// ├─ Environments (folder)
/// ├─ Props (folder)
/// └─ Chapters (folder)
///     └─ <chapter-slug> (sub-folder)
///         └─ Scenes
///             └─ <scene-slug>
/// ```
fn build_tree_items(graph: Option<&sagaline_core::StoryGraph>) -> Vec<TreeItem> {
    let Some(g) = graph else {
        return Vec::new();
    };

    let mut root_items: Vec<TreeItem> = Vec::new();

    root_items.push(TreeItem::new(
        g.story.id.0.clone(),
        format!("Story · {}", g.story.slug),
    ));
    let chapters = g.entities_by_type(StoryEntityType::Chapter);
    let sections: &[(StoryEntityType, &str)] = &[
        (StoryEntityType::Bible, "Bible"),
        (StoryEntityType::Character, "Characters"),
        (StoryEntityType::Environment, "Environments"),
        (StoryEntityType::Prop, "Props"),
        (StoryEntityType::Chapter, "Chapters"),
    ];

    for (ty, label) in sections.iter() {
        if matches!(ty, StoryEntityType::Shot) {
            continue;
        }
        let entries = g.entities_by_type(*ty);
        if entries.is_empty() {
            continue;
        }

        let folder = match ty {
            StoryEntityType::Chapter => {
                let mut folder =
                    TreeItem::new(format!("__folder_{label}"), label.to_string()).expanded(true);
                for ch in &chapters {
                    let scenes_under = g.entities_by_type(StoryEntityType::Scene);
                    let scenes_for_chapter: Vec<_> = scenes_under
                        .into_iter()
                        .filter(|s| {
                            s.path
                                .starts_with(format!("chapters/{}/", ch.slug).as_str())
                        })
                        .collect();
                    if scenes_for_chapter.is_empty() {
                        folder = folder.child(TreeItem::new(
                            ch.id.0.clone(),
                            format!("  {}", ch.slug),
                        ));
                        continue;
                    }
                    folder = folder.child(TreeItem::new(
                        ch.id.0.clone(),
                        format!("  {}", ch.slug),
                    ));
                    let mut scenes_folder = TreeItem::new(
                        format!("__scenes_{}", ch.slug),
                        format!("  Scenes in {}", ch.slug),
                    )
                    .expanded(true);
                    for sc in scenes_for_chapter {
                        scenes_folder = scenes_folder.child(TreeItem::new(
                            sc.id.0.clone(),
                            format!("    {}", sc.slug),
                        ));
                    }
                    folder = folder.child(scenes_folder);
                }
                folder
            }
            _ => {
                let mut folder =
                    TreeItem::new(format!("__folder_{label}"), label.to_string()).expanded(true);
                for e in &entries {
                    folder = folder.child(TreeItem::new(
                        e.id.0.clone(),
                        format!("  {}", e.slug),
                    ));
                }
                folder
            }
        };

        root_items.push(folder);
    }

    root_items
}

// ---------------------------------------------------------------------------
// Action wiring
// ---------------------------------------------------------------------------

pub fn register_actions(view: Entity<WorkspaceView>, cx: &mut App) {
    let view_for_open = view.clone();
    cx.on_action::<OpenStory>(move |_, cx| {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open story directory".into()),
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
                    view.open_story(path, cx);
                });
            }
        })
        .detach();
    });

    cx.on_action::<ReloadStory>(move |_, cx| {
        let _ = view.update(cx, |view, cx| {
            view.reload(cx);
        });
    });
}

// ---------------------------------------------------------------------------
// Banner
// ---------------------------------------------------------------------------

fn render_banner(state: &WorkspaceState) -> gpui_kit::Div {
    let title = match state.graph() {
        Some(g) => match g
            .story
            .frontmatter
            .get("title")
            .and_then(serde_yaml::Value::as_str)
        {
            Some(t) => format!("Sagaline — {t}"),
            None => format!("Sagaline — {}", g.story.id),
        },
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

    if state.graph().is_none() && state.last_error.is_none() {
        row = row.child(div().ml_4().text_sm().child("⌘ O to open a story"));
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
) -> gpui_kit::Div {
    let tabs = render_tab_bar(tab, activity_count);
    let body: gpui_kit::Div = match tab {
        RightTab::Preview => render_preview(state),
        RightTab::Activity => match log {
            Some(log) => crate::activity::render_activity(log),
            None => div()
                .flex_1()
                .h_full()
                .p_2()
                .child("Activity log not installed (binary mode)."),
        },
    };
    v_flex().flex_1().h_full().child(tabs).child(body)
}

fn render_tab_bar(active: RightTab, activity_count: usize) -> gpui_kit::Div {
    let preview_weight = if active == RightTab::Preview {
        FontWeight::BOLD
    } else {
        FontWeight::NORMAL
    };
    let activity_weight = if active == RightTab::Activity {
        FontWeight::BOLD
    } else {
        FontWeight::NORMAL
    };
    let label_activity = if activity_count == 0 {
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
        .child(
            div()
                .text_sm()
                .font_weight(preview_weight)
                .child("Preview"),
        )
        .child(
            div()
                .text_sm()
                .font_weight(activity_weight)
                .child(label_activity),
        )
}

// ---------------------------------------------------------------------------
// Right pane: preview
// ---------------------------------------------------------------------------

fn render_preview(state: &WorkspaceState) -> gpui_kit::Div {
    let inner: gpui_kit::Div = match (state.graph(), state.selected.as_ref()) {
        (Some(graph), Some(sel)) => match find_entity(graph, sel) {
            Some(e) => render_entity_preview(e),
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

fn render_entity_preview(e: &ParsedEntity) -> gpui_kit::Div {
    let mut col = v_flex().gap_3().size_full();

    col = col.child(
        div()
            .text_lg()
            .font_semibold()
            .child(format!("{} · {}", e.type_.as_str(), e.slug)),
    );

    let yaml = serde_yaml::to_string(&e.frontmatter).unwrap_or_default();
    col = col.child(
        v_flex()
            .gap_1()
            .child(div().text_xs().font_semibold().child("front matter"))
            .child(
                div()
                    .text_sm()
                    .font_family("monospace")
                    .p_2()
                    .rounded_sm()
                    .child(yaml),
            ),
    );

    if !e.body.trim().is_empty() {
        col = col.child(
            v_flex()
                .gap_1()
                .child(div().text_xs().font_semibold().child("body"))
                .child(
                    div()
                        .text_sm()
                        .p_2()
                        .rounded_sm()
                        .child(e.body.clone()),
                ),
        );
    }

    col
}
