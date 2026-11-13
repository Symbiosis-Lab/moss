//! Map a served URL (or the site root) to the source page that produces it,
//! and list the pages a served build holds.
//!
//! The editor asks this when a site's root is opened, and again whenever the
//! preview navigates. It tries the build's article map first (the fast path,
//! which knows where the build really served each source), then lists the
//! disk for the file expected at the address (`Hello, World!.md` at
//! `hello-world/`), because the map is rewritten on every build and lags a
//! file the author just wrote. Everything here is Tauri-free;
//! the desktop command and the HTTP carrier both call [`resolve_page_source`].

use std::path::Path;

use crate::build::scan::article_map::ArticleMap;
use crate::editor::resolve::links::{expected_url, PageSource};
use crate::editor::resolve::page_source::{
    detect_home_source, detect_root_home_source, folder_page_source, root_page_source, HomeVerdict,
};
use crate::editor::resolve::takeover::{takeover_for, TakeoverKind};

/// Resolve `url_path` (a served URL, `""` or `"/"` for the root) to its source
/// page inside the vault at `folder_path`.
///
/// A URL nothing backs resolves to `source_path: None`, with the takeover the
/// author could create to claim it; that is a normal answer, not an error.
/// Without the build's map (none yet, during a first build, or one this moss
/// cannot read) and with no source on disk for a page below the root, the
/// answer is `source_known: false`: only the map tells a generated page from
/// one whose source has a lossy slug, so "nothing backs it" is not known.
pub fn resolve_page_source(url_path: &str, folder_path: &Path) -> Result<PageSource, String> {
    let moss_dir = folder_path.join(".moss");
    // A missing or unreadable map degrades to an empty one, so the disk
    // fallbacks below still run: they keep the editor from offering to create
    // a home that already exists.
    let mut unknown = (!crate::moss_paths::MossPaths::from_moss_dir(moss_dir.clone()).article_map().exists())
        .then(|| "no build has finished yet".to_string());
    let map = ArticleMap::load(&moss_dir).unwrap_or_else(|e| {
        log::debug!("[resolve_page_source] article-map unreadable ({e}); using source-truth fallback");
        unknown = Some(e);
        ArticleMap::default()
    });
    let normalized = url_path.trim_start_matches('/');
    let existing = |rel: &str| {
        let full = crate::build::scan::path_collision::on_disk(folder_path, rel);
        full.exists().then(|| full.to_string_lossy().to_string())
    };

    if let Some(info) = map.articles.get(normalized) {
        let source_path = (!info.source_path.is_empty())
            .then(|| existing(&info.source_path))
            .flatten();
        let syndicated = crate::build::types::syndicated_urls(info.frontmatter.get("syndicated"));
        if source_path.is_some() {
            return Ok(PageSource {
                source_path,
                is_dir: false,
                is_article: true,
                syndicated,
                syndicate: info.syndicate.clone(),
                takeover: None,
                source_pending: false,
                source_known: true,
            });
        }
    }

    if let Some(source_path) = map.pages.get(normalized).and_then(|src| existing(src)) {
        return Ok(PageSource {
            source_path: Some(source_path),
            is_dir: true,
            is_article: false,
            syndicated: vec![],
            syndicate: None,
            takeover: None,
            source_pending: false,
            source_known: true,
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

    // A served file that is no page (a PDF, a picture) has the file it was
    // built from as its source: nothing generated it, and an editor showing
    // that file is already on it.
    let mut settled = |found: Result<Option<String>, String>| {
        found.unwrap_or_else(|why| {
            unknown.get_or_insert(why);
            None
        })
    };
    let source_path = settled(page_on_disk(normalized, folder_path, root.name(), &map.dir_overrides))
        .or_else(|| settled(served_asset_source(normalized, folder_path, &map)).and_then(|src| existing(&src)));
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
    let not_known = unknown.filter(|_| source_path.is_none() && !normalized.is_empty());
    if let Some(why) = &not_known {
        log::debug!("[resolve_page_source] whether '{url_path}' has a source is not known yet: {why}");
    }
    Ok(PageSource {
        source_path,
        is_dir: normalized.is_empty(),
        is_article: false,
        syndicated: vec![],
        syndicate: None,
        takeover: takeover.filter(|_| not_known.is_none()),
        source_pending: false,
        source_known: not_known.is_none(),
    })
}

/// The page file a build would serve at `url`, found by listing the folders
/// the address passes through and comparing each entry's expected address,
/// the same one a new page is opened at. For a folder's own address it is the
/// home the election names, when that is one of the folder's home-named
/// files; any other verdict is left to the folder takeover. The path is the
/// listing's names joined with `/`, as a mapped page's is. Entries the site
/// walk leaves out are passed over. `Err` when a folder on the way cannot be
/// listed: what it holds is not known, which is not the same as nothing.
fn page_on_disk(
    url: &str,
    folder_path: &Path,
    root_name: &str,
    dir_overrides: &std::collections::HashMap<String, String>,
) -> Result<Option<String>, String> {
    use crate::build::scan::path_collision::{logical_path, nfc_order};
    let asked = url.trim_end_matches('/').trim_end_matches(".html").trim_end_matches("/index");
    // The root's home is elected before this is asked.
    if asked.is_empty() || asked == "index" {
        return Ok(None);
    }
    let served_at = |rel: &str| expected_url(rel, root_name, dir_overrides).map(|at| at.trim_matches('/').to_string());
    let layer_sources = crate::config::map_layers::declared_sources(folder_path);
    // Depth first in name order, so two names served at one address always give the same answer.
    let mut folders = vec![String::new()];
    while let Some(folder) = folders.pop() {
        let dir = folder_path.join(&folder);
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .and_then(|entries| entries.map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned())).collect())
            .map_err(|e| format!("'{}' could not be listed: {e}", dir.display()))?;
        names.sort_by(|a, b| nfc_order(a, b));
        let depth = folder.split('/').filter(|name| !name.is_empty()).count() + 1;
        let mut inside = Vec::new();
        let mut at_the_address = None;
        for name in names {
            let rel = if folder.is_empty() { name.clone() } else { format!("{folder}/{name}") };
            let path = dir.join(&name);
            let is_dir = path.is_dir();
            if crate::build::scan::classify::left_out(&path, is_dir, depth).is_some() || (is_dir && layer_sources.contains(&logical_path(&rel))) {
                continue;
            }
            if !is_dir {
                if moss_core::source_edit::policy::is_page_path(Path::new(&name)) && served_at(&rel).as_deref() == Some(asked) {
                    return Ok(Some(folder_path.join(rel).to_string_lossy().into_owned()));
                }
                continue;
            }
            // A folder is served where its home is.
            match served_at(&format!("{rel}/index.md")).as_deref().and_then(|at| asked.strip_prefix(at)) {
                Some("") => at_the_address = at_the_address.or(Some(rel)),
                Some(rest) if rest.starts_with('/') => inside.push(rel),
                _ => {}
            }
        }
        // A page beside a folder of the same address comes first.
        if let Some(rel) = at_the_address {
            let name = rel.rsplit('/').next().unwrap_or(&rel);
            return Ok(match detect_home_source(&folder_path.join(&rel), name, false) {
                HomeVerdict::File(home) if served_at(&format!("{rel}/{home}")).as_deref() == Some(asked) => {
                    Some(folder_path.join(format!("{rel}/{home}")).to_string_lossy().into_owned())
                }
                _ => None,
            });
        }
        folders.extend(inside.into_iter().rev());
    }
    Ok(None)
}

/// The source of the non-page file served at `url`, from the last build's
/// manifest. A page URL (the root, a folder, an `.html` file) never reads it.
/// `Err` when there is no manifest to read, or two files share the address:
/// which file is served there is not known, which is not the same as none.
fn served_asset_source(url: &str, folder_path: &Path, map: &ArticleMap) -> Result<Option<String>, String> {
    if url.is_empty() || url.ends_with('/') || url.ends_with(".html") {
        return Ok(None);
    }
    let hashes = crate::ops::watch::sweep::walk::read_baseline_from_disk(folder_path)
        .ok_or("no build manifest to read yet")?;
    crate::editor::resolve::links::asset_source_for_url(url, &hashes, &map.dir_overrides)
}

/// One page of the served build.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct PageInfo {
    /// The page's file under the served directory, e.g. `posts/article.html`.
    pub url_path: String,
    /// Its `<title>`, or a name made from its path when it has none.
    pub title: String,
}

/// Every HTML page under `served_dir`, sorted by path. The caller passes the
/// directory the preview is serving now: before the first seal and after a
/// rebuild that is not the promoted generation, and a guess would miss pages.
pub fn pages_in(served_dir: &Path) -> Vec<PageInfo> {
    if served_dir.symlink_metadata().is_err() {
        return Vec::new();
    }
    let mut pages: Vec<PageInfo> = walkdir::WalkDir::new(served_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file() && e.path().extension().is_some_and(|ext| ext == "html"))
        .filter_map(|e| {
            let url_path = e.path().strip_prefix(served_dir).ok()?.to_string_lossy().into_owned();
            let title = title_of(e.path()).unwrap_or_else(|| {
                url_path
                    .trim_end_matches(".html")
                    .trim_end_matches("/index")
                    .replace('/', " > ")
                    .replace(['-', '_'], " ")
            });
            Some(PageInfo { url_path, title })
        })
        .collect();
    pages.sort_by(|a, b| a.url_path.cmp(&b.url_path));
    pages
}

fn title_of(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let (_, after_open) = content.split_once("<title>")?;
    let (title, _) = after_open.split_once("</title>")?;
    let title = title.trim();
    (!title.is_empty()).then(|| title.to_string())
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
                        "date": null, "tags": [], "syndicate": ["example-network"],
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
        assert_eq!(r.syndicate.as_deref(), Some(&["example-network".to_string()][..]), "the channels its copies go to");
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

    /// The address spells neither name as the disk does: `Events` is served
    /// lowercase and `untitled 2` with a hyphen.
    #[test]
    fn a_page_no_build_has_mapped_yet_resolves_under_its_spelling_on_disk() {
        let dir = vault(&[], &[], &["Events/untitled.md", "Events/untitled 2.md", "Events/2026/Open Day.md"]);
        for (url, file) in [
            ("/events/untitled/", "Events/untitled.md"),
            ("/events/untitled-2/", "Events/untitled 2.md"),
            ("events/2026/open-day.html", "Events/2026/Open Day.md"),
            ("events/2026/open-day/index.html", "Events/2026/Open Day.md"),
        ] {
            let r = resolve_page_source(url, dir.path()).unwrap();
            assert_eq!(r.source_path, Some(dir.path().join(file).to_string_lossy().into_owned()), "{url}");
        }
    }

    #[test]
    fn an_unmapped_folder_address_resolves_to_the_home_inside_it() {
        let dir = vault(&[], &[], &["My Notes/index.md", "My Notes/other.md", "Field Work/Field Work.md"]);
        for (url, file) in [("/my-notes/", "My Notes/index.md"), ("field-work/index.html", "Field Work/Field Work.md")] {
            let r = resolve_page_source(url, dir.path()).unwrap();
            assert_eq!(r.source_path, Some(dir.path().join(file).to_string_lossy().into_owned()), "{url}");
        }
    }

    /// Every home candidate is expected at its folder's address; which one
    /// is the home is the build's election, not the first name listed.
    #[test]
    fn an_unmapped_folder_address_resolves_to_the_home_the_build_elects() {
        let homes: Vec<_> = ["index.en.md", "README.md", "docs.md"]
            .into_iter()
            .map(|beside| {
                let dir = vault(&[], &[], &["docs/index.md", &format!("docs/{beside}")]);
                let r = resolve_page_source("/docs/", dir.path()).unwrap();
                let home = r.source_path.map(|p| p.strip_prefix(&*dir.path().to_string_lossy()).unwrap().to_string());
                (beside, home)
            })
            .collect();
        let index = || Some("/docs/index.md".to_string());
        assert_eq!(homes, [("index.en.md", index()), ("README.md", index()), ("docs.md", index())]);
    }

    #[test]
    fn a_page_in_a_map_layers_source_folder_backs_no_address() {
        let dir = vault(&[], &[], &["maps/atlas/notes.md"]);
        std::fs::write(dir.path().join(".moss/config.toml"), "[map.layers.atlas]\nkind = \"streets\"\nsource = \"maps/atlas\"\n").unwrap();
        assert_eq!(resolve_page_source("/maps/atlas/notes/", dir.path()).unwrap().source_path, None);
    }

    #[test]
    fn what_the_site_walk_leaves_out_backs_no_address() {
        let dir = vault(&[], &[], &[".moss/x.md", "node_modules/x.md", ".drafts/x.md", "AGENTS.md", "posts/.draft.md", "other/.moss/config.toml", "other/x.md"]);
        for url in ["/moss/x/", "/node_modules/x/", "/drafts/x/", "/agents/", "/posts/draft/", "/other/x/"] {
            assert_eq!(resolve_page_source(url, dir.path()).unwrap().source_path, None, "{url}");
        }
    }

    #[test]
    fn two_names_served_at_one_address_resolve_to_the_first_in_name_order() {
        let dir = vault(&[], &[], &["a-b.md", "a b.md"]);
        let r = resolve_page_source("/a-b/", dir.path()).unwrap();
        assert_eq!(r.source_path, Some(dir.path().join("a b.md").to_string_lossy().into_owned()));
    }

    /// A folder that cannot be listed may hold the page: no offer to create it.
    #[cfg(unix)]
    #[test]
    fn a_folder_that_cannot_be_listed_leaves_the_source_unknown() {
        use std::os::unix::fs::PermissionsExt;
        let dir = vault(&[], &[], &["events/a.md"]);
        let events = dir.path().join("events");
        std::fs::set_permissions(&events, std::fs::Permissions::from_mode(0o000)).unwrap();
        let r = resolve_page_source("/events/b/", dir.path());
        std::fs::set_permissions(&events, std::fs::Permissions::from_mode(0o755)).unwrap();
        let r = r.unwrap();
        assert!(r.source_path.is_none() && !r.source_known && r.takeover.is_none());
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

    /// The preview on a served PDF or picture is on that file, not on a page
    /// moss generated: an editor showing the file keeps showing it.
    #[test]
    fn a_served_file_resolves_to_the_file_it_was_built_from() {
        let dir = vault(&[], &[], &["index.md", "doc.pdf", "My Files/pic.png"]);
        let stat = serde_json::json!({ "hash": "abc", "size": 1, "mtime": 1000 });
        std::fs::write(
            dir.path().join(".moss/build.nosync/hashes.json"),
            serde_json::json!({
                "files": { "index.html": "100644:a", "doc.pdf": "100644:a", "my-files/pic.png": "100644:a" },
                "sources": { "index.md": stat, "doc.pdf": stat, "My Files/pic.png": stat },
                "image_outputs": ["my-files/pic.webp"],
            })
            .to_string(),
        )
        .unwrap();
        for (url, file) in [("/doc.pdf", "doc.pdf"), ("/my-files/pic.webp", "My Files/pic.png")] {
            let r = resolve_page_source(url, dir.path()).unwrap();
            assert!(r.source_path.as_deref().is_some_and(|p| p.ends_with(file)), "{url}: {:?}", r.source_path);
            assert!(r.source_known && r.takeover.is_none() && !r.is_dir && !r.is_article, "{url}");
        }
        // A file the build does not serve is still nothing to edit.
        let missing = resolve_page_source("/missing.pdf", dir.path()).unwrap();
        assert!(missing.source_path.is_none() && missing.source_known);
        // Without a manifest, which file is served there is not known yet.
        std::fs::remove_file(dir.path().join(".moss/build.nosync/hashes.json")).unwrap();
        let unread = resolve_page_source("/doc.pdf", dir.path()).unwrap();
        assert!(unread.source_path.is_none() && !unread.source_known);
    }

    #[test]
    fn a_section_index_resolves_from_the_pages_map() {
        let dir = vault(&[], &[("posts/", "posts/index.md")], &["posts/index.md"]);
        let r = resolve_page_source("posts/", dir.path()).unwrap();
        assert!(r.is_dir && !r.is_article && r.source_path.is_some());
    }

    #[test]
    fn a_file_missing_from_the_map_is_found_on_disk() {
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
        std::fs::write(dir.path().join(".moss/build.nosync/article-map.json"), "{ \"articles\": 7 }").unwrap();
        let r = resolve_page_source("", dir.path()).unwrap();
        assert!(r.source_path.unwrap().ends_with("index.md"));
    }

    /// Without a map this moss can read (another version's format, an IO
    /// error, or none yet during a first build), a page nothing on disk backs
    /// may still have a source the build served elsewhere than expected, so
    /// it is unknown, never "generated". What the disk alone settles still
    /// resolves: a source at its expected address, and a real folder's missing home.
    #[test]
    fn without_a_readable_map_a_page_without_a_source_on_disk_is_unknown() {
        let dir = vault(&[("posts/hello-world/", "posts/Hello, World!.md")], &[], &["posts/Hello, World!.md", "notes/a.md"]);
        assert!(resolve_page_source("posts/hello-world/", dir.path()).unwrap().source_path.is_some(), "the map knows it");
        let generated = resolve_page_source("rss/", dir.path()).unwrap();
        assert!(generated.source_known && generated.source_path.is_none() && generated.takeover.is_none(), "a readable map settles a generated page");

        let map = dir.path().join(".moss/build.nosync/article-map.json");
        for state in ["a map in a format this moss cannot parse", "no map yet"] {
            if state == "no map yet" { std::fs::remove_file(&map).unwrap() } else { std::fs::write(&map, "{ \"articles\": 7 }").unwrap() }
            assert!(resolve_page_source("posts/hello-world/", dir.path()).unwrap().source_path.is_some(), "{state}: a lossy slug, found by its expected address");
            let rss = resolve_page_source("rss/", dir.path()).unwrap();
            assert!(!rss.source_known && rss.takeover.is_none(), "{state}");
            assert!(resolve_page_source("notes/a/", dir.path()).unwrap().source_path.is_some(), "{state}: a source at the URL");
            let folder = resolve_page_source("notes/", dir.path()).unwrap();
            assert!(folder.source_known && (folder.source_path.is_some() || folder.takeover.is_some()), "{state}: a real folder, settled by the disk");
        }
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

    #[test]
    fn a_page_is_named_by_its_title_or_else_its_path() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("posts")).unwrap();
        std::fs::write(dir.path().join("index.html"), "<head><title>  Home  </title></head>").unwrap();
        std::fs::write(dir.path().join("posts/first-note.html"), "<head><title></title></head>").unwrap();
        std::fs::write(dir.path().join("style.css"), "").unwrap();
        let pages = pages_in(dir.path());
        let named: Vec<(&str, &str)> = pages.iter().map(|p| (p.url_path.as_str(), p.title.as_str())).collect();
        assert_eq!(named, [("index.html", "Home"), ("posts/first-note.html", "posts > first note")]);
    }

    /// Once `current` exists, a rebuild stages pages the promoted generation
    /// lacks while the preview serves `staging/`; only the served directory finds them.
    #[test]
    fn pages_are_read_from_the_directory_the_preview_serves() {
        let dir = tempfile::TempDir::new().unwrap();
        let paths = crate::moss_paths::MossPaths::from_moss_dir(dir.path().join(".moss"));
        let staging = paths.staging_dir();
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("index.html"), "<title>Home</title>").unwrap();
        assert_eq!(pages_in(&paths.initial_serve_dir()).len(), 1, "before the first seal, staging is served");

        std::fs::create_dir_all(paths.generation_dir("gen001")).unwrap();
        paths.set_current_ptr("gen001").unwrap();
        std::fs::write(staging.join("second.html"), "<title>Second</title>").unwrap();
        assert_eq!(pages_in(&staging).len(), 2);
        assert_eq!(pages_in(&paths.initial_serve_dir()).len(), 0, "the promoted generation is not what is served");
    }
}
