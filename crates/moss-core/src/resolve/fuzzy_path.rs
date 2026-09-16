//! Fuzzy path resolution and relative URL computation.
//!
//! Wraps [`ContentGraph::resolve_path`] with typed results and provides
//! [`relative_url`] for generating correct relative links between files
//! using pretty URL format (directory-based).

use crate::content_graph::ContentGraph;

use super::parent_dir;

/// The result of resolving a reference against the content graph.
#[derive(Debug, PartialEq, Clone)]
pub enum ResolvedRef {
    /// The reference resolved to a file at this normalized path.
    Found(String),
    /// The reference could not be resolved to any known file.
    Unresolved,
}

/// Resolve a reference string against the content graph.
///
/// This is a thin wrapper over [`ContentGraph::resolve_path`] that returns
/// a typed [`ResolvedRef`] instead of `Option<String>`.
///
/// # Arguments
///
/// * `reference` — the link target (e.g. `"hello"`, `"posts/hello.md"`)
/// * `graph` — the content graph to search
/// * `from_path` — the file containing the link (for disambiguation)
pub fn resolve_reference(reference: &str, graph: &ContentGraph, from_path: &str) -> ResolvedRef {
    match graph.resolve_path(reference, from_path) {
        Some(path) => ResolvedRef::Found(path),
        None => ResolvedRef::Unresolved,
    }
}

/// Compute the relative URL from one file to another using pretty URL format.
///
/// Both paths should be relative to the source root (e.g. `"posts/hello.md"`).
/// The output uses directory-based pretty URLs:
///
/// - `"posts/hello.md"` becomes the URL `"posts/hello/"` (a directory)
/// - `"posts/index.md"` becomes the URL `"posts/"` (the directory itself)
/// - The returned URL is relative from `from_path`'s **pretty URL directory**
///   (e.g. `guide.md` is served from `guide/`, not root)
///
/// # Examples
///
/// ```
/// use moss_core::resolve::fuzzy_path::relative_url;
///
/// // Same directory → sibling pages need ".."
/// assert_eq!(relative_url("posts/a.md", "posts/b.md"), "../b/");
///
/// // Nested file to parent directory
/// assert_eq!(relative_url("posts/deep/a.md", "posts/b.md"), "../../b/");
///
/// // Target is index.md (URL is the directory)
/// assert_eq!(relative_url("posts/a.md", "posts/index.md"), "../");
/// ```
pub fn relative_url(from_path: &str, to_path: &str) -> String {
    // Use the pretty URL directory of the source file, not the file's parent.
    // A root-level `guide.md` is served from `guide/index.html`, so the
    // browser's base directory is `guide/`, not the project root.
    let from_dir = to_pretty_url_dir(from_path);
    let to_url_path = to_pretty_url_dir(to_path);

    // Split both into components
    let from_parts: Vec<&str> = if from_dir.is_empty() {
        vec![]
    } else {
        from_dir.split('/').collect()
    };

    let to_parts: Vec<&str> = if to_url_path.is_empty() {
        vec![]
    } else {
        // Remove trailing slash for splitting, then we'll add it back
        let trimmed = to_url_path.trim_end_matches('/');
        if trimmed.is_empty() {
            vec![]
        } else {
            trimmed.split('/').collect()
        }
    };

    // Find common prefix length
    let common = from_parts
        .iter()
        .zip(to_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();

    // Number of ".." needed to go up from from_dir
    let ups = from_parts.len() - common;

    // Remaining path components after the common prefix
    let remaining = &to_parts[common..];

    let mut result = String::new();

    if ups == 0 && remaining.is_empty() {
        // Same directory — from_dir IS the target URL directory
        return "./".to_string();
    }

    // Add "../" for each level we need to go up
    for _ in 0..ups {
        result.push_str("../");
    }

    // Add the remaining path components
    for (i, part) in remaining.iter().enumerate() {
        if i > 0 {
            result.push('/');
        }
        result.push_str(part);
    }

    // Ensure trailing slash for pretty URLs
    if !result.ends_with('/') {
        result.push('/');
    }

    result
}

/// Compute a relative URL from `from_path`'s parent directory to `to_path`,
/// preserving the target filename and extension. Each path segment is
/// percent-encoded so spaces and non-ASCII characters round-trip safely
/// through the markdown parser and HTML attribute boundaries.
///
/// Unlike [`relative_url`], which uses pretty-URL directories, this function
/// uses the *filesystem* parent directory (e.g. `posts/hello.md` -> `posts`).
/// The extra `../` needed for pretty-URL nesting is added later by
/// `adjust_relative_paths_for_pretty_urls` in the Tauri build layer.
/// Using `to_pretty_url_dir` here would double-count that adjustment.
///
/// Use this for binary assets (images, fonts, etc.) and any reference that
/// should keep its file extension in the URL.
pub fn relative_asset_path(from_path: &str, to_path: &str) -> String {
    let from_dir = parent_dir(from_path);
    let from_parts: Vec<&str> = if from_dir.is_empty() {
        vec![]
    } else {
        from_dir.split('/').collect()
    };

    let to_parts: Vec<&str> = if to_path.is_empty() {
        vec![]
    } else {
        to_path.split('/').collect()
    };

    let common = from_parts
        .iter()
        .zip(to_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let ups = from_parts.len() - common;
    let remaining = &to_parts[common..];

    let mut result = String::new();
    for _ in 0..ups {
        result.push_str("../");
    }
    for (i, part) in remaining.iter().enumerate() {
        if i > 0 {
            result.push('/');
        }
        push_encoded_segment(&mut result, part);
    }

    if result.is_empty() {
        // Same directory, just the filename
        let filename = to_path.rsplit('/').next().unwrap_or(to_path);
        let mut out = String::new();
        push_encoded_segment(&mut out, filename);
        out
    } else {
        result
    }
}

/// Percent-encode a path, segment by segment, preserving `/` as the separator.
///
/// Each segment keeps the RFC 3986 unreserved set (`A-Z a-z 0-9 - . _ ~`) plus
/// the sub-delim/extra characters that don't break a markdown `[alt](url)`
/// parse or an HTML attribute boundary: `!`, `$`, `&`, `'`, `+`, `,`, `;`,
/// `=`, `@`. Everything else — SPACE, parens, `#`, `?`, all non-ASCII bytes —
/// becomes `%XX`. The `..` and `.` segments survive untouched.
///
/// This is a *path* encoder: it treats `?` and `#` as ordinary bytes (e.g.
/// for filenames that contain them). For an HTML attribute that may carry a
/// `?query` or `#fragment`, use [`percent_encode_url`] instead — calling this
/// function on a URL silently turns `foo.html?a=1` into `foo.html%3Fa=1`,
/// which the browser then reads as a literal-filename 404.
pub fn percent_encode_path_segments(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for (i, segment) in path.split('/').enumerate() {
        if i > 0 {
            out.push('/');
        }
        push_encoded_segment(&mut out, segment);
    }
    out
}

/// Percent-*decode* `%XX` byte sequences back to the on-disk name. The inverse
/// of [`percent_encode_path_segments`], and it lives next to it so the pair
/// cannot drift.
///
/// Call this at every point where a value that has already been through the
/// encoder is used as a **filesystem path or a cache key** rather than as a
/// URL. A cover named `封面.jpg` reaches such a reader as
/// `%E5%B0%81%E9%9D%A2.jpg`; joining that onto a source root looks for a file
/// whose name is literally the escape sequence and silently misses.
///
/// Lenient by construction: a lone or malformed `%` passes through verbatim,
/// and a decode that isn't valid UTF-8 falls back to the input unchanged, so
/// this is safe to call on a string that was never encoded (it is a no-op when
/// there is no `%`).
pub fn percent_decode_path(path: &str) -> String {
    if !path.contains('%') {
        return path.to_string();
    }
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| path.to_string())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Split a URL into its path portion and any `?query` / `#fragment` suffix.
/// The split happens at the first occurrence of `?` or `#` (whichever appears
/// first); if neither is present the suffix is empty. The suffix is returned
/// verbatim, including the leading `?` / `#`.
///
/// Used by [`percent_encode_url`] and by callers that need to apply
/// path-only transformations (e.g. directory-name overrides) before reattaching
/// an opaque query/fragment.
pub fn split_url_path(url: &str) -> (&str, &str) {
    let q = url.find('?');
    let h = url.find('#');
    let cut = match (q, h) {
        (Some(qi), Some(hi)) => Some(qi.min(hi)),
        (Some(qi), None) => Some(qi),
        (None, Some(hi)) => Some(hi),
        (None, None) => None,
    };
    let Some(i) = cut else {
        return (url, "");
    };
    // `i` came from `find('?')` / `find('#')` — both ASCII bytes, so the
    // returned position is on a UTF-8 char boundary by construction.
    #[allow(clippy::string_slice)]
    (&url[..i], &url[i..])
}

/// URL-aware sibling of [`percent_encode_path_segments`]. Splits at the first
/// `?` or `#`, runs the segment encoder on the path portion, and re-attaches
/// the query/fragment verbatim. Use this for any string that flows into an
/// HTML `src=`/`href=` attribute or a markdown `[alt](url)` link, where the
/// caller cannot guarantee the value is a pure path.
pub fn percent_encode_url(url: &str) -> String {
    let (path, suffix) = split_url_path(url);
    if suffix.is_empty() {
        percent_encode_path_segments(path)
    } else {
        let mut out = percent_encode_path_segments(path);
        out.push_str(suffix);
        out
    }
}

fn push_encoded_segment(out: &mut String, segment: &str) {
    for &b in segment.as_bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b'~'
            | b'!'
            | b'$'
            | b'&'
            | b'\''
            | b'+'
            | b','
            | b';'
            | b'='
            | b'@' => {
                out.push(b as char);
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{:02X}", b);
            }
        }
    }
}

/// Convert a source file path to its pretty URL directory path.
///
/// - `"posts/hello.md"` -> `"posts/hello"`  (the URL becomes `posts/hello/`)
/// - `"posts/index.md"` -> `"posts"`        (the URL becomes `posts/`)
/// - `"index.md"`       -> `""`             (the URL becomes `/`)
pub(crate) fn to_pretty_url_dir(path: &str) -> String {
    // Strip the file extension
    let without_ext = match path.rsplit_once('.') {
        Some((head, _)) => head,
        None => path,
    };

    // If the filename is "index", the URL is the parent directory
    let filename = without_ext.rsplit('/').next().unwrap_or(without_ext);
    if filename == "index" {
        let parent = parent_dir(without_ext);
        parent.to_string()
    } else {
        without_ext.to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content_graph::ContentGraphBuilder;

    /// Build a graph with common test files.
    fn sample_graph() -> ContentGraph {
        let mut b = ContentGraphBuilder::new();
        b.add_file("posts/hello.md", "/posts/hello");
        b.add_file("posts/world.md", "/posts/world");
        b.add_file("guides/hello.md", "/guides/hello");
        b.add_file("projects/index.md", "/projects");
        b.add_file("about.md", "/about");
        b.add_file("images/photo.png", "/images/photo.png");
        b.add_file("index.md", "/");
        b.build()
    }

    // -- percent_decode_path tests --

    /// The encoder and its inverse live next to each other so they cannot
    /// drift; this asserts the round trip for the names that actually broke
    /// production (CJK covers, spaces, commas).
    #[test]
    fn percent_decode_path_inverts_the_segment_encoder() {
        for raw in [
            "獎項/封面.jpg",
            "News/Winter Song.mov",
            "img/a,b.png",
            "plain/ascii.webp",
            "nested/深い/道/photo.jpeg",
        ] {
            let encoded = percent_encode_path_segments(raw);
            assert_eq!(percent_decode_path(&encoded), raw, "round trip for {raw}");
        }
    }

    /// Decoding must be safe to call on a value that was never encoded — the
    /// cover-color readers call it unconditionally.
    #[test]
    fn percent_decode_path_is_a_noop_without_escapes() {
        assert_eq!(percent_decode_path("images/photo.png"), "images/photo.png");
        assert_eq!(percent_decode_path(""), "");
    }

    /// A lone or malformed `%` is a legal filename byte, not a decode error.
    #[test]
    fn percent_decode_path_passes_through_malformed_escapes() {
        assert_eq!(percent_decode_path("100%.png"), "100%.png");
        assert_eq!(percent_decode_path("a%zz b.png"), "a%zz b.png");
        assert_eq!(percent_decode_path("trailing%2"), "trailing%2");
    }

    /// Invalid UTF-8 must fall back to the input rather than panic or lose it.
    #[test]
    fn percent_decode_path_falls_back_on_invalid_utf8() {
        assert_eq!(percent_decode_path("bad%FF.png"), "bad%FF.png");
    }

    // -- resolve_reference tests --

    #[test]
    fn test_resolve_exact_relative() {
        let graph = sample_graph();
        assert_eq!(
            resolve_reference("posts/hello.md", &graph, "posts/world.md"),
            ResolvedRef::Found("posts/hello.md".into())
        );
    }

    #[test]
    fn test_resolve_filename_only() {
        let graph = sample_graph();
        // "world" is unique, so filename-only lookup should find it
        assert_eq!(
            resolve_reference("world", &graph, ""),
            ResolvedRef::Found("posts/world.md".into())
        );
    }

    #[test]
    fn test_resolve_case_insensitive() {
        let graph = sample_graph();
        // "World" with different casing should still resolve
        assert_eq!(
            resolve_reference("World", &graph, ""),
            ResolvedRef::Found("posts/world.md".into())
        );
        assert_eq!(
            resolve_reference("ABOUT", &graph, ""),
            ResolvedRef::Found("about.md".into())
        );
    }

    #[test]
    fn test_resolve_unresolved() {
        let graph = sample_graph();
        assert_eq!(
            resolve_reference("nonexistent", &graph, "posts/hello.md"),
            ResolvedRef::Unresolved
        );
        assert_eq!(
            resolve_reference("missing/page.md", &graph, ""),
            ResolvedRef::Unresolved
        );
    }

    #[test]
    fn test_resolve_image_reference() {
        let graph = sample_graph();
        // Non-markdown file (image) should also resolve
        assert_eq!(
            resolve_reference("images/photo.png", &graph, "posts/hello.md"),
            ResolvedRef::Found("images/photo.png".into())
        );
    }

    // -- relative_url tests --
    //
    // Pretty URL layout:
    //   guide.md    → served at guide/index.html    (browser dir: guide/)
    //   posts/a.md  → served at posts/a/index.html  (browser dir: posts/a/)
    //   index.md    → served at index.html           (browser dir: /)
    //   posts/index.md → served at posts/index.html  (browser dir: posts/)

    #[test]
    fn test_relative_url_same_dir() {
        // "posts/a.md" (at posts/a/) to "posts/b.md" (at posts/b/) → "../b/"
        assert_eq!(relative_url("posts/a.md", "posts/b.md"), "../b/");
    }

    #[test]
    fn test_relative_url_nested() {
        // "posts/deep/a.md" (at posts/deep/a/) to "posts/b.md" (at posts/b/) → "../../b/"
        assert_eq!(relative_url("posts/deep/a.md", "posts/b.md"), "../../b/");
    }

    #[test]
    fn test_relative_url_sibling_dir() {
        // "blog/a.md" (at blog/a/) to "notes/b.md" (at notes/b/) → "../../notes/b/"
        assert_eq!(relative_url("blog/a.md", "notes/b.md"), "../../notes/b/");
    }

    #[test]
    fn test_relative_url_index() {
        // "posts/a.md" (at posts/a/) to "posts/index.md" (at posts/) → "../"
        assert_eq!(relative_url("posts/a.md", "posts/index.md"), "../");

        // "blog/a.md" (at blog/a/) to "posts/index.md" (at posts/) → "../../posts/"
        assert_eq!(relative_url("blog/a.md", "posts/index.md"), "../../posts/");

        // root index: "posts/a.md" (at posts/a/) to "index.md" (at /) → "../../"
        assert_eq!(relative_url("posts/a.md", "index.md"), "../../");
    }

    #[test]
    fn test_relative_url_root_to_nested() {
        // "index.md" (at /) to "posts/hello.md" (at posts/hello/) → "posts/hello/"
        assert_eq!(relative_url("index.md", "posts/hello.md"), "posts/hello/");
    }

    #[test]
    fn test_relative_url_nested_to_root() {
        // "posts/hello.md" (at posts/hello/) to "about.md" (at about/) → "../../about/"
        assert_eq!(relative_url("posts/hello.md", "about.md"), "../../about/");
    }

    #[test]
    fn test_relative_url_root_level_file() {
        // "guide.md" (at guide/) to "notes/daily.md" (at notes/daily/) → "../notes/daily/"
        assert_eq!(
            relative_url("guide.md", "notes/daily.md"),
            "../notes/daily/"
        );
    }

    #[test]
    fn test_relative_url_index_from_dir() {
        // "posts/index.md" (at posts/) to "posts/a.md" (at posts/a/) → "a/"
        assert_eq!(relative_url("posts/index.md", "posts/a.md"), "a/");
    }

    // -- relative_asset_path tests --
    //
    // Asset paths use the *filesystem* parent of `from_path` (no pretty-URL
    // nesting). Each segment is percent-encoded for safe markdown/HTML emission.

    #[test]
    fn test_relative_asset_path_same_dir() {
        assert_eq!(
            relative_asset_path("posts/hello.md", "posts/photo.jpg"),
            "photo.jpg"
        );
    }

    #[test]
    fn test_relative_asset_path_sibling_dir() {
        assert_eq!(
            relative_asset_path("posts/hello.md", "assets/photo.jpg"),
            "../assets/photo.jpg"
        );
    }

    #[test]
    fn test_relative_asset_path_encodes_spaces() {
        // The original symptom: a filename with spaces produced an unparsable
        // markdown image link. Spaces must encode to %20.
        assert_eq!(
            relative_asset_path("posts/hello.md", "assets/Pasted image 20260505.png"),
            "../assets/Pasted%20image%2020260505.png"
        );
    }

    #[test]
    fn test_relative_asset_path_encodes_non_ascii() {
        // CJK directory + filename: every non-ASCII byte percent-encodes.
        // 图片 = E5 9B BE E7 89 87, 摄影 = E6 91 84 E5 BD B1
        assert_eq!(
            relative_asset_path("文字/article.md", "图片/摄影/_43A2045.jpg"),
            "../%E5%9B%BE%E7%89%87/%E6%91%84%E5%BD%B1/_43A2045.jpg"
        );
    }

    #[test]
    fn test_relative_asset_path_preserves_unreserved() {
        // Unreserved RFC 3986 chars stay literal.
        assert_eq!(
            relative_asset_path("a.md", "img-1_v2.0~final.jpg"),
            "img-1_v2.0~final.jpg"
        );
    }

    #[test]
    fn test_relative_asset_path_root_to_nested() {
        // from is at root (no parent dir), to is nested with spaces.
        assert_eq!(
            relative_asset_path("index.md", "img/cover photo.png"),
            "img/cover%20photo.png"
        );
    }

    // -- percent_encode_url + split_url_path tests --
    //
    // `percent_encode_url` is the URL-aware sibling of `percent_encode_path_segments`.
    // It splits at the first `?` or `#`, encodes only the path portion, and passes
    // the query/fragment through verbatim. This is the right encoder for URLs
    // produced by the embed renderers and for any caller that handles `src=`/`href=`
    // attribute values.

    #[test]
    fn percent_encode_url_preserves_query_string() {
        // The bug class: the path-only encoder turns `?` into `%3F`, breaking
        // any iframe embed of the form `![[file.html?a=1&r=2]]`. The URL-aware
        // encoder must keep the `?` literal.
        assert_eq!(
            percent_encode_url("../scale-compare.html?a=major_pent,major_blues&r=major_pent:D"),
            "../scale-compare.html?a=major_pent,major_blues&r=major_pent:D"
        );
    }

    #[test]
    fn percent_encode_url_preserves_fragment() {
        // Fragments must also pass through. `#` is the path/fragment separator.
        assert_eq!(
            percent_encode_url("../doc.html#section-2"),
            "../doc.html#section-2"
        );
    }

    #[test]
    fn percent_encode_url_preserves_query_and_fragment_together() {
        // Both `?` and `#` present — split at whichever appears first (RFC
        // says it's always `?`, but we don't trust input shape).
        assert_eq!(
            percent_encode_url("../app.html?a=1#part"),
            "../app.html?a=1#part"
        );
    }

    #[test]
    fn percent_encode_url_still_encodes_path_segments() {
        // The path *part* still gets the segment encoder treatment — spaces
        // and non-ASCII bytes become %20/%XX.
        assert_eq!(
            percent_encode_url("../assets/Pasted image.png?v=2"),
            "../assets/Pasted%20image.png?v=2"
        );
        assert_eq!(
            percent_encode_url("../图片/cover.jpg?v=2"),
            "../%E5%9B%BE%E7%89%87/cover.jpg?v=2"
        );
    }

    #[test]
    fn percent_encode_url_no_suffix_matches_path_encoder() {
        // For URLs without `?` or `#`, output is identical to
        // `percent_encode_path_segments` — preserving the existing contract.
        let input = "../assets/Pasted image 20260505.png";
        assert_eq!(
            percent_encode_url(input),
            percent_encode_path_segments(input)
        );
    }

    #[test]
    fn split_url_path_at_question_mark() {
        assert_eq!(split_url_path("foo.html?a=1&b=2"), ("foo.html", "?a=1&b=2"));
    }

    #[test]
    fn split_url_path_at_fragment() {
        assert_eq!(split_url_path("doc.html#section"), ("doc.html", "#section"));
    }

    #[test]
    fn split_url_path_picks_first_separator() {
        // `?` before `#` (RFC-compliant order).
        assert_eq!(split_url_path("a.html?q=1#f"), ("a.html", "?q=1#f"));
        // `#` before `?` (atypical but defined behavior — split at whichever
        // appears first; everything after that point is opaque to the path
        // encoder either way).
        assert_eq!(split_url_path("a.html#f?q=1"), ("a.html", "#f?q=1"));
    }

    #[test]
    fn split_url_path_no_separator_returns_empty_suffix() {
        assert_eq!(split_url_path("plain/path.png"), ("plain/path.png", ""));
        assert_eq!(split_url_path(""), ("", ""));
    }
}
