//! Cross-section formatting and shared UI helpers used by the workspace
//! views. Kept free-standing so per-section modules can compose them
//! without dragging in each other's impls.

use gpui_kit::component::Size;
use gpui_kit::component::avatar::{Avatar, AvatarGroup};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::*;
use gpui_kit::*;
use sagaline_core::model::{ChapterStatus, ReferenceImage};

use super::WorkspaceView;

/// One-line explanation of what each chapter status means. Surfaced as a
/// hover tooltip on chapter list rows and the chapter detail header.
pub fn chapter_status_tooltip(s: ChapterStatus) -> std::borrow::Cow<'static, str> {
    match s {
        ChapterStatus::Draft => t!("chapter_status.tooltip_draft"),
        ChapterStatus::Planning => t!("chapter_status.tooltip_planning"),
        ChapterStatus::Generating => t!("chapter_status.tooltip_generating"),
        ChapterStatus::Generated => t!("chapter_status.tooltip_generated"),
        ChapterStatus::Published => t!("chapter_status.tooltip_published"),
    }
}

/// Build a compact, colored Tag for the given chapter status. Returned as
/// `AnyElement` so it slots into `.children(iter.map(...))` builders.
pub fn chapter_status_tag(s: ChapterStatus) -> AnyElement {
    let label = match s {
        ChapterStatus::Draft => t!("chapter_status.draft"),
        ChapterStatus::Planning => t!("chapter_status.planning"),
        ChapterStatus::Generating => t!("chapter_status.generating"),
        ChapterStatus::Generated => t!("chapter_status.generated"),
        ChapterStatus::Published => t!("chapter_status.published"),
    };
    let tag = match s {
        ChapterStatus::Draft => Tag::secondary(),
        ChapterStatus::Planning => Tag::info(),
        ChapterStatus::Generating => Tag::warning(),
        ChapterStatus::Generated => Tag::success(),
        ChapterStatus::Published => Tag::primary(),
    };
    tag.rounded_full()
        .with_size(Size::Small)
        .child(label)
        .into_any_element()
}

/// Map a reference source string to an `ImageSource`. URLs (http/https/file)
/// become `Resource::Uri`; everything else becomes `Resource::Path`. `None`
/// when the source string is empty so callers can decide to fall back.
pub fn image_source_for(source: &str) -> Option<gpui_kit::ImageSource> {
    if source.is_empty() {
        return None;
    }
    if source.starts_with("http://") || source.starts_with("https://") || source.starts_with("file://") {
        Some(gpui_kit::ImageSource::Resource(gpui_kit::Resource::Uri(
            source.to_string().into(),
        )))
    } else {
        Some(gpui_kit::ImageSource::Resource(gpui_kit::Resource::Path(
            std::path::PathBuf::from(source).into(),
        )))
    }
}

/// Build the avatar row / avatar-group element used inside character age,
/// environment, and prop cards. Below 4 refs we render a flat row; above
/// we use `AvatarGroup` with a +N ellipsis.
pub fn render_reference_avatars(
    refs: &[ReferenceImage],
    _cx: &mut Context<WorkspaceView>,
) -> AnyElement {
    if refs.is_empty() {
        return div().into_any_element();
    }
    let label_for = |r: &ReferenceImage| -> Avatar {
        let mut av = Avatar::new().name(r.label.clone());
        if let Some(src) = image_source_for(&r.source) {
            av = av.src(src);
        }
        av
    };
    if refs.len() > 3 {
        let group = AvatarGroup::new()
            .limit(3)
            .ellipsis()
            .children(refs.iter().map(label_for));
        return group.into_any_element();
    }
    let row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .children(refs.iter().map(label_for));
    row.into_any_element()
}

/// Human-readable label for a character reference inside a shot summary
/// pill: `<name>·<age><appearance>`. Used by both the Overview concept
/// tree and the Chapters shot detail.
pub fn format_char_ref_label(name: &str, age_label: &str, appearance_name: &str) -> String {
    format!("{}·{}{}", name, age_label, appearance_name)
}

/// Compact `ChNN·SceneNN` label for environment / chapter cross-references.
/// Two-digit zero-padding keeps the references column-aligned.
pub fn format_chapter_scene_ref(chapter_number: u32, scene_number: u32) -> String {
    format!("Ch{:02}·Scene{:02}", chapter_number, scene_number)
}