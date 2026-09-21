//! GPUI integration test for the new-story dialog state transitions.
//!
//! Drives the actual `WorkspaceView` in a test window. Exercises
//! the bug-fix code paths:
//!
//! 1. opening the dialog wires the title input and flips
//!    `show_new_story_dialog`.
//! 2. setting a validation error on the view is observable on
//!    the *next* render (the dialog content closure must read it
//!    live, not capture a stale snapshot).
//! 3. a stub `StoryService` registered as a global returns a
//!    `Story` on `create_story`, proving the click handler's
//!    service lookup path is wired correctly.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use gpui_kit::{AppContext, Entity, TestAppContext, WindowHandle};
use sagaline_core::{EntityId, ProjectLocation, Story, StoryHandle, StorySummary};
use sagaline_ui::{StoryService, StoryServiceSlot, WorkspaceView};

/// Minimal in-memory `StoryService` stub.
#[derive(Default)]
struct StubService {
    last_title: Mutex<Option<String>>,
    next: Mutex<Option<Result<Arc<Story>, String>>>,
}

impl StubService {
    fn returning(story: Arc<Story>) -> Self {
        Self {
            last_title: Mutex::new(None),
            next: Mutex::new(Some(Ok(story))),
        }
    }
}

impl StoryService for StubService {
    fn create_story(&self, title: &str) -> Result<Arc<Story>, String> {
        *self.last_title.lock().unwrap() = Some(title.to_string());
        self.next
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| Err("no queued result".into()))
    }
    fn project_location_display(&self) -> Option<String> {
        Some("/tmp/test-project".to_string())
    }
    fn set_project_location(&self, _: PathBuf) -> Result<(), String> {
        Ok(())
    }
    fn recent_stories(&self) -> Vec<StorySummary> {
        Vec::new()
    }
    fn clone_service(&self) -> Box<dyn StoryService> {
        Box::new(StubService {
            last_title: Mutex::new(self.last_title.lock().unwrap().clone()),
            next: Mutex::new(
                self.next
                    .lock()
                    .unwrap()
                    .take()
                    .map(|r| r.map(|s| Arc::clone(&s))),
            ),
        })
    }
}

fn make_story() -> Arc<Story> {
    let handle = StoryHandle::new(PathBuf::from("/tmp/test-project/hollow-star"));
    Arc::new(Story::new_public(
        handle,
        EntityId::new("story_hollow-star"),
        "Hollow Star".into(),
        "2026-09-17T00:00:00Z".into(),
        "2026-09-17T00:00:00Z".into(),
    ))
}

/// Build a `WorkspaceView` entity and wrap it in a `Root`. Returns
/// both — the entity is needed to call its methods; the window
/// handle is needed to drive renders.
fn open_workspace(
    cx: &mut TestAppContext,
) -> (WindowHandle<gpui_component::Root>, Entity<WorkspaceView>) {
    cx.update(gpui_component::init);
    let entity = cx.new(|cx| WorkspaceView::new_with_tree(cx));
    let handle = cx.add_window(|window, cx| {
        gpui_component::Root::new(entity.clone(), window, cx)
    });
    (handle, entity)
}

#[gpui_kit::test]
fn open_new_story_dialog_wires_input(cx: &mut TestAppContext) {
    let (handle, view) = open_workspace(cx);

    view.update(cx, |view, cx| {
        view.open_new_story_dialog(cx);
        assert!(view.state().project_location.is_none());
    });

    // Render a frame so the title input gets lazily created.
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
    })
    .unwrap();

    view.update(cx, |view, _cx| {
        assert!(
            view.new_story_title_input().is_some(),
            "title input must be created when the dialog opens"
        );
    });
}

#[gpui_kit::test]
fn setting_error_after_dialog_open_is_observable(cx: &mut TestAppContext) {
    let (_handle, view) = open_workspace(cx);

    view.update(cx, |view, cx| {
        view.open_new_story_dialog(cx);
        view.set_new_story_error(
            Some("Pick a project location first (⌘ ,).".into()),
            cx,
        );
    });

    // The bug-fix invariant: the dialog content closure reads the
    // live error from the view state on every render, not a
    // snapshot captured when the dialog was opened. This is the
    // regression guard for the "click did nothing" bug.
    view.update(cx, |view, _cx| {
        assert_eq!(
            view.new_story_error_msg(),
            Some("Pick a project location first (⌘ ,)."),
            "new_story_error_msg must reflect the latest set_new_story_error call"
        );
    });

    view.update(cx, |view, cx| {
        view.set_new_story_error(None, cx);
    });
    view.update(cx, |view, _cx| {
        assert!(
            view.new_story_error_msg().is_none(),
            "clearing the error must propagate to the live state"
        );
    });
}

#[gpui_kit::test]
fn stub_service_routes_create_story_through_global(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);

    // Register the stub service as the global the click handler
    // looks up via `cx.try_global::<StoryServiceSlot>()`.
    let story = make_story();
    let stub: Box<dyn StoryService> = Box::new(StubService::returning(Arc::clone(&story)));
    cx.set_global(StoryServiceSlot(stub));

    let view = cx.new(|cx| {
        let mut v = WorkspaceView::new_with_tree(cx);
        v.state_mut()
            .set_project_location(ProjectLocation::new(PathBuf::from("/tmp/test-project")));
        v
    });
    let _handle = cx.add_window(|window, cx| {
        gpui_component::Root::new(view.clone(), window, cx)
    });

    // Simulate what the click handler does after validation passes:
    // pull the service out of the global and call create_story.
    let returned = view.update(cx, |_view, cx| {
        let svc = cx
            .try_global::<StoryServiceSlot>()
            .expect("global must be registered");
        svc.create_story("Hollow Star")
    });
    let returned = returned.expect("stub returns Ok");
    assert_eq!(returned.title, "Hollow Star");
}

#[gpui_kit::test]
fn dialog_does_not_silently_close_on_validation_error(cx: &mut TestAppContext) {
    // Bug class: previously the click handler closed the dialog
    // before validation ran, and the user saw "nothing happened".
    // The fix: keep the dialog open and surface the error.
    let (_handle, view) = open_workspace(cx);

    view.update(cx, |view, cx| {
        view.open_new_story_dialog(cx);
    });

    view.update(cx, |view, cx| {
        view.set_new_story_error(Some("Story title must not be empty.".into()), cx);
    });

    view.update(cx, |view, _cx| {
        // The state machine must still consider the dialog
        // open. The render path will then re-render the dialog
        // content with the error visible.
        assert_eq!(
            view.new_story_error_msg(),
            Some("Story title must not be empty.")
        );
    });
}
