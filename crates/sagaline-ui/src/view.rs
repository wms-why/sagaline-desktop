//! Top-level workspace view and shell. Per-section render functions live
//! in the sibling modules under `src/view/`. The shell owns:
//!   - `WorkspaceView` struct + `new` + `Render` impl (incl. Cmd+1..6
//!     keyboard navigation)
//!   - `render_sidebar` — the navigation rail
//!   - `render_content` — the two-tier header / scroll-area layout
//!   - `render_header` — title + section label + continue button
//!
//! Submodules attach additional `impl WorkspaceView` blocks; methods on
//! the same struct resolve across files normally.

pub mod characters;
pub mod chapters;
pub mod environments;
pub mod format;
pub mod overview;
pub mod props;
pub mod references;
pub mod story_bible;

use gpui_kit::component::button::*;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::InputState;
use gpui_kit::component::sidebar::*;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::*;
use gpui_kit::*;
use sagaline_core::cmd::Cmd;

use crate::state::{Section, WorkspaceState};

/// Top-level workspace view, owned by the gpui window.
pub struct WorkspaceView {
    pub state: WorkspaceState,
    /// Lazily-created text input for the "New story" dialog. We can't
    /// construct it in `new()` because `InputState::new` requires `&mut Window`
    /// and `&mut Context<Self>`, neither of which is available there. The
    /// first `Render::render` call creates it and caches the entity here.
    new_story_input: Option<gpui::Entity<InputState>>,
}

impl WorkspaceView {
    pub fn new(state: WorkspaceState) -> Self {
        Self {
            state,
            new_story_input: None,
        }
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Lazy-init the dialog's input entity on first render — InputState::new
        // needs `&mut Window` and `&mut Context<Self>`, which we only have
        // here. The same entity is shared by the dialog's content builder
        // and read by the OK handler to dispatch `CreateStory`.
        if self.new_story_input.is_none() {
            self.new_story_input =
                Some(cx.new(|cx| InputState::new(window, cx).placeholder(t!("dialog.new_story_placeholder"))));
        }

        // Two-column layout: navigation rail + content pane.
        div()
            .id("workspace-root")
            .size_full()
            .flex()
            .flex_row()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, _, cx| {
                let ks = &event.keystroke;
                if !ks.modifiers.platform
                    || ks.modifiers.shift
                    || ks.modifiers.alt
                    || ks.modifiers.control
                {
                    return;
                }
                let idx = match ks.key.as_str() {
                    "1" => 0,
                    "2" => 1,
                    "3" => 2,
                    "4" => 3,
                    "5" => 4,
                    "6" => 5,
                    _ => return,
                };
                if let Some(section) = Section::ALL.get(idx) {
                    view.state.select_section(*section);
                    cx.notify();
                }
            }))
            .child(self.render_sidebar(window, cx))
            .child(div().flex_1().size_full().child(self.render_content(cx)))
    }
}

// ---------------------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------------------

/// Build the sidebar row used for every section that exposes a count:
/// `Characters · N`, `Chapters · N`, etc. Centralised because all five
/// rows have lockstep "label · count + click to select section" shape.
fn counted_section_item(
    cx: &mut Context<WorkspaceView>,
    label: std::borrow::Cow<'static, str>,
    count: usize,
    section: Section,
) -> SidebarMenuItem {
    SidebarMenuItem::new(format!("{label} · {count}"))
        .on_click(cx.listener(move |view, _, _, _| view.state.select_section(section)))
}

impl WorkspaceView {
    fn render_sidebar(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // `cx` in scope.
        let view_entity = cx.entity();
        let input = self
            .new_story_input
            .clone()
            .expect("new_story_input initialized in render()");
        let cmd_tx = self.state.cmd_tx.clone();

        // Stories group
        let stories: Vec<_> = self
            .state
            .workspace
            .story
            .iter()
            .map(|s| s.title.clone())
            .collect();
        let selected_story_id = self.state.selected_story_id.clone();
        let active_section = self.state.section;

        let story_label = if let Some(s) = &self.state.workspace.story {
            t!("sidebar.with_emoji", title = s.title.clone()).into_owned()
        } else {
            t!("sidebar.no_story").into_owned()
        };

        let story_item = SidebarMenuItem::new(story_label)
            .active(selected_story_id.is_some())
            .children([
                SidebarMenuItem::new(t!("sidebar.story_item_overview")).on_click(cx.listener(
                    |view, _, _, _| view.state.select_section(Section::Overview),
                )),
                SidebarMenuItem::new(t!("sidebar.story_item_story_bible")).on_click(cx.listener(
                    |view, _, _, _| view.state.select_section(Section::StoryBible),
                )),
                counted_section_item(
                    cx,
                    t!("sidebar.story_item_characters"),
                    self.state.workspace.characters.len(),
                    Section::Characters,
                ),
                counted_section_item(
                    cx,
                    t!("sidebar.story_item_environments"),
                    self.state.workspace.environments.len(),
                    Section::Environments,
                ),
                counted_section_item(
                    cx,
                    t!("sidebar.story_item_props"),
                    self.state.workspace.props.len(),
                    Section::Props,
                ),
                counted_section_item(
                    cx,
                    t!("sidebar.story_item_references"),
                    self.collect_references().len(),
                    Section::References,
                ),
                counted_section_item(
                    cx,
                    t!("sidebar.story_item_chapters"),
                    self.state.workspace.chapters.len(),
                    Section::Chapters,
                ),
            ])
            .default_open(true);

        // Highlight the active section. We mark the matching child active.
        // (SidebarMenuItem.active() controls visual emphasis.)
        let _ = (stories, active_section); // suppress unused warnings; used in render_content

        let new_story_item = SidebarMenuItem::new(t!("sidebar.new_story")).on_click(
            move |_event, window, cx| {
                // Both closures are `Fn` (re-rendered / re-invoked), so any
                // non-`Copy` capture has to be cloned at every boundary — the
                // outer on-click handler and the dialog builder are both
                // expected to be callable many times.
                let view_entity = view_entity.clone();
                let input = input.clone();
                let cmd_tx = cmd_tx.clone();
                window.open_alert_dialog(cx, move |alert, _window, _cx| {
                    let view_entity = view_entity.clone();
                    let input = input.clone();
                    let cmd_tx = cmd_tx.clone();
                    alert
                        .title(t!("dialog.new_story_title"))
                        .description(t!("dialog.new_story_description"))
                        .show_cancel(true)
                        .button_props(
                            DialogButtonProps::default()
                                .ok_text(t!("dialog.new_story_create"))
                                .cancel_text(t!("dialog.new_story_cancel"))
                                .on_ok(move |_event, _window, cx| {
                                    // The OK callback only sees `&mut App`, so we
                                    // hop through `view_entity` to read the title
                                    // and dispatch the create command on the view.
                                    let raw = view_entity
                                        .read_with(cx, |view, _cx| {
                                            view.new_story_input
                                                .as_ref()
                                                .map(|i| i.read_with(cx, |s, _cx| s.value().to_string()))
                                                .unwrap_or_default()
                                        });
                                    let title = raw.trim().to_string();
                                    if title.is_empty() {
                                        return false;
                                    }
                                    let (reply_tx, reply_rx) =
                                        futures::channel::oneshot::channel();
                                    if cmd_tx
                                        .unbounded_send(Cmd::CreateStory {
                                            title,
                                            reply: reply_tx,
                                        })
                                        .is_err()
                                    {
                                        return true;
                                    }
                                    let view_id = view_entity.entity_id();
                                    // Clone for the inner spawn closure — the
                                    // outer `on_ok` is `Fn`, so we can't move
                                    // out of its captures.
                                    let spawn_view_entity = view_entity.clone();
                                    let spawn_input = input.clone();
                                    cx.spawn(async move |cx| {
                                        if let Ok(Ok(story)) = reply_rx.await {
                                            let _ = spawn_view_entity.update(cx, |view, cx| {
                                                let _ = view.state.select_story(story.id);
                                                cx.notify();
                                            });
                                            // Clear the input via the view's window.
                                            let _ = cx.with_window(view_id, |window, cx| {
                                                let _ = spawn_input.update(cx, |s, cx| {
                                                    s.set_value("", window, cx);
                                                });
                                            });
                                        }
                                    })
                                    .detach();
                                    true
                                }),
                        )
                });
            },
        );

        Sidebar::new("workspace-nav")
            .w(px(280.))
            .child(
                SidebarGroup::new(t!("sidebar.my_stories")).child(
                    SidebarMenu::new()
                        .child(new_story_item)
                        .child(SidebarMenuItem::new(t!("sidebar.stories")))
                        .child(story_item),
                ),
            )
    }
}

// ---------------------------------------------------------------------------
// Content pane
// ---------------------------------------------------------------------------

impl WorkspaceView {
    fn render_content(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.render_header(cx);
        let body: AnyElement = match self.state.section {
            Section::Overview => self.render_overview(cx).into_any_element(),
            Section::StoryBible => self.render_story_bible(cx).into_any_element(),
            Section::Characters => self.render_characters(cx).into_any_element(),
            Section::Environments => self.render_environments(cx).into_any_element(),
            Section::Props => self.render_props(cx).into_any_element(),
            Section::References => self.render_references(cx).into_any_element(),
            Section::Chapters => self.render_chapters(cx).into_any_element(),
        };

        // Two-tier layout:
        //
        //   ┌────────────────────────────────────────┐
        //   │ header (natural height, fixed top)     │
        //   ├────────────────────────────────────────┤
        //   │ body container:                        │
        //   │   .flex_1()      ── takes remaining    │
        //   │   .overflow_hidden ── drops its        │
        //   │      content-based automatic min-size  │
        //   │   so the inner scroll area can shrink  │
        //   │   below the body height                │
        //   │   ┌────────────────────────────────┐   │
        //   │   │ scroll area: .size_full()     │   │
        //   │   │   .overflow_y_scrollbar()     │   │
        //   │   │   .p_6()                      │   │
        //   │   │   ┌────────────────────────┐  │   │
        //   │   │   │  body (long content)   │  │   │
        //   │   │   └────────────────────────┘  │   │
        //   │   └────────────────────────────────┘   │
        //   └────────────────────────────────────────┘
        //
        // The split between "container with overflow_hidden" and "scroll
        // area inside it" mirrors the pattern used by gpui-component's
        // Dialog / Sheet internals. Without overflow_hidden on the
        // container, the inner content's intrinsic min-size fights the
        // scroll and the area refuses to shrink; without flex_1 + size_full
        // on the scroll area, the scroll viewport collapses to the
        // content's height and never reports overflow.
        div()
            .size_full()
            .v_flex()
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .size_full()
                            .overflow_y_scrollbar()
                            .p_6()
                            .child(body),
                    ),
            )
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self
            .state
            .workspace
            .story
            .as_ref()
            .map(|s| s.title.clone())
            .unwrap_or_else(|| t!("header.fallback_title").to_string());

        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px_6()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_semibold()
                            .child(title),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.state.section.label()),
                    ),
            )
            .child(
                Button::new("new-shot")
                    .primary()
                    .label(t!("header.continue_story"))
                    .on_click(|_, _, _| {
                        println!("Continue story — model adapters come online later");
                    }),
            )
    }
}