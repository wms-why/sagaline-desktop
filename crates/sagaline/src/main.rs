// Sagaline desktop binary — opens a gpui-kit window.
//
// Wires key bindings:
//   ⌘ O       — Open Story
//   ⌘ R       — Reload current story

use gpui_kit::component::Root;
use gpui_kit::*;

use sagaline_ui::{register_actions, WorkspaceView};

mod key_bindings {
    use gpui_kit::KeyBinding;
    use sagaline_ui::{OpenStory, ReloadStory};

    pub fn open_story() -> KeyBinding {
        KeyBinding::new("cmd-o", OpenStory, None)
    }

    pub fn reload_story() -> KeyBinding {
        KeyBinding::new("cmd-r", ReloadStory, None)
    }
}

fn main() {
    application().run(|cx| {
        gpui_kit::init(cx);

        cx.bind_keys([key_bindings::open_story(), key_bindings::reload_story()]);

        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                let view = cx.new(|cx| WorkspaceView::new_with_tree(cx));
                register_actions(view.clone(), cx);
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open window");
        })
        .detach();
    });
}