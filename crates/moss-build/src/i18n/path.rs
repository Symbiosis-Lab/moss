//! Path-based language detection: lang inferred from ancestor folders.
//!
//! When a doc lives under a folder named after a language code (e.g.
//! `en/`, `zh-hans/`, `zh/`), that folder placement is a deliberate
//! authoring decision and should be honored as the doc's language —
//! stronger than content-detection heuristics, weaker than per-doc
//! explicit signals (frontmatter `lang:` and filename suffixes).
//!
//! This module just walks the path. The priority chain that decides
//! whether to use the result lives in
//! [`crate::i18n::resolve_document_language`].
//!
//! ## Single policy for folder-name → Language lookup
//!
//! [`resolve_language_from_folder`] is the single canonical helper for
//! "does this folder/URL segment name identify a language?". All call
//! sites that need to check whether a directory component is a lang-root
//! (URL routing in `blocking.rs`, path walking here) must call this
//! function rather than calling `Language::from_code` directly. That
//! keeps the lookup logic in one place and makes the intent explicit.
//!
//! If the project ever lands an explicit opt-in (folder note `lang:`,
//! project-level `[site] languages = ["en", "zh-hans"]`, or similar),
//! update
//! **only** this helper and all callers inherit the new policy
//! automatically.

use super::Language;
use std::path::Path;

/// Resolve a folder (or URL segment) name to a [`Language`].
///
/// This is the canonical lookup for "is this directory component a
/// recognised language-root folder?". Returns `Some(lang)` if `name`
/// matches a known language code (case-insensitive), `None` otherwise.
///
/// All call sites that route or filter on lang-prefix directories must
/// use this function rather than `Language::from_code` directly, so
/// that any future policy change (e.g. requiring an explicit opt-in)
/// only needs to be made here.
///
/// # Examples
/// - `"en"` → `Some(En)`
/// - `"zh-hans"` → `Some(ZhHans)`
/// - `"zh"` → `Some(ZhHans)` (bare `zh` is shorthand for Simplified)
/// - `"articles"` → `None`
/// - `"2026"` → `None`
pub fn resolve_language_from_folder(name: &str) -> Option<Language> {
    Language::from_code(name)
}

/// Walk the ancestor folders of a path and return the **first match**
/// whose name parses as a language code, scanning **outermost-first**
/// (top-level wins over deeper nesting). Returns `None` if no ancestor
/// folder name parses as a language code.
///
/// Operates purely on the path string — only directory components are
/// considered, the file itself is skipped, and `..` components are
/// preserved verbatim by `Path::components()` (so a path like
/// `../en/post.md` would still match `en`). No filesystem access.
///
/// # Examples
/// - `en/videos/winter-song.md` → `Some(En)` (matches `en/`)
/// - `videos/winter-song.md` → `None`
/// - `en/zh-hans/oops.md` → `Some(En)` (outer wins; nested mismatch is
///   a user mistake, not for this helper to resolve — it'll surface
///   downstream as a translation-pairing oddity)
/// - `notes/en/foo.md` → `Some(En)` (any ancestor, not just top-level —
///   this matches Hugo / Astro conventions where the lang folder can
///   live under a section root)
pub fn ancestor_lang_from_path(file_path: &str) -> Option<Language> {
    Path::new(file_path)
        .parent()?
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .find_map(resolve_language_from_folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_ancestor_match() {
        assert_eq!(ancestor_lang_from_path("videos/winter-song.md"), None);
    }

    #[test]
    fn flat_file_no_parent_dir() {
        assert_eq!(ancestor_lang_from_path("just-a-file.md"), None);
    }

    #[test]
    fn empty_path() {
        assert_eq!(ancestor_lang_from_path(""), None);
    }

    #[test]
    fn top_level_en() {
        assert_eq!(
            ancestor_lang_from_path("en/videos/winter-song.md"),
            Some(Language::En)
        );
    }

    #[test]
    fn top_level_zh_hans() {
        assert_eq!(
            ancestor_lang_from_path("zh-hans/about.md"),
            Some(Language::ZhHans)
        );
    }

    #[test]
    fn bare_zh_normalizes_to_simplified() {
        // Matches `Language::from_code("zh")` behavior (Task 2.6).
        assert_eq!(
            ancestor_lang_from_path("zh/post.md"),
            Some(Language::ZhHans)
        );
    }

    #[test]
    fn zh_hant_traditional() {
        assert_eq!(
            ancestor_lang_from_path("zh-hant/about.md"),
            Some(Language::ZhHant)
        );
    }

    #[test]
    fn nested_under_section() {
        // Lang folder doesn't have to be at the project root.
        assert_eq!(
            ancestor_lang_from_path("notes/en/scratch.md"),
            Some(Language::En)
        );
    }

    #[test]
    fn outer_wins_when_nested_codes_disagree() {
        // User mistake (en/ containing a zh-hans/ folder); we pick the
        // outermost match deterministically. Translation pairing
        // downstream will catch the inconsistency if it matters.
        assert_eq!(
            ancestor_lang_from_path("en/zh-hans/oops.md"),
            Some(Language::En)
        );
    }

    #[test]
    fn does_not_match_filename_stem() {
        // `en.md` is a file, not a folder — no match.
        assert_eq!(ancestor_lang_from_path("posts/en.md"), None);
    }

    #[test]
    fn case_insensitive_via_from_code() {
        // `Language::from_code` lowercases, so `EN/` works too.
        assert_eq!(
            ancestor_lang_from_path("EN/post.md"),
            Some(Language::En)
        );
    }

    #[test]
    fn unrelated_folder_named_like_lang_at_deeper_level() {
        // Currently we accept any ancestor that parses as a lang code.
        // This is the "implicit" model — if the user has a folder named
        // `en/` for any reason (engineering, entries, etc.), it gets
        // treated as English. The plan doc tracks this as Issue 1
        // (folder-name lang-prefix routing without opt-in); the same
        // policy applies here for consistency with the routing layer.
        assert_eq!(
            ancestor_lang_from_path("articles/2026/en/post.md"),
            Some(Language::En)
        );
    }
}
