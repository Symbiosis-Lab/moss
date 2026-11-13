//! Build-time warning for author-typed raw `<img>`/`<video>`/`<audio>` HTML in markdown.
//!
//! # Why this exists
//!
//! Raw HTML in markdown source is opaque to moss: pulldown-cmark passes it
//! through verbatim and the synthesizer never sees it. The author silently
//! loses LQIP, intrinsic dimensions, and lazy-load attribute injection (no
//! post-pass adds them to the emitted HTML). This module is the only mechanism
//! that surfaces that to the author so they can convert to
//! `![alt](src)` / `![[file]]` syntax and opt back into moss enhancement.
//!
//! # Behavior
//!
//! [`raw_media_tags`] is a pure scan over markdown source that returns one
//! tag per `<img>`/`<video>`/`<audio>` tag-start. Detection is
//! case-insensitive on tag name; attributes are not inspected. Markdown image
//! syntax (`![alt](src)`) and Obsidian-style embeds (`![[file]]`) never match,
//! and neither does a tag inside code or an HTML comment, where it is text.
//!
//! [`scan_for_raw_media_warnings`] is the I/O-shaped caller-facing wrapper:
//! it runs the detector and emits one `log::warn!` line per detection. No
//! dedup (an author with five raw `<img>` tags gets five lines — each is a
//! distinct case worth nudging).
//!
//! Fire-and-forget: the scanner only logs; it never affects the build output.

use std::path::Path;

use moss_core::resolve::authored_assets::raw_media_tags;

/// Detect raw `<img>` / `<video>` tags in markdown source and emit one
/// build-time `log::warn!` per occurrence. The warning names the page and
/// the tag, and points the author at the markdown image syntax that opts
/// back into moss enhancement.
///
/// Fire-and-forget; never blocks or alters the build output.
pub fn scan_for_raw_media_warnings(markdown: &str, page_path: &Path) {
    for w in raw_media_tags(markdown) {
        log::warn!(
            "Page {}: raw <{}> tag at byte {} - moss won't enhance this (no LQIP/dims/lazy-load). \
             Use ![alt](src) or ![[file]] for moss-managed assets.",
            page_path.display(),
            w.tag,
            w.byte_offset,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn scan_does_not_panic_on_empty_input() {
        // The log-only wrapper should be a no-op for empty / clean markdown.
        scan_for_raw_media_warnings("", &PathBuf::from("test.md"));
        scan_for_raw_media_warnings("just text", &PathBuf::from("test.md"));
        scan_for_raw_media_warnings(
            "![alt](photo.jpg)",
            &PathBuf::from("articles/post.md"),
        );
    }
}
