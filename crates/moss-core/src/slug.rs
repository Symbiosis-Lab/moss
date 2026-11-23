//! URL-safe slug generation for moss-core.
//!
//! The two pure primitives used by `compute_url_path` and re-exported to moss-build.
//!
//! **Disambiguation:** `moss_core::content_graph` has its own internal `generate_slug`
//! that strips file extensions and handles full relative paths (a "path-to-key"
//! transform). This module's `generate_slug` is a "text-to-slug" primitive for
//! titles, folder names, and URL segments. Use the right one for the right job.
//! UID generation and duplicate-slug deduplication remain in moss-build
//! pending a planned byte-slicing audit.

use unicode_normalization::UnicodeNormalization;

/// Converts a string to a URL-safe slug.
///
/// - Lowercases ASCII
/// - Replaces spaces and underscores with hyphens
/// - Replaces `&` → `and`, `@` → `at`, `+` → `plus`, `#` → `hash`, `%` → `percent`
/// - Preserves CJK and other Unicode letters
/// - Strips consecutive hyphens; trims leading/trailing hyphens
/// - Caps at 100 chars
/// - Falls back to `"untitled"` for empty results
pub fn generate_slug(text: &str) -> String {
    // Compose first (NFC): a decomposed name from macOS/iCloud would otherwise
    // lose its combining marks and get a different URL than its composed twin.
    let result = text
        .nfc()
        .collect::<String>()
        .to_lowercase()
        .replace([' ', '_'], "-")
        .replace('&', "and")
        .replace('@', "at")
        .replace('+', "plus")
        .replace('#', "hash")
        .replace('%', "percent")
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '.' { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<&str>>()
        .join("-")
        .trim_matches('-')
        .chars()
        .take(100)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string();

    if result.is_empty() { "untitled".to_string() } else { result }
}

/// Normalize path separators to `/`.
///
/// moss treats `\` as a path separator everywhere so content paths behave
/// identically regardless of the authoring OS. Windows `strip_prefix` yields
/// backslash-separated relative paths; left un-normalized they collapse nested
/// page/asset URLs (every segment after the first is lost) and defeat the file
/// watcher's `/.moss/` gate (causing a runaway rebuild loop). Literal backslashes
/// in content filenames are therefore not supported — they are read as separators.
pub fn normalize_separators(s: &str) -> String {
    s.replace('\\', "/")
}

/// Two names in composed (NFC) order, falling back to their bytes when they
/// compose alike. Every walk and list whose order reaches the output sorts by
/// this, so a folder reads in one order on every machine whatever its disk
/// lists first and however its sync client spelled the names.
pub fn nfc_order(a: &str, b: &str) -> std::cmp::Ordering {
    a.nfc().cmp(b.nfc()).then_with(|| a.cmp(b))
}

/// Two `/`-separated paths in the order a depth-first walk of the folder
/// visits them, siblings in [`nfc_order`]: a folder's contents come right
/// after its name, so `a/x.md` precedes `a-b.md` although `-` sorts before `/`.
pub fn walk_order(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a.split('/'), b.split('/'));
    loop {
        match (a.next(), b.next()) {
            (Some(x), Some(y)) => match nfc_order(x, y) {
                std::cmp::Ordering::Equal => continue,
                other => return other,
            },
            (x, y) => return x.is_some().cmp(&y.is_some()),
        }
    }
}

/// Apply slug rules to every separator-delimited segment of a path.
///
/// `News/Sub Section` → `news/sub-section`. Backslash separators are normalized
/// first (`News\Sub Section` → the same result). Empty segments and segments
/// made only of dots (`.`, `..`) are dropped, so the result can never climb
/// out of the folder it is joined onto.
pub fn slugify_path_segments(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    normalize_separators(path)
        .split('/')
        .filter(|s| !s.trim_matches('.').is_empty())
        .map(generate_slug)
        .collect::<Vec<_>>()
        .join("/")
}

/// Slug a frontmatter `url:` value: [`slugify_path_segments`], or `untitled`
/// when nothing survives (`..`, `/`).
pub fn slugify_url_override(url: &str) -> String {
    let slug = slugify_path_segments(url);
    if slug.is_empty() { generate_slug("") } else { slug }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_order_visits_a_folder_right_after_its_name() {
        let mut paths = vec!["a-b.md", "b.md", "a/x.md", "a.md", "a/b/c.md", "\u{e9}.md", "f.md"];
        paths.sort_by(|a, b| walk_order(a, b));
        assert_eq!(paths, ["a/b/c.md", "a/x.md", "a-b.md", "a.md", "b.md", "f.md", "\u{e9}.md"]);
        assert_eq!(walk_order("e\u{301}/x.md", "f.md"), std::cmp::Ordering::Greater);
    }

    #[test]
    fn nfd_and_nfc_spellings_give_the_same_slug() {
        assert_eq!(generate_slug("cafe\u{0301} au lait"), "caf\u{00e9}-au-lait");
        assert_eq!(slugify_path_segments("Dossier-e\u{0301}te\u{0301}/x"), slugify_path_segments("Dossier-\u{e9}t\u{e9}/x"));
    }

    #[test]
    fn path_segments_drop_dot_and_empty_parts() {
        assert_eq!(slugify_path_segments("../a//./B"), "a/b");
        assert_eq!(slugify_path_segments(".."), "");
        assert_eq!(slugify_url_override(".."), "untitled");
    }

    #[test]
    fn ascii_lowercased_and_hyphenated() {
        assert_eq!(generate_slug("Hello World"), "hello-world");
    }

    #[test]
    fn special_chars_replaced() {
        assert_eq!(generate_slug("A & B"), "a-and-b");
        assert_eq!(generate_slug("price@50%"), "priceat50percent");
    }

    #[test]
    fn cjk_preserved() {
        assert_eq!(generate_slug("你好世界"), "你好世界");
    }

    #[test]
    fn empty_falls_back_to_untitled() {
        assert_eq!(generate_slug("---"), "untitled");
    }

    #[test]
    fn path_segments_slugified() {
        assert_eq!(slugify_path_segments("News/Sub Section"), "news/sub-section");
        assert_eq!(slugify_path_segments(""), "");
    }

    #[test]
    fn normalize_separators_converts_backslashes() {
        // moss treats `\` as a path separator everywhere (Windows-authored
        // content paths arrive backslash-separated). Forward slashes pass through.
        assert_eq!(normalize_separators("News\\2025"), "News/2025");
        assert_eq!(normalize_separators("a/b/c"), "a/b/c");
        assert_eq!(normalize_separators("Sub Dir\\Winter-Song.mov"), "Sub Dir/Winter-Song.mov");
        assert_eq!(normalize_separators(""), "");
    }

    #[test]
    fn path_segments_handle_backslash_separators() {
        // The Windows bug: a backslash-separated path must slug into the SAME
        // nested `/`-form as the slash version, not collapse into one segment.
        assert_eq!(slugify_path_segments("News\\Sub Section"), "news/sub-section");
        assert_eq!(
            slugify_path_segments("News\\Sub Section"),
            slugify_path_segments("News/Sub Section"),
        );
        assert!(!slugify_path_segments("A\\B\\C").contains('\\'));
    }
}
