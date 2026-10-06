//! Map a served URL (or the site root) to the source page that produces it.
//!
//! The editor asks this when a site's root is opened, and again whenever the
//! preview navigates. It tries the build's article map first (the fast path,
//! which knows lossy slugs: a source named `Hello, World!.md` is served at
//! `hello-world/`), then reads the disk, because the map is rewritten on every
//! build and lags a file the author just wrote. Everything here is Tauri-free;
//! the desktop command and the HTTP carrier both call [`resolve_page_source`].

use std::path::Path;

use crate::build::scan::article_map::ArticleMap;
use crate::editor::resolve::links::{url_to_source_candidates, PageSource};
use crate::editor::resolve::page_source::{
    detect_root_home_source, folder_page_source, root_page_source,
};
use crate::editor::resolve::takeover::{takeover_for, TakeoverKind};

/// Resolve `url_path` (a served URL, `""` or `"/"` for the root) to its source
/// page inside the vault at `folder_path`.
///
/// A URL nothing backs resolves to `source_path: None`, with the takeover the
/// author could create to claim it; that is a normal answer, not an error.
pub fn resolve_page_source(url_path: &str, folder_path: &Path) -> Result<PageSource, String> {
    let moss_dir = folder_path.join(".moss");
    // An unreadable map degrades to an empty one: the map is rewritten on every
    // build, and a torn read must not skip the disk fallback below, which is
    // what keeps the editor from offering to create a home that already exists.
    let map = ArticleMap::load(&moss_dir).unwrap_or_else(|e| {
        log::debug!("[resolve_page_source] article-map unreadable ({e}); using source-truth fallback");
        ArticleMap::default()
    });
    let normalized = url_path.trim_start_matches('/');
    let existing = |rel: &str| {
        let full = folder_path.join(rel);
        full.exists().then(|| full.to_string_lossy().to_string())
    };

    if let Some(info) = map.articles.get(normalized) {
        let source_path = (!info.source_path.is_empty())
            .then(|| existing(&info.source_path))
            .flatten();
        let syndicated = info
            .frontmatter
            .get("syndicated")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        return Ok(PageSource {
            source_path,
            is_dir: false,
            is_article: true,
            syndicated,
            takeover: None,
            source_pending: false,
        });
    }

    if let Some(src) = map.pages.get(normalized) {
        return Ok(PageSource {
            source_path: existing(src),
            is_dir: true,
            is_article: false,
            syndicated: vec![],
            takeover: None,
            source_pending: false,
        });
    }

    // The root's home from the disk: `pages[""]` lags a freshly written
    // self-named home, and the URL candidates below only know `index.md`.
    let root = crate::vault_root::VaultRoot::resolve(folder_path);
    if normalized.is_empty() {
        if let Some(resolved) = root_page_source(detect_root_home_source(&root), folder_path) {
            return Ok(resolved);
        }
    }

    let source_path = url_to_source_candidates(normalized)
        .into_iter()
        .find_map(|c| existing(&c));
    // What would replace the page when nothing backs it, in the language the
    // build resolved (the root entry of `site-languages.json`); no build yet
    // means the default, never a content scan from a resolve.
    let takeover = if source_path.is_none() {
        let site_lang = crate::build::features::email::read_site_languages(folder_path)
            .and_then(|list| list.into_iter().find(|a| a.scope.is_empty()))
            .and_then(|a| crate::i18n::Language::from_code(&a.lang))
            .unwrap_or_else(crate::i18n::build_default_language);
        takeover_for(&map, folder_path, root.name(), normalized, site_lang)
    } else {
        None
    };
    // A real subfolder with no map entry may still hold a home on disk the
    // build has not registered (a folder template's seeded home).
    let takeover = match takeover {
        Some(t) if !normalized.is_empty() && t.kind == TakeoverKind::FolderHome => {
            return Ok(folder_page_source(t));
        }
        other => other,
    };
    Ok(PageSource {
        source_path,
        is_dir: normalized.is_empty(),
        is_article: false,
        syndicated: vec![],
        takeover,
        source_pending: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A vault whose article map holds `articles` (url key -> source path) and
    /// `pages`, with `files` written as sources.
    fn vault(articles: &[(&str, &str)], pages: &[(&str, &str)], files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let build = dir.path().join(".moss").join("build.nosync");
        std::fs::create_dir_all(&build).unwrap();
        let articles: serde_json::Map<String, serde_json::Value> = articles
            .iter()
            .map(|(k, src)| {
                (
                    k.to_string(),
                    serde_json::json!({
                        "source_path": src, "title": "T", "content": "", "url_path": k,
                        "date": null, "tags": [],
                        "frontmatter": { "syndicated": ["https://example.test/a"] }
                    }),
                )
            })
            .collect();
        let pages: serde_json::Map<String, serde_json::Value> =
            pages.iter().map(|(k, v)| (k.to_string(), (*v).into())).collect();
        std::fs::write(
            build.join("article-map.json"),
            serde_json::json!({ "articles": articles, "pages": pages }).to_string(),
        )
        .unwrap();
        for f in files {
            let p = dir.path().join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "---\ntitle: T\n---\nbody").unwrap();
        }
        dir
    }

    #[test]
    fn an_article_resolves_through_the_map_with_its_syndication() {
        let dir = vault(&[("posts/hello/", "posts/hello.md")], &[], &["posts/hello.md"]);
        let r = resolve_page_source("posts/hello/", dir.path()).unwrap();
        assert!(r.is_article && !r.is_dir);
        assert!(r.source_path.unwrap().ends_with("posts/hello.md"));
        assert_eq!(r.syndicated, vec!["https://example.test/a"]);
    }

    /// The served slug is not derivable from the file name; only the map knows.
    #[test]
    fn a_lossy_slug_resolves_to_its_source() {
        let dir = vault(&[("posts/hello-world/", "posts/Hello, World!.md")], &[], &["posts/Hello, World!.md"]);
        let r = resolve_page_source("/posts/hello-world/", dir.path()).unwrap();
        assert!(r.is_article);
        assert!(r.source_path.unwrap().ends_with("Hello, World!.md"));
    }

    #[test]
    fn the_root_resolves_to_the_home_page_with_or_without_a_slash() {
        let dir = vault(&[], &[], &["index.md"]);
        for url in ["", "/"] {
            let r = resolve_page_source(url, dir.path()).unwrap();
            assert!(r.is_dir && !r.is_article, "{url:?}");
            assert!(r.source_path.unwrap().ends_with("index.md"));
        }
    }

    #[test]
    fn a_home_registered_in_the_map_under_a_non_ascii_name_resolves() {
        let dir = vault(&[], &[("", "山水.md")], &["山水.md"]);
        let r = resolve_page_source("", dir.path()).unwrap();
        assert!(r.is_dir);
        assert!(r.source_path.unwrap().ends_with("山水.md"));
    }

    /// The build has not registered a self-named home yet; the disk decides.
    #[test]
    fn a_self_named_home_missing_from_the_map_is_found_on_disk() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("Garden");
        std::fs::create_dir_all(root.join(".moss")).unwrap();
        std::fs::write(root.join("Garden.md"), "---\nhome: true\ntitle: Garden\n---\nhi").unwrap();
        let r = resolve_page_source("", &root).unwrap();
        assert!(r.is_dir);
        assert!(r.source_path.unwrap().ends_with("Garden.md"));
    }

    #[test]
    fn a_section_index_resolves_from_the_pages_map() {
        let dir = vault(&[], &[("posts/", "posts/index.md")], &["posts/index.md"]);
        let r = resolve_page_source("posts/", dir.path()).unwrap();
        assert!(r.is_dir && !r.is_article && r.source_path.is_some());
    }

    #[test]
    fn a_file_missing_from_the_map_is_found_by_url_candidates() {
        let dir = vault(&[], &[], &["about.md"]);
        let r = resolve_page_source("about", dir.path()).unwrap();
        assert!(!r.is_article && !r.is_dir && r.source_path.is_some());
    }

    #[test]
    fn an_unknown_url_has_no_source() {
        let dir = vault(&[], &[], &[]);
        let r = resolve_page_source("nothing/here/", dir.path()).unwrap();
        assert!(!r.is_article && !r.is_dir && r.source_path.is_none());
    }

    #[test]
    fn an_unreadable_map_still_finds_the_home_on_disk() {
        let dir = vault(&[], &[], &["index.md"]);
        std::fs::write(dir.path().join(".moss/build.nosync/article-map.json"), "{ torn").unwrap();
        let r = resolve_page_source("", dir.path()).unwrap();
        assert!(r.source_path.unwrap().ends_with("index.md"));
    }

    /// The takeover branch: a URL no page backs, and what the editor offers.
    mod folder_home {
        use super::*;
        use crate::editor::resolve::takeover::TakeoverKind;

        fn slashed(s: &str) -> String {
            s.replace('\\', "/")
        }

        /// The (dir, display) a `FolderHome` takeover would write its home into.
        fn folder_home(ps: &PageSource) -> Option<(String, String)> {
            let t = ps.takeover.as_ref()?;
            assert_eq!(t.kind, TakeoverKind::FolderHome);
            Some((slashed(&t.files.last()?.dir), t.display.clone()))
        }

        fn write_map(dir: &Path, map: serde_json::Value) {
            std::fs::write(dir.join(".moss/build.nosync/article-map.json"), map.to_string()).unwrap();
        }

        #[test]
        fn a_nested_index_is_a_page_not_a_directory_surface() {
            let dir = vault(&[], &[], &["blog/2024/index.md"]);
            let r = resolve_page_source("blog/2024/", dir.path()).unwrap();
            assert!(!r.is_dir);
        }

        #[test]
        fn a_root_with_no_intentional_home_offers_the_takeover_not_a_random_page() {
            // Neither `index.md`, a self-named note nor a `home: true` marker:
            // the alphabetical fallback the build elects a home with must not
            // open `apple.md` as the root's source.
            let parent = tempfile::tempdir().unwrap();
            let root = parent.path().join("wild garden");
            std::fs::create_dir_all(root.join(".moss/build.nosync")).unwrap();
            write_map(&root, serde_json::json!({ "articles": {}, "pages": {} }));
            std::fs::write(root.join("apple.md"), "---\ntitle: Apple\n---\nbody").unwrap();
            std::fs::write(root.join("banana.md"), "---\ntitle: Banana\n---\nbody").unwrap();
            let r = resolve_page_source("", &root).unwrap();
            assert!(r.is_dir, "the root is still a directory surface");
            assert!(r.source_path.is_none());
            let t = r.takeover.expect("the root offers to create its home");
            assert_eq!(t.kind, TakeoverKind::RootHome);
        }

        /// A real folder with no map entry of its own: the editor recovers the
        /// on-disk name from the served slug, from a child's source or the disk.
        #[test]
        fn a_no_home_subfolder_resolves_its_lone_article_as_the_home() {
            let dir = vault(&[("field-notes/hello", "Field Notes/hello.md")], &[], &["Field Notes/hello.md"]);
            let r = resolve_page_source("field-notes/", dir.path()).unwrap();
            assert!(r.source_path.as_deref().is_some_and(|p| p.ends_with("hello.md")), "{:?}", r.source_path);
            assert!(r.is_dir, "a real folder reports is_dir");
            assert!(r.takeover.is_none(), "a folder whose article is its home is not offered a second one");
        }

        #[test]
        fn a_nested_no_home_subfolder_resolves_its_lone_article_as_the_home() {
            let dir = vault(
                &[("field-notes/2024/post", "Field Notes/2024/post.md")],
                &[],
                &["Field Notes/2024/post.md"],
            );
            let r = resolve_page_source("field-notes/2024/", dir.path()).unwrap();
            assert!(r.source_path.as_deref().is_some_and(|p| p.ends_with("post.md")), "{:?}", r.source_path);
            assert!(r.is_dir);
            assert!(r.takeover.is_none());
        }

        #[test]
        fn an_empty_on_disk_subfolder_offers_a_folder_home_under_its_real_name() {
            let dir = vault(&[], &[], &[]);
            std::fs::create_dir_all(dir.path().join("Open Shelf")).unwrap();
            let r = resolve_page_source("open-shelf/", dir.path()).unwrap();
            assert_eq!(r.source_path, None);
            assert!(r.is_dir);
            let want = format!("{}/Open Shelf", slashed(&dir.path().to_string_lossy()));
            assert_eq!(folder_home(&r), Some((want, "Open Shelf".into())));
        }

        /// The served slug (`photos`) differs from the directory (`写真`); the
        /// override the build recorded is what reverses it.
        #[test]
        fn an_empty_folder_renamed_by_a_dir_override_still_maps_back_to_its_directory() {
            let dir = vault(&[], &[], &[]);
            std::fs::create_dir_all(dir.path().join("写真")).unwrap();
            write_map(
                dir.path(),
                serde_json::json!({ "articles": {}, "pages": {}, "dir_overrides": { "写真": "photos" } }),
            );
            let r = resolve_page_source("photos/", dir.path()).unwrap();
            assert!(r.is_dir);
            let want = format!("{}/写真", slashed(&dir.path().to_string_lossy()));
            assert_eq!(folder_home(&r), Some((want, "写真".into())));
        }

        #[test]
        fn a_synthesized_page_with_no_backing_directory_offers_no_takeover() {
            let dir = vault(&[], &[], &[]);
            std::fs::create_dir_all(dir.path().join("Notes")).unwrap();
            let r = resolve_page_source("feed/", dir.path()).unwrap();
            assert_eq!(r.takeover, None, "no backing directory, no folder button");
            assert!(!r.is_dir);
        }
    }
}
