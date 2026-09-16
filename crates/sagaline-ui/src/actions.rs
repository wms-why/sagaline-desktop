//! Action definitions for sagaline-ui.
//!
//! [`OpenStory`] and [`ReloadStory`] are dispatched by menu items,
//! key shortcuts, and code paths from elsewhere in the app. The view
//! layer binds them via `cx.on_action(...)`.

gpui_kit::actions!(sagaline, [OpenStory, ReloadStory]);
