//! Slug generation, UID generation, and duplicate URL resolution.
//!
//! This module provides:
//! - [`generate_slug`] — URL-safe slug from arbitrary text (titles, headings)
//! - [`generate_uid`] — 8-char hex UID, the same on every machine for the same page
//! - [`resolve_duplicate_slugs`] / [`resolve_duplicate_slugs_with_lang`] — deduplication of URL paths

use crate::build::types::ParsedDocument;
use moss_core::PageKind;

// generate_slug and slugify_path_segments moved to moss-core.
pub use moss_core::slug::{generate_slug, slugify_path_segments};

/// Lowercase directory segments only; preserve the final basename verbatim.
/// A ladder directory (`Clip.hls`) is the exception among the directories: it
/// is a bundle named after a video file, so it keeps that file's spelling.
///
/// This is the rule used by [`ServedPath::from_source`][crate::build::served_path::ServedPath::from_source]
/// for build-pipeline output paths. The bug it fixes is **case-sensitivity
/// drift** (a source `Resources/` directory landing as `resources/` on
/// case-sensitive servers), so the only transformation we need is
/// lowercasing. Slug-rewriting beyond that breaks paths that are *already
/// valid URLs* with non-ASCII or `@`-prefixed segments — e.g.,
/// `@jupyter-notebook/` (npm-scope-style names JupyterLite ships in its
/// bundled extensions directory).
///
/// The basename is preserved verbatim because third-party JS bundles
/// (JupyterLite, MathJax, p5.js) load asset files by their exact
/// filenames. Lowercase-rewriting `MathJax_Main-Bold.woff` → 404.
///
/// Examples:
/// - `Resources/orbit-model.html` → `resources/orbit-model.html`
/// - `jupyter/build/schemas/@jupyter-notebook/foo.json` → `jupyter/build/schemas/@jupyter-notebook/foo.json`
/// - `News/Sub Section/post.md` → `news/sub section/post.md`
/// - `Photo (1).jpg` → `Photo (1).jpg` (no dirs)
/// - `文档/介绍.html` → `文档/介绍.html`
///
/// For *URL emission* (article pretty-URLs in `compute_url_path`), the
/// filename IS slugged via `generate_slug(stem)` separately — that's
/// intentional: pretty URLs need ascii-safe slugs; static asset filenames
/// don't.
pub fn slugify_dir_path(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let path = crate::build::scan::path_collision::logical_path(path);
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return String::new();
    }
    let last = segments.len() - 1;
    segments
        .iter()
        .enumerate()
        .map(|(i, seg)| {
            if i == last || moss_core::asset_paths::hls_dir_stem(seg).is_some() {
                seg.to_string()
            } else {
                seg.to_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// A deterministic, nonsecret 8-character hex UID for the page at `relative_path`.
/// Uses the first four bytes of SHA-256 over the NFC-normalized path, a zero
/// byte and `discriminator`. For a new page, the discriminator is the hex
/// SHA-256 of its raw bytes; for a duplicate, it is the contested UID and an
/// attempt number. Equal inputs produce the same UID on every machine.
/// Once published into a page, the UID stays when the page moves or changes
/// and cannot be recomputed from the later page.
pub fn generate_uid(relative_path: &str, discriminator: &str) -> String {
    use sha2::{Digest, Sha256};
    use unicode_normalization::UnicodeNormalization;
    let mut hasher = Sha256::new();
    hasher.update(relative_path.nfc().collect::<String>().as_bytes());
    hasher.update([0]);
    hasher.update(discriminator.as_bytes());
    hasher.finalize()[..4].iter().map(|b| format!("{b:02x}")).collect()
}

pub use moss_core::page::page_map::UrlCollision;

/// Windows reserved device names — `CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`,
/// `LPT1`-`LPT9` — matched case-insensitively and regardless of any
/// extension. The OS intercepts a path component spelled this way as the
/// device itself, so neither a file nor a directory can exist under one of
/// these names on an NTFS volume.
const RESERVED_WINDOWS_DEVICE_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL",
    "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Whether a page's `url_path` (`"<dir>/index.html"`, or the root
/// `"index.html"`) would create an output path with a Windows reserved
/// device name as ANY directory component — the leaf (the page's own slug)
/// or an intermediate one (a source folder that slugifies to a reserved
/// name, which becomes an ancestor directory of every page under it).
///
/// The root homepage owns no directory of its own (`None` from the
/// `strip_suffix`) and is never flagged. A moss site built directly on
/// Windows can't produce such a path at all — the source file/folder that
/// would slug to it could never have been created there — but a vault built
/// on macOS/Linux (legal filenames there) and later synced to Windows, or
/// built there directly, would try to create e.g. `nul/index.html` or
/// `con/anything/index.html`, which fails: `CreateDirectory`/`CreateFile` on
/// a segment named `nul` or `con` resolves to the device, not a path on
/// disk. Each segment is compared by the part before its first `.`, so a
/// segment carrying an extension (`nul.txt`) still matches.
fn is_reserved_device_output(url_path: &str) -> bool {
    let Some(dir) = url_path.strip_suffix("/index.html") else {
        return false;
    };
    dir.split('/').filter(|s| !s.is_empty()).any(|segment| {
        let stem = segment.split('.').next().unwrap_or(segment);
        RESERVED_WINDOWS_DEVICE_NAMES
            .iter()
            .any(|&reserved| stem.eq_ignore_ascii_case(reserved))
    })
}

/// A source folder whose index page would live at a Windows reserved device
/// name gets no index page at all: every page under it is already skipped,
/// and the directory would otherwise be created for the index alone. Warns
/// once, naming the folder; `true` means skip.
pub(crate) fn warn_reserved_folder(dir: &str, mapped_index: &str) -> bool {
    if !is_reserved_device_output(mapped_index) {
        return false;
    }
    log::warn!(
        "Skipping folder '{dir}': its output directory would be a Windows reserved device name ({mapped_index}) — cannot exist on NTFS"
    );
    true
}

/// Admits `doc` to `docs`, unless [`is_reserved_device_output`] flags its
/// `url_path` — in which case the page is dropped, with one warning naming
/// the file, instead of failing the whole build. Called once per page from
/// the sequential reduce in `build::render::blocking`, before the
/// whole-corpus passes (slug dedup, folder synthesis, ...) can see it. The
/// returned index is the only valid document owner for any per-source state.
pub(crate) fn admit_unless_reserved_device_output(
    docs: &mut Vec<crate::build::types::ParsedDocument>,
    doc: crate::build::types::ParsedDocument,
) -> Option<usize> {
    if is_reserved_device_output(&doc.url_path) {
        log::warn!(
            "Skipping '{}': its output directory would be a Windows reserved device name ({}) — cannot exist on NTFS",
            doc.source_path.as_deref().unwrap_or(&doc.url_path),
            doc.url_path,
        );
        return None;
    }
    docs.push(doc);
    Some(docs.len() - 1)
}

/// Number the DIRECTORY, never the file.
///
/// `<dir>/index.html` is the only shape `to_pretty_url` gives a trailing
/// slash, and a trailing-slash URL is the only kind `ServeDir` routes — so
/// `<dir>/index-2.html` served as `<dir>/index-2` and 404'd with its bytes on
/// disk. `compute_url_path` returns `.../index.html` on every branch; the root
/// home (`index.html`) is the one page with no directory of its own, and gets
/// one here, so the result is directory-shaped unconditionally.
/// [`moss_core::page::page_map::resolve_duplicate_slugs`] over the build's
/// documents, writing each moved page's new address.
pub fn resolve_duplicate_slugs_with_lang(
    documents: &mut [ParsedDocument],
    site_lang: crate::i18n::Language,
) -> Vec<UrlCollision> {
    let entries: Vec<moss_core::page::page_map::SlugEntry<'_>> = documents.iter().map(|d| moss_core::page::page_map::SlugEntry {
        url_path: &d.url_path,
        is_folder: d.kind == PageKind::Folder,
        lang: d.lang,
        slot_only: d.slot_only,
        source_path: d.source_path.as_deref(),
    }).collect();
    let (moved, collisions) = moss_core::page::page_map::resolve_duplicate_slugs(&entries, site_lang);
    for (i, url_path) in moved {
        documents[i].url_path = url_path;
    }
    collisions
}

pub(crate) fn collision_messages(collisions: &[UrlCollision]) -> Vec<String> {
    collisions.iter().map(|c| {
        format!("{} and {} both publish to /{}. Give each source a unique address.", c.keeper, c.loser, c.wanted)
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // =========================================================================
    // Tests for generate_slug
    // =========================================================================

    #[test]
    fn test_generate_slug_basic() {
        assert_eq!(generate_slug("Hello World"), "hello-world");
        assert_eq!(generate_slug("My Blog Post"), "my-blog-post");
    }

    #[test]
    fn test_generate_slug_special_characters() {
        assert_eq!(generate_slug("API & Documentation"), "api-and-documentation");
        assert_eq!(generate_slug("Contact @ Email"), "contact-at-email");
        assert_eq!(generate_slug("C++ Programming"), "cplusplus-programming");
        assert_eq!(generate_slug("50% Off"), "50percent-off");
        assert_eq!(generate_slug("FAQ #1"), "faq-hash1");
    }

    #[test]
    fn test_generate_slug_underscores() {
        assert_eq!(generate_slug("file_name_example"), "file-name-example");
        assert_eq!(generate_slug("snake_case_title"), "snake-case-title");
    }

    #[test]
    fn test_generate_slug_remove_invalid_chars() {
        assert_eq!(generate_slug("Title (with) brackets"), "title-with-brackets");
        assert_eq!(generate_slug("Price: $99.99"), "price-99.99");
        assert_eq!(generate_slug("User/Admin/Settings"), "user-admin-settings");
    }

    #[test]
    fn test_generate_slug_consecutive_hyphens() {
        assert_eq!(generate_slug("Multiple   Spaces"), "multiple-spaces");
        assert_eq!(generate_slug("Too---Many-Hyphens"), "too-many-hyphens");
        assert_eq!(generate_slug("Mixed _-_ Separators"), "mixed-separators");
    }

    #[test]
    fn test_generate_slug_edge_cases() {
        assert_eq!(generate_slug(""), "untitled");
        assert_eq!(generate_slug("   "), "untitled");
        assert_eq!(generate_slug("---"), "untitled");
        assert_eq!(generate_slug("$@%!"), "atpercent");
    }

    #[test]
    fn test_generate_slug_unicode() {
        // Unicode letters should be preserved (CJK, accented chars, etc.)
        assert_eq!(generate_slug("Café Menu"), "café-menu");
        assert_eq!(generate_slug("Résumé Template"), "résumé-template");

        // CJK characters should be preserved
        assert_eq!(generate_slug("你好世界"), "你好世界");
        assert_eq!(generate_slug("日本語テスト"), "日本語テスト");
        assert_eq!(generate_slug("한국어 테스트"), "한국어-테스트");

        // Mixed CJK and ASCII
        assert_eq!(generate_slug("Hello 世界"), "hello-世界");
        assert_eq!(generate_slug("文章 123 Test"), "文章-123-test");
    }

    #[test]
    fn test_generate_slug_length_limit() {
        let very_long = "a".repeat(200);
        let result = generate_slug(&very_long);
        assert!(result.len() <= 100);
        assert_eq!(result, "a".repeat(100));
    }

    #[test]
    fn test_generate_slug_trim_hyphens() {
        assert_eq!(generate_slug("-leading hyphen"), "leading-hyphen");
        assert_eq!(generate_slug("trailing hyphen-"), "trailing-hyphen");
        assert_eq!(generate_slug("-both-"), "both");
    }

    // =========================================================================
    // Tests for generate_uid
    // =========================================================================

    #[test]
    fn a_uid_is_the_same_for_the_same_page_whatever_the_name_encoding() {
        let hash = "5e3c0a";
        let uid = generate_uid("posts/caf\u{e9}.md", hash);
        assert_eq!(generate_uid("posts/cafe\u{301}.md", hash), uid);
        assert_ne!(generate_uid("posts/other.md", hash), uid);
        assert_ne!(generate_uid("posts/caf\u{e9}.md", "77d1b4"), uid);
    }

    #[test]
    fn test_generate_uid_length() {
        // uid should be exactly 8 hex characters
        let uid = generate_uid("posts/my-article.md", "# A page\n");
        assert_eq!(uid.len(), 8, "uid should be 8 characters long");
        assert!(uid.chars().all(|c| c.is_ascii_hexdigit()), "uid should be hex characters only");
    }

    // =========================================================================
    // Tests for resolve_duplicate_slugs
    // =========================================================================

    /// Helper function to create a minimal ParsedDocument for testing
    fn make_test_doc(title: &str, url_path: &str) -> ParsedDocument {
        ParsedDocument {
            title: title.to_string(),
            label: title.to_string(),
            url_path: url_path.to_string(),
            reading_time: 1,
            slug: title.to_lowercase().replace(' ', "-"),
            permalink: format!("/{}", url_path), // allow:served-path-url-construct (test fixture — permalink field, not HTML-emitted URL)
            kind: PageKind::Article,
            ..Default::default()
        }
    }

    /// A `zh-hans` document — the losing side of a cross-language contest.
    fn zh(title: &str, url_path: &str) -> ParsedDocument {
        ParsedDocument { lang: crate::i18n::Language::ZhHans, ..make_test_doc(title, url_path) }
    }

    /// A folder index with a source path, which is what a collision names.
    fn folder(title: &str, url_path: &str, source: &str) -> ParsedDocument {
        ParsedDocument {
            kind: PageKind::Folder,
            source_path: Some(source.to_string()),
            ..make_test_doc(title, url_path)
        }
    }

    #[test]
    fn test_resolve_duplicate_slugs_no_duplicates() {
        let mut docs = vec![
            make_test_doc("First", "first.html"),
            make_test_doc("Second", "second.html"),
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, crate::i18n::Language::En);

        assert_eq!(docs[0].url_path, "first.html");
        assert_eq!(docs[1].url_path, "second.html");
    }

    #[test]
    fn a_multi_segment_url_colliding_with_a_real_page_is_numbered_in_place() {
        // `url: share/id` against a real `share/id.md`.
        let mut docs = vec![
            make_test_doc("Real", "share/id/index.html"),
            make_test_doc("Override", "share/id/index.html"),
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, crate::i18n::Language::En);

        assert_eq!(docs[0].url_path, "share/id/index.html");
        assert_eq!(docs[1].url_path, "share/id-2/index.html");
    }

    #[test]
    fn test_resolve_duplicate_slugs_with_duplicates() {
        let mut docs = vec![
            make_test_doc("Hello World", "hello.html"),
            make_test_doc("Hello Again", "hello.html"),
            make_test_doc("Hello Once More", "hello.html"),
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, crate::i18n::Language::En);

        assert_eq!(docs[0].url_path, "hello.html", "First should keep original");
        assert_eq!(
            docs[1].url_path, "hello-2/index.html",
            "Second is numbered into its own directory, so it stays servable"
        );
        assert_eq!(
            docs[2].url_path, "hello-3/index.html",
            "Third likewise"
        );
    }

    #[test]
    fn test_resolve_duplicate_slugs_with_directories() {
        let mut docs = vec![
            make_test_doc("Post One", "posts/test.html"),
            make_test_doc("Post Two", "posts/test.html"),
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, crate::i18n::Language::En);

        assert_eq!(docs[0].url_path, "posts/test.html");
        assert_eq!(docs[1].url_path, "posts/test-2/index.html");
    }

    #[test]
    fn test_resolve_duplicate_slugs_mixed_paths() {
        // Different directories should NOT conflict
        let mut docs = vec![
            make_test_doc("Post", "posts/test.html"),
            make_test_doc("Page", "pages/test.html"),
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, crate::i18n::Language::En);

        // Different directories - no conflict
        assert_eq!(docs[0].url_path, "posts/test.html");
        assert_eq!(docs[1].url_path, "pages/test.html");
    }

    /// The regression that earned the directory-numbering rule.
    ///
    /// A shape assertion alone does not say why the shape matters, so compose
    /// the two functions the real bug ran through: the deduplicated page has to
    /// come out of `to_pretty_url` as a trailing-slash directory URL, because
    /// that is the only form the preview's `ServeDir` can route. The old
    /// `<dir>/index-2.html` came out as the extensionless `<dir>/index-2` and
    /// 404'd with its bytes on disk.
    #[test]
    fn every_deduplicated_page_gets_a_servable_url() {
        use crate::build::scan::article_map::to_pretty_url;
        use crate::i18n::Language;

        // Two folders carrying the same `url:` override — a duplicated folder
        // keeps the original's — plus the root home, which has no directory of
        // its own to number.
        let mut docs = vec![
            make_test_doc("Comics", "awards/comics/index.html"),
            make_test_doc("Memory", "awards/comics/index.html"),
            make_test_doc("Home", "index.html"),
            make_test_doc("Home copy", "index.html"),
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, Language::En);

        for doc in &docs {
            let url = to_pretty_url(&doc.url_path);
            assert!(
                url.is_empty() || url.ends_with('/'),
                "`{}` serves as `/{}`, which ServeDir has no route for",
                doc.url_path,
                url
            );
        }
        assert_eq!(docs[1].url_path, "awards/comics-2/index.html");
        assert_eq!(docs[3].url_path, "index-2/index.html");
    }

    /// Three same-language losers against one primary of another language.
    ///
    /// The old lang-prefix branch had no counter, so all three landed on
    /// `zh-hans/about/index.html` — one address, three documents, last render
    /// wins. Fixed by making both branches claim.
    #[test]
    fn a_lang_prefix_is_claimed_once_and_then_numbered() {
        use crate::i18n::Language;

        let mut docs = vec![
            zh("About", "about/index.html"),
            zh("About copy", "about/index.html"),
            zh("About copy 2", "about/index.html"),
        ];
        docs[0].lang = Language::En;

        resolve_duplicate_slugs_with_lang(&mut docs, Language::En);

        assert_eq!(docs[0].url_path, "about/index.html");
        assert_eq!(docs[1].url_path, "zh-hans/about/index.html");
        assert_eq!(docs[2].url_path, "zh-hans/about-2/index.html");
    }

    /// A real page already sitting at the prefixed address is not clobbered.
    #[test]
    fn a_lang_prefix_never_overwrites_an_existing_page() {
        use crate::i18n::Language;

        let mut docs = vec![
            make_test_doc("About", "about/index.html"),
            zh("About zh", "about/index.html"),
            // A site with a literal `zh-hans/` folder of its own.
            make_test_doc("Squatter", "zh-hans/about/index.html"),
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, Language::En);

        assert_eq!(docs[2].url_path, "zh-hans/about/index.html");
        assert_eq!(docs[1].url_path, "zh-hans/about-2/index.html");
    }

    /// One duplicated folder is one collision, not one per page beneath it.
    ///
    /// The children's own `url:` segments are relative and correct — changing
    /// the parent's resolves them untouched — so reporting them buries the
    /// single edit that fixes the lot.
    #[test]
    fn only_the_folder_that_caused_the_cascade_is_reported() {
        use crate::i18n::Language;

        let mut docs = vec![
            folder("Comics", "awards/comics/index.html", "評選/繪本組/繪本組.md"),
            folder("Memory", "awards/comics/index.html", "評選/散文組/散文組.md"),
            folder("S1 orig", "awards/comics/s1/index.html", "評選/繪本組/第一屆/第一屆.md"),
            folder("S1 copy", "awards/comics/s1/index.html", "評選/散文組/第一屆/第一屆.md"),
        ];
        docs[2].kind = PageKind::Folder;
        docs[3].kind = PageKind::Folder;

        let reported = resolve_duplicate_slugs_with_lang(&mut docs, Language::En);

        assert_eq!(reported.len(), 1, "got {:?}", reported);
        assert_eq!(reported[0].loser, "評選/散文組/散文組.md");
        assert_eq!(reported[0].keeper, "評選/繪本組/繪本組.md");
        assert_eq!(reported[0].wanted, "awards/comics/");
        assert_eq!(reported[0].moved_to, "awards/comics-2/");
    }

    /// A cross-language pair resolved by its prefix is the designed outcome,
    /// not something to tell a bilingual author about on every rebuild.
    #[test]
    fn a_prefixed_translation_is_not_reported_as_a_collision() {
        use crate::i18n::Language;

        let mut docs = vec![
            make_test_doc("About", "about/index.html"),
            zh("About zh", "about/index.html"),
        ];

        assert!(resolve_duplicate_slugs_with_lang(&mut docs, Language::En).is_empty());
    }

    #[test]
    fn test_resolve_duplicate_slugs_with_lang_site_lang_wins_primary() {
        // ZH doc listed first, EN doc second. site_lang=En.
        // EN should get the primary slot, ZH should get lang-prefix directory.
        use crate::i18n::Language;

        let mut docs = vec![
            {
                let mut d = make_test_doc("你好世界", "hello/index.html");
                d.lang = Language::ZhHans;
                d
            },
            {
                let mut d = make_test_doc("Hello World", "hello/index.html");
                d.lang = Language::En;
                d
            },
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, Language::En);

        // EN doc should keep primary slot
        let en_doc = docs.iter().find(|d| d.lang == Language::En).unwrap();
        assert_eq!(en_doc.url_path, "hello/index.html", "EN should keep primary slot");

        // ZH doc should get lang-prefix directory
        let zh_doc = docs.iter().find(|d| d.lang == Language::ZhHans).unwrap();
        assert_eq!(zh_doc.url_path, "zh-hans/hello/index.html", "ZH should get lang-prefix directory");
    }

    #[test]
    fn test_resolve_duplicate_slugs_with_lang_same_lang_no_reorder() {
        // Both docs are same language — first one keeps primary (filesystem order)
        use crate::i18n::Language;

        let mut docs = vec![
            make_test_doc("First", "test/index.html"),
            make_test_doc("Second", "test/index.html"),
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, Language::En);

        assert_eq!(docs[0].url_path, "test/index.html");
        assert_eq!(docs[1].url_path, "test-2/index.html");
    }

    #[test]
    fn test_resolve_duplicate_slugs_homepage_translation() {
        // When index.md (is_index=true) and index.zh-hans.md (is_index=false, demoted)
        // both get url_path "index.html", the home file winner (is_index=true)
        // keeps the primary slot regardless of site language detection.
        // Regression test: previously index.zh-hans.md got url_path "index/index.html"
        // (a separate directory) instead of being deduplicated with the homepage.
        use crate::i18n::Language;

        let mut docs = vec![
            {
                let mut d = make_test_doc("My Site", "index.html");
                d.lang = Language::En;
                d.clean_stem = "index".to_string();
                d.kind = PageKind::Folder; // Winner of detect_home_file_in_folder
                d
            },
            {
                let mut d = make_test_doc("我的网站", "index.html");
                d.lang = Language::ZhHans;
                d.clean_stem = "index".to_string();
                d.kind = PageKind::Article; // Demoted translation
                d
            },
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, Language::En);

        let en_doc = docs.iter().find(|d| d.lang == Language::En).unwrap();
        assert_eq!(en_doc.url_path, "index.html", "EN homepage (winner) should keep primary slot");

        let zh_doc = docs.iter().find(|d| d.lang == Language::ZhHans).unwrap();
        assert_eq!(zh_doc.url_path, "zh-hans/index.html", "ZH homepage translation should get lang-prefix directory");
    }

    #[test]
    fn test_resolve_duplicate_slugs_homepage_winner_beats_site_lang() {
        // Even when site language is Chinese, the home file winner (is_index=true)
        // should keep index.html. This prevents language detection from displacing
        // the canonical homepage chosen by detect_home_file_in_folder.
        use crate::i18n::Language;

        let mut docs = vec![
            {
                let mut d = make_test_doc("My Site", "index.html");
                d.lang = Language::En;
                d.clean_stem = "index".to_string();
                d.kind = PageKind::Folder; // Winner
                d
            },
            {
                let mut d = make_test_doc("我的网站", "index.html");
                d.lang = Language::ZhHans;
                d.clean_stem = "index".to_string();
                d.kind = PageKind::Article; // Demoted translation
                d
            },
        ];

        // Site language is ZH, but the winner should still keep index.html
        resolve_duplicate_slugs_with_lang(&mut docs, Language::ZhHans);

        let en_doc = docs.iter().find(|d| d.lang == Language::En).unwrap();
        assert_eq!(en_doc.url_path, "index.html", "Home file winner should keep primary slot even when site_lang differs");

        let zh_doc = docs.iter().find(|d| d.lang == Language::ZhHans).unwrap();
        assert_eq!(zh_doc.url_path, "zh-hans/index.html", "Demoted translation should get lang-prefix directory");
    }

    #[test]
    fn test_resolve_duplicate_slugs_three_languages() {
        // EN, ZH-Hans, and ZH-Hant all produce the same slug.
        // EN (site_lang) keeps primary, each translation gets its own lang-prefix dir.
        use crate::i18n::Language;

        let mut docs = vec![
            {
                let mut d = make_test_doc("About", "about/index.html");
                d.lang = Language::En;
                d
            },
            {
                let mut d = make_test_doc("关于", "about/index.html");
                d.lang = Language::ZhHans;
                d
            },
            {
                let mut d = make_test_doc("關於", "about/index.html");
                d.lang = Language::ZhHant;
                d
            },
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, Language::En);

        let en = docs.iter().find(|d| d.lang == Language::En).unwrap();
        assert_eq!(en.url_path, "about/index.html", "EN keeps primary slot");

        let zh_hans = docs.iter().find(|d| d.lang == Language::ZhHans).unwrap();
        assert_eq!(zh_hans.url_path, "zh-hans/about/index.html", "ZH-Hans gets lang-prefix");

        let zh_hant = docs.iter().find(|d| d.lang == Language::ZhHant).unwrap();
        assert_eq!(zh_hant.url_path, "zh-hant/about/index.html", "ZH-Hant gets lang-prefix");
    }
}
