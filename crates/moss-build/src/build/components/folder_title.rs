//! Folder-index page heading helper.
//!
//! Emits the single `<h1 class="moss-folder-title">` shared by all three
//! folder-index render paths:
//!   1. Explicit folder index WITH cover — rendered inside `.moss-collection-cover-body`
//!      by `folder_cover::render`.
//!   2. Explicit folder index WITHOUT cover — prepended to content in `render/html.rs`.
//!   3. Synthetic folder index (no `.md` source) — prepended to the auto-generated
//!      listing in `render/blocking.rs`.
//!
//! Index pages always carry exactly one `<h1>` after this consolidation, restoring
//! document-outline parity with article pages.

use crate::build::features::html_escape;

/// Render the folder-index page heading. `label` is the plain-text page title
/// (typically `doc.label` or the folder name); it is HTML-escaped here.
///
/// `emit_source_fm` mirrors the article heading (`inject_article_title_h1`):
/// in an editor preview the heading carries `data-source-fm="title"` so a
/// click on it points back at the `title` field. Pass `false` where no
/// markdown source stands behind the heading — a synthetic folder index has no
/// file to point at. Stripped from shipped HTML by `build::ship`.
///
/// `visually_hidden` adds `.visually-hidden` (site.css's Base & Reset) to the
/// class list rather than suppressing the heading outright — a places
/// explorer root (design decision 7, "the map is the page") draws its own
/// title nowhere visible, but a sighted layout is not the only reader: an
/// `<h1>` absent from the page entirely leaves screen-reader navigation with
/// no landmark at all.
pub fn render(label: &str, emit_source_fm: bool, visually_hidden: bool) -> String {
    let fm_attr = if emit_source_fm {
        r#" data-source-fm="title""#
    } else {
        ""
    };
    let class = if visually_hidden {
        "moss-folder-title visually-hidden"
    } else {
        "moss-folder-title"
    };
    format!(
        r#"<h1 class="{}"{}>{}</h1>"#,
        class,
        fm_attr,
        html_escape(label)
    )
}

#[cfg(test)]
#[path = "folder_title_tests.rs"]
mod tests;
