//! Folder cover component for folder index pages
//!
//! Renders a book-open layout: cover image on the left, content on the right.
//! Delegates cover rendering to `cover::render_cover_html` for image/video/iframe support.

use crate::build::media::cover::{self, CoverType};

/// Renders the folder cover layout for a folder page.
///
/// When a cover is set, wraps the content in a two-column row with the cover
/// media on the left and the cover body on the right. The cover body contains:
///   1. An `<h1 class="moss-folder-title">` rendered via
///      `folder_title::render(label)` — the single page heading for SEO and
///      a11y outline. Shared with the no-cover folder-index path
///      (`render/html.rs`) and the synthetic folder-index path
///      (`render/blocking.rs`).
///   2. The page content (markdown body). The caller may prepend the page's
///      `byline:` rows to `content` so they land between the h1 and the body,
///      inside the cover body column.
///
/// Supports image, video, and iframe covers via the shared `render_cover_html`
/// path. When no cover is set, returns the content unchanged — the no-cover
/// h1 is added by the caller in `render/html.rs` so this helper stays purely
/// a cover-layout component.
///
/// `label` is the plain-text chrome label (typically `doc.label`); HTML
/// escaping happens inside `folder_title::render`, so do not pre-escape.
pub fn render(
    cover: Option<&str>,
    label: &str,
    content: &str,
    cover_type: CoverType,
    attrs: &moss_core::media::MediaAttrs,
    // Optional variant manifest. When `Some`, the cover image is routed
    // through `image_render::synthesize_image_html`; when `None`, the
    // legacy regex pass adds attrs to the bare `<img>`.
    media_lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    // Editor preview: annotate the heading with `data-source-fm="title"` and
    // the cover media wrapper with `data-source-fm="cover"`, so a click on
    // either points back at its frontmatter field. See `folder_title::render`.
    emit_source_fm: bool,
) -> String {
    match cover {
        Some(path) => {
            let alt = format!("{} cover", label);
            // Folder covers are typically the LCP candidate on folder
            // index pages — emit eager + fetchpriority="high" when the
            // synthesizer is in scope.
            let eager = media_lookup.is_some();
            let cover_html = cover::render_cover_html(path, cover_type, &alt, "moss-collection-cover", attrs, false, media_lookup, eager);
            // Only the media div names `cover:` — never the whole cover-row.
            // The row also holds the title and the lede, and the resolver
            // (`el.closest('[data-source-fm]')`) checks fm BEFORE
            // data-source-line, so an fm attribute on the row would swallow
            // every body-text click inside it.
            let cover_html = if emit_source_fm {
                cover_html.replacen(
                    r#"<div class="moss-collection-cover""#,
                    r#"<div class="moss-collection-cover" data-source-fm="cover""#,
                    1,
                )
            } else {
                cover_html
            };
            let folder_title_h1 = super::folder_title::render(label, emit_source_fm);
            format!(
                r#"<div class="moss-collection-cover-row">{}<div class="moss-collection-cover-body">{}{}</div></div>"#,
                cover_html,
                folder_title_h1,
                content,
            )
        }
        None => content.to_string(),
    }
}

// `description:` is metadata, not page text. It briefly rendered here as a
// visible standfirst under folder titles (f94280d00, reverted 2026-08-08): the
// reader could not tell whether the sentence was the author's or one moss
// derived from the body via the fallback chain in `build/page/meta.rs`, and
// printing words the author did not write is a surprise. It now reaches the
// reader only through `<meta>`, `og:`, `twitter:` and JSON-LD, on every page
// kind. Frontmatter that exists only to be displayed — `byline:`, `colophon:` —
// is displayed; everything else is metadata, and the body is the author's.

#[cfg(test)]
#[path = "folder_cover_tests.rs"]
mod tests;
