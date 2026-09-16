//! Synthetic homepage for empty folders.
//!
//! When the user opens a folder containing zero markdown files
//! (`articles.is_empty()` per the onboarding spec at
//! `docs/archive/2026-05-28-onboarding-design.md`), moss still needs to emit
//! a real `index.html` so the preview shows a moss-rendered site rather than
//! a 404. This module produces the synthetic `ParsedDocument` for that page.
//!
//! ## Predicate
//!
//! The trigger matches the build scan rule: `articles = walk(folder)`
//! `.filter(is_markdown).filter(not_in_excluded_dir)`. Excluded dirs are
//! defined by [`crate::build::scan::classify::is_excluded_dir_name`] —
//! `node_modules/` and dot-prefixed names (`.git/`, `.moss/`, etc.). See
//! `classify.rs:21-42` for the canonical list.
//!
//! The render pipeline uses the equivalent `documents.is_empty()` check at
//! the call site in `blocking.rs` after markdown processing has populated
//! the docs vector. The two predicates are equivalent because every
//! markdown file scanned becomes a `ParsedDocument`.
//!
//! ## Output
//!
//! - `html_content`: empty string — site chrome (nav, theme toggle, footer)
//!   comes from the standard homepage render path; the `<article>` body is
//!   intentionally blank. The editor's onboarding cards already tell the user
//!   what to do; the preview doesn't need to repeat it.
//!
//! Everything else (chrome, nav skeleton, theme toggle, footer, OG metadata,
//! sitemap) comes from the standard homepage render path in
//! `blocking.rs` — this module just hands it a doc to render.

use crate::build::types::ParsedDocument;
use moss_core::PageKind;

/// Construct the synthetic homepage `ParsedDocument` for an empty folder.
///
/// The doc routes through the standard homepage render path
/// (`generate_html_collect_og` with `is_homepage: true`) in `blocking.rs`.
/// Fields:
/// - `title` / `label`: folder name. Layout config also resolves to folder
///   name via `Path::file_name` so the chrome and the page title agree
///   without us touching layout resolution.
/// - `url_path`: `index.html`. The render path keys the homepage lookup on
///   this exact string.
/// - `kind`: `PageKind::Folder`. Marks this as an index page (matching what
///   `index.md`/`readme.md` would yield) so any downstream code that filters
///   by kind treats it consistently.
/// - `html_content`: empty string. The page renders with an empty `<article>`
///   body — site chrome (nav, theme toggle, footer) stays intact via the
///   standard template. No injected H1 or placeholder text; the editor's
///   onboarding cards surface the call-to-action instead.
/// - `lang`: site language so the page's `<html lang>` matches the rest of
///   the (would-be) site.
pub fn synthesize_empty_homepage(
    folder_name: &str,
    site_lang: crate::i18n::Language,
) -> ParsedDocument {
    ParsedDocument {
        title: folder_name.to_string(),
        label: folder_name.to_string(),
        url_path: "index.html".to_string(),
        kind: PageKind::Folder,
        html_content: String::new(),
        lang: site_lang,
        clean_stem: "index".to_string(),
        is_root_level: true,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthesizes_empty_doc_for_empty_folder() {
        let doc = synthesize_empty_homepage("my-blog", crate::i18n::Language::En);
        assert_eq!(doc.title, "my-blog");
        assert_eq!(doc.label, "my-blog");
        assert_eq!(doc.url_path, "index.html");
        assert_eq!(doc.kind, PageKind::Folder);
        // No injected H1 or placeholder text — the article body is blank.
        assert!(
            doc.html_content.is_empty(),
            "html_content should be empty for empty-folder homepage, got: {:?}",
            doc.html_content,
        );
    }

    #[test]
    fn url_path_is_root_index_html() {
        // The root-homepage codepath in render/blocking.rs keys off
        // `url_path == "index.html"` (exactly). Any drift here breaks the
        // homepage dispatch.
        let doc = synthesize_empty_homepage("anywhere", crate::i18n::Language::En);
        assert_eq!(doc.url_path, "index.html");
    }

    #[test]
    fn kind_is_folder_not_article() {
        // Folder kind matches what `index.md` would produce. Article kind
        // would cause downstream filters (e.g. nav listing) to treat this
        // synthetic doc as a regular post.
        let doc = synthesize_empty_homepage("x", crate::i18n::Language::En);
        assert_eq!(doc.kind, PageKind::Folder);
    }

    #[test]
    fn lang_threads_through() {
        let doc = synthesize_empty_homepage("x", crate::i18n::Language::ZhHans);
        assert_eq!(doc.lang, crate::i18n::Language::ZhHans);
    }
}
