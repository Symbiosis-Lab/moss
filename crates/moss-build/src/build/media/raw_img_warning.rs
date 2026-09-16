//! Build-time warning for author-typed raw `<img>`/`<video>`/`<audio>` HTML in markdown.
//!
//! # Why this exists
//!
//! Phase 2 of the unified-image-emission migration retires
//! `build/media/placeholder.rs`, the regex post-pass that injected
//! `data-placeholder-src`, `width`/`height`, and `loading="lazy"` onto every
//! `<img>`/`<video>` element in the emitted HTML — including raw HTML tags an
//! author hand-typed in their markdown source (one of the four documented
//! placeholder.rs carve-outs).
//!
//! After Phase 2E deletes `placeholder.rs`, raw HTML in markdown source becomes
//! fully opaque to moss: pulldown-cmark passes it through verbatim and the
//! synthesizer never sees it. The author loses LQIP, intrinsic dimensions, and
//! lazy-load attribute injection silently. This module is the only mechanism
//! that surfaces the regression to the author so they can convert to
//! `![alt](src)` / `![[file]]` syntax and opt back into moss enhancement.
//!
//! # Behavior
//!
//! [`detect_raw_media`] is a pure scan over markdown source that returns one
//! [`RawMediaWarning`] per `<img>`/`<video>`/`<audio>` tag-start. Detection is
//! case-insensitive on tag name; attributes are not inspected. Markdown image
//! syntax (`![alt](src)`) and Obsidian-style embeds (`![[file]]`) never match.
//!
//! [`scan_for_raw_media_warnings`] is the I/O-shaped caller-facing wrapper:
//! it runs the detector and emits one `log::warn!` line per detection. No
//! dedup (an author with five raw `<img>` tags gets five lines — each is a
//! distinct case worth nudging).
//!
//! Fire-and-forget: the scanner only logs; it never affects the build output.

use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

/// One occurrence of an author-typed raw `<img>`, `<video>` or `<audio>` tag.
///
/// Returned by [`detect_raw_media`] so callers (and tests) can reason about
/// detections without coupling to the logging side effect.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RawMediaWarning {
    /// Lowercase tag name: `"img"`, `"video"` or `"audio"`.
    pub tag: String,
    /// Byte offset of the `<` character in the source markdown.
    pub byte_offset: usize,
}

/// Compiled once per process; cheap to reuse across every page in a build.
fn raw_media_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Match `<img`, `<video` or `<audio` followed by whitespace, `>`, or
        // `/` (self-closing). The trailing class anchors the boundary so we
        // never match `<imgur>` or `<videos>` style identifiers as raw media
        // tags.
        //
        // `audio` is here because `![[track.mp3]]` synthesizes a player
        // (`SynthKind::Audio`), so a hand-typed `<audio>` bypasses moss for
        // the same reason the other two do. It was omitted until 2026-08-06
        // while the shipped authoring guidance already promised the warning
        // for all three — the doc was right about the intent and the scanner
        // was the one out of step.
        Regex::new(r"(?i)<(img|video|audio)([\s/>])").expect("static raw-media regex compiles")
    })
}

/// Scan markdown source for raw `<img>` / `<video>` / `<audio>` tag-starts. Pure: no I/O,
/// no logging. Returns an empty vec when the markdown only uses
/// `![alt](src)` / `![[file]]` syntax.
pub fn detect_raw_media(markdown: &str) -> Vec<RawMediaWarning> {
    let re = raw_media_regex();
    re.captures_iter(markdown)
        .filter_map(|cap| {
            let whole = cap.get(0)?;
            let tag = cap.get(1)?;
            Some(RawMediaWarning {
                tag: tag.as_str().to_ascii_lowercase(),
                byte_offset: whole.start(),
            })
        })
        .collect()
}

/// Detect raw `<img>` / `<video>` tags in markdown source and emit one
/// build-time `log::warn!` per occurrence. The warning names the page and
/// the tag, and points the author at the markdown image syntax that opts
/// back into moss enhancement.
///
/// Fire-and-forget; never blocks or alters the build output.
pub fn scan_for_raw_media_warnings(markdown: &str, page_path: &Path) {
    for w in detect_raw_media(markdown) {
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
    fn detects_raw_img_tag() {
        let md = "some text <img src=\"x.jpg\"> more text";
        let hits = detect_raw_media(md);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tag, "img");
        assert_eq!(hits[0].byte_offset, "some text ".len());
    }

    #[test]
    fn detects_video_tag() {
        let md = "<video src=\"x.mp4\">";
        let hits = detect_raw_media(md);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tag, "video");
        assert_eq!(hits[0].byte_offset, 0);
    }

    /// `![[track.mp3]]` synthesizes an audio player, so a hand-typed
    /// `<audio>` loses that enhancement exactly as a raw `<img>` loses LQIP.
    /// The shipped authoring guidance promised a warning for all three tags
    /// while the scanner matched only two, so a raw `<audio>` was the one
    /// case where the documented safety net did not exist.
    #[test]
    fn detects_audio_tag() {
        let md = "<audio src=\"x.mp3\" controls></audio>";
        let hits = detect_raw_media(md);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tag, "audio");
        assert_eq!(hits[0].byte_offset, 0);
    }

    #[test]
    fn no_detection_for_markdown_image() {
        let md = "![alt](photo.jpg) plain markdown\n![[file.png]] embed";
        assert!(detect_raw_media(md).is_empty());
    }

    #[test]
    fn detects_uppercase_tag() {
        let md = "<IMG src=\"x.jpg\">";
        let hits = detect_raw_media(md);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tag, "img");
    }

    #[test]
    fn detects_self_closing_img() {
        let md = "<img/>";
        let hits = detect_raw_media(md);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tag, "img");
    }

    #[test]
    fn detects_multiple_tags_in_one_page() {
        let md = "<img src=\"a.jpg\"> middle <video src=\"b.mp4\"></video> <img>";
        let hits = detect_raw_media(md);
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].tag, "img");
        assert_eq!(hits[1].tag, "video");
        assert_eq!(hits[2].tag, "img");
    }

    #[test]
    fn does_not_match_similar_identifiers() {
        // `<imgur>`, `<videos>` and `<audiobook>` are not raw media tags -
        // the boundary class (`\s`, `>`, `/`) prevents the false match.
        let md = "<imgur>foo</imgur> <videos>bar</videos> <audiobook>baz</audiobook>";
        assert!(detect_raw_media(md).is_empty());
    }

    #[test]
    fn detects_tag_with_no_attributes() {
        let md = "<img>";
        let hits = detect_raw_media(md);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tag, "img");
    }

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
