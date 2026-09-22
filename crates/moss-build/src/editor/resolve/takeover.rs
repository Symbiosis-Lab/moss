//! The file an author would create to take over a page moss generates.
//!
//! Four kinds of page exist on a built site with no source file behind them:
//! the root of a vault that has articles but no home note, an index-less
//! folder, an unclaimed term page in any namespace (and the namespace root
//! that lists them), and the home of a language tree that only filename-suffix
//! translations populate. Each is one operation from the editor's point of
//! view — write these files, then open this one — and this module computes
//! that recipe from the build's own record so the pane that shows the button
//! carries no per-kind branch and guesses nothing about the site.
//!
//! The record is the article-map (ADR-019): `terms` says which term URLs are
//! unclaimed, `generated` says which indexes the build synthesized,
//! `dir_overrides` says which real directory already serves a URL. Disk is
//! consulted only where the map cannot answer — [`derive_folder_identity`]'s
//! contract, which this module keeps: a directory it names either exists or
//! is stated as one to create, never invented as if it existed.
//!
//! Design: `docs/archive/2026-09-05-term-page-takeover-from-the-editor.md`.

use std::path::Path;

use crate::build::scan::article_map::ArticleMap;
use crate::i18n::Language;

use super::page_source::derive_folder_identity;

/// Which generated page the recipe replaces. The editor pane keys its copy on
/// this and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum TakeoverKind {
    /// The vault root has articles but no home note.
    RootHome,
    /// An index-less folder, including a term namespace root (`/authors/`).
    FolderHome,
    /// An unclaimed term's generated page, whatever namespace it is in.
    /// Which frontmatter field the claim is written through is
    /// [`Takeover::field`], not a variant of its own: a site can declare any
    /// number of kinds, and an enum that grew a variant per field would have
    /// to be regenerated every time one was added.
    TermPage,
    /// A language tree's synthesized home.
    LangHome,
}

pub use crate::vault::fs::NewFile;

/// The whole recipe. `files` are written in order and the LAST one is the
/// page the editor opens afterwards — the claim file for a term, the home
/// note otherwise. `display` is the name the pane shows: the term, the
/// folder, the root, the language.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Takeover {
    pub kind: TakeoverKind,
    pub display: String,
    /// For a [`TakeoverKind::TermPage`], the name-list field the claim is
    /// written through — `"editor"`, not `"editor_page"`. `None` for every
    /// other kind. The pane shows it, so a person offered a claim can see
    /// they are about to be recorded as an editor rather than an author.
    pub field: Option<String>,
    pub files: Vec<NewFile>,
}

/// The recipe for `url` (site-relative, no leading slash, trailing slash
/// optional), or `None` when the page has no takeover: it has a source, it
/// is a feed, it is a redirect stub. The caller has already established that
/// no source file backs the URL; this only decides what would.
///
/// `root_name` is the vault's own name (`VaultRoot::name`), which is what a
/// self-named root home is called. `site_lang` names the folder a term claim
/// lives in.
pub fn takeover_for(
    map: &ArticleMap,
    project_root: &Path,
    root_name: &str,
    url: &str,
    site_lang: Language,
) -> Option<Takeover> {
    let key = url.trim_matches('/');
    if key.is_empty() {
        return Some(Takeover {
            kind: TakeoverKind::RootHome,
            display: root_name.to_string(),
            field: None,
            files: vec![home_note(project_root, root_name)],
        });
    }
    if let Some(site) = map.terms.get(key).filter(|site| site.claimed_by.is_none()) {
        let (ns, _) = key.split_once('/')?;
        let kind = namespace_kind(map, ns, site_lang)?;
        // The claim goes through the field this person is actually reached
        // by: someone who only ever appears in `jury:` is offered
        // `jury_page:`, not `author_page:`. Ties go to the kind's own field
        // order, and a term with no members at all — a claim is its only
        // occurrence — to its first field.
        let field = map
            .fields_with_members
            .get(key)
            .and_then(|with_members| kind.fields.iter().find(|f| with_members.contains(f)))
            .or_else(|| kind.fields.first())?
            .clone();
        let claim_key = crate::build::terms::claim_field_key(&field)?;
        let folder = namespace_folder(map, project_root, ns, &kind.title);
        let mut files = folder.url_note(ns).into_iter().collect::<Vec<_>>();
        files.push(NewFile {
            dir: folder.dir,
            name: format!("{}.md", filename_for(&site.display)),
            frontmatter: fields(serde_json::json!({ claim_key: site.display })),
        });
        return Some(Takeover {
            kind: TakeoverKind::TermPage,
            display: site.display.clone(),
            field: Some(field),
            files,
        });
    }
    let generated = map.generated.iter().any(|g| g.trim_matches('/') == key);
    if let Some(kind) = generated.then(|| namespace_kind(map, key, site_lang)).flatten() {
        let folder = namespace_folder(map, project_root, key, &kind.title);
        let title = kind.title;
        // The root's own page is the folder's home note: the `url:` note when
        // the folder does not serve the namespace yet, a plain unlisted home
        // when it does. Either way the root stays as unlisted as the
        // synthesized one it replaces. A folder that has a home note of its
        // own and still does not serve the namespace is the author's to
        // route; nothing moss writes beside that note would.
        if folder.has_home && !folder.serves_url {
            return None;
        }
        let home = folder.url_note(key).unwrap_or_else(|| {
            home_note_in(&folder.dir, &folder.name, serde_json::json!({ "home": true, "listed": false }))
        });
        return Some(Takeover { kind: TakeoverKind::FolderHome, display: title, field: None, files: vec![home] });
    }
    if generated
        && !key.contains('/')
        && moss_core::home::is_known_language_code(key)
        && !project_root.join(key).is_dir()
    {
        // A language home is a translation of the root home, named after it:
        // `index.zh-hans.md` beside `index.md`, `刘果.zh-hans.md` beside
        // `刘果.md`. No root home means nothing to translate yet.
        let root_home = map.pages.get("")?;
        let stem = Path::new(root_home).file_stem()?.to_str()?;
        return Some(Takeover {
            kind: TakeoverKind::LangHome,
            display: moss_core::home::endonym(key).unwrap_or(key).to_string(),
            field: None,
            files: vec![NewFile {
                dir: project_root.to_string_lossy().to_string(),
                name: format!("{stem}.{}.md", key.to_lowercase()),
                frontmatter: serde_json::Map::new(),
            }],
        });
    }
    if let Some(lang) = moss_core::home::lang_tree_prefix(key).filter(|l| !project_root.join(l).is_dir()) {
        return translated_folder_home(map, project_root, &key[lang.len() + 1..], &lang.to_lowercase());
    }
    let (dir, name) = derive_folder_identity(map, project_root, key);
    let (dir, name) = (dir?, name?);
    Some(Takeover {
        kind: TakeoverKind::FolderHome,
        display: name.clone(),
        field: None,
        files: vec![home_note(Path::new(&dir), &name)],
    })
}

/// A subfolder's home in one more language, when the language is a filename
/// suffix rather than a directory: `/zh-hans/essays/` is served by
/// `essays/index.zh-hans.md`, and only beside a bare `essays/index.md`.
/// Beside a self-named `essays.md` the build elects the suffixed index as the
/// folder's one home and serves it at `/essays/` instead (probed
/// 2026-09-06), so such a folder is offered nothing rather than a file that
/// would move its default page, and so is a folder whose only index is
/// already suffixed: the build serves that one at `/essays/` as well. A
/// folder without a home gets the bare index too, empty, which renders as the
/// synthesized listing did; the translation is the file opened. A language
/// directory on disk never reaches here: its subfolders are ordinary folders
/// with ordinary homes.
fn translated_folder_home(map: &ArticleMap, project_root: &Path, folder_url: &str, lang: &str) -> Option<Takeover> {
    let (dir, name) = derive_folder_identity(map, project_root, folder_url);
    let (dir, name) = (dir?, name?);
    let index = |suffix: &str| NewFile { dir: dir.clone(), name: format!("index{suffix}.md"), frontmatter: serde_json::Map::new() };
    let translation = index(&format!(".{lang}"));
    // The build's record first, then the folder's own election on disk: a
    // bare index written a moment ago is not offered again, and a self-named
    // one is seen before it routes. The election runs with the root's gate
    // because only a proven home counts here: the build never serves a
    // folder's first article as its home, so the alphabetical fallback would
    // refuse the very folder this recipe exists for. An election still
    // waiting on the cloud offers nothing rather than guessing.
    use super::page_source::HomeVerdict;
    let home = match map.pages.get(&format!("{folder_url}/")) {
        Some(p) => Some(p.clone()),
        None => match super::page_source::detect_home_source(Path::new(&dir), &name, true) {
            HomeVerdict::File(rel) => Some(rel),
            HomeVerdict::StillArriving => return None,
            HomeVerdict::None => None,
        },
    };
    let files = match home.as_deref().map(|p| Path::new(p).file_stem()?.to_str()) {
        None => vec![index(""), translation],
        Some(Some(stem)) if moss_core::home::is_index_stem(stem) => vec![translation],
        Some(_) => return None,
    };
    Some(Takeover { kind: TakeoverKind::FolderHome, display: name, field: None, files })
}

/// The top-level directory a namespace's files live in, and whether it
/// serves `/<ns>/` already. Only a top-level folder can: a home note's
/// `url:` replaces its own segment only (`page_map::page_map_entry`), so a
/// nested `關於/作者群` with `url: authors` serves `/關於/authors/`, not the
/// namespace root, and is not this folder.
struct NamespaceFolder {
    /// Absolute path; may not exist yet.
    dir: String,
    /// The directory's own name, which its self-named home note is called.
    name: String,
    /// True when the folder's slug or its recorded `url:` is the namespace.
    serves_url: bool,
    /// True when the build already knows a home note for it, so no second
    /// one may be written: the claim still lands (it is by field, not by
    /// location) and the author routes the folder by editing that note.
    has_home: bool,
}

impl NamespaceFolder {
    /// The self-named, unlisted home note with `url: <ns>` that makes the
    /// folder serve the namespace, when it does not and can be made to.
    fn url_note(&self, ns: &str) -> Option<NewFile> {
        (!self.serves_url && !self.has_home).then(|| {
            home_note_in(&self.dir, &self.name, serde_json::json!({ "url": ns, "listed": false }))
        })
    }
}

/// The kind that owns this namespace, as the build recorded it — the one
/// place a namespace's title and its claim fields come from, so the editor
/// never re-reads config or assumes a field name from a URL prefix.
///
/// `None` when nothing in the map is in that namespace: a kind outlives its
/// dimension being switched off, and a real `authors/` directory on a site
/// with `[terms] author = false` is an ordinary folder that must not be
/// offered a term-page recipe.
///
/// A map written before kinds existed has none at all. Its two built-in
/// namespaces are reconstructed from their defaults, titled in the site
/// language, so an editor opened against a stale map still offers the right
/// recipe; the next build replaces the reconstruction with the record.
fn namespace_kind(map: &ArticleMap, ns: &str, site_lang: Language) -> Option<crate::build::terms::TermKind> {
    if !map.terms.keys().any(|k| k.split_once('/').is_some_and(|(n, _)| n == ns)) {
        return None;
    }
    if let Some(kind) = map.kinds.iter().find(|k| k.key == ns) {
        return Some(kind.clone());
    }
    crate::build::terms::BUILTIN_DEFAULT_FIELDS
        .iter()
        .find(|(key, _)| *key == ns)
        .map(|(key, field)| crate::build::terms::TermKind {
            key: key.to_string(),
            fields: vec![field.to_string()],
            title: crate::i18n::term_root_title(site_lang, key).to_string(),
        })
}

/// The real directory already serving the namespace wins — the one the
/// editor resolves for any folder URL (`derive_folder_identity`: a child
/// article's source, else the disk walk that reverses the slug and honors
/// `dir_overrides`, top level only). Otherwise the folder is named in the
/// site's language — the same title the namespace root's breadcrumb
/// carries, `作者` on a Chinese site — whether or not it exists yet; when
/// that title already slugs to the namespace (`Authors`), the folder is the
/// namespace itself, `authors/`, so a case-sensitive volume never ends up
/// with two folders for one URL.
fn namespace_folder(
    map: &ArticleMap,
    project_root: &Path,
    ns: &str,
    title: &str,
) -> NamespaceFolder {
    if let (Some(dir), Some(name)) = derive_folder_identity(map, project_root, ns) {
        return NamespaceFolder { dir, has_home: map.pages.contains_key(&format!("{ns}/")), name, serves_url: true };
    }
    let name = if moss_core::slug::generate_slug(title) == ns { ns } else { title };
    // A localized folder that exists but serves elsewhere (`作者/作者.md`
    // with `url: people`) has its home recorded under that other URL.
    let served_as = map.dir_overrides.get(name).map(String::as_str).unwrap_or(name);
    NamespaceFolder {
        dir: project_root.join(name).to_string_lossy().to_string(),
        name: name.to_string(),
        serves_url: name == ns,
        has_home: map.pages.contains_key(&format!("{served_as}/")),
    }
}

/// A self-named home note (`<name>.md` with `home: true`), which
/// `moss_core::home::is_home_file` elects as the folder's home at the root and
/// in every subfolder alike. `index` for a nameless root, so a bare `.md` is
/// never created.
fn home_note(dir: &Path, name: &str) -> NewFile {
    home_note_in(&dir.to_string_lossy(), name, serde_json::json!({ "home": true }))
}

fn home_note_in(dir: &str, name: &str, frontmatter: serde_json::Value) -> NewFile {
    let name = if name.is_empty() { "index" } else { name };
    NewFile { dir: dir.to_string(), name: format!("{name}.md"), frontmatter: fields(frontmatter) }
}

/// The object inside a `json!({..})` literal; every recipe here is one.
fn fields(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    match v {
        serde_json::Value::Object(m) => m,
        _ => unreachable!("frontmatter recipes are JSON objects"),
    }
}

/// A term's display name as a filename: the name itself unless the vault's
/// own guard would refuse it, then its slug. The claim field carries the real
/// name either way.
fn filename_for(display: &str) -> String {
    crate::vault::fs::sanitize_entry_name(display).unwrap_or_else(|_| moss_core::terms::term_slug(display))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::scan::article_map::ArticleInfo;
    use crate::build::terms::TermSite;

    fn map() -> ArticleMap {
        let mut m = ArticleMap::default();
        m.terms.insert(
            "authors/馬欣宜".into(),
            TermSite { display: "馬欣宜".into(), claimed_by: None },
        );
        m.terms.insert(
            "authors/scarly".into(),
            TermSite { display: "Scarly".into(), claimed_by: Some("about/ma/".into()) },
        );
        m.terms.insert("tags/城市".into(), TermSite { display: "城市".into(), claimed_by: None });
        m.generated = vec!["authors/".into(), "authors/馬欣宜/".into(), "tags/".into(), "zh-hans/".into()];
        m.pages.insert("".into(), "index.md".into());
        m
    }

    fn root() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn only(t: &Takeover) -> &NewFile {
        assert_eq!(t.files.len(), 1, "{t:?}");
        &t.files[0]
    }

    #[test]
    fn root_without_a_home_gets_a_self_named_home_note() {
        let dir = root();
        let t = takeover_for(&map(), dir.path(), "My Site", "/", Language::En).unwrap();
        assert_eq!(t.kind, TakeoverKind::RootHome);
        let f = only(&t);
        assert_eq!((f.name.as_str(), &serde_json::Value::Object(f.frontmatter.clone())), ("My Site.md", &serde_json::json!({ "home": true })));
        assert_eq!(f.dir, dir.path().to_string_lossy());
    }

    #[test]
    fn nameless_root_falls_back_to_index() {
        let dir = root();
        let t = takeover_for(&map(), dir.path(), "", "", Language::En).unwrap();
        assert_eq!(only(&t).name, "index.md");
    }

    #[test]
    fn unclaimed_author_on_an_english_site_is_one_file_in_authors() {
        let dir = root();
        let t = takeover_for(&map(), dir.path(), "s", "authors/馬欣宜/", Language::En).unwrap();
        assert_eq!((t.kind, t.display.as_str()), (TakeoverKind::TermPage, "馬欣宜"));
        assert_eq!(t.field.as_deref(), Some("author"));
        let f = only(&t);
        assert_eq!(f.dir, dir.path().join("authors").to_string_lossy(), "the English title slugs to the namespace, so the folder IS the namespace");
        assert_eq!(f.name, "馬欣宜.md");
        assert_eq!(serde_json::Value::Object(f.frontmatter.clone()), serde_json::json!({ "author_page": "馬欣宜" }));
    }

    #[test]
    fn unclaimed_author_on_a_chinese_site_adds_the_unlisted_root_home_that_serves_authors() {
        let dir = root();
        let t = takeover_for(&map(), dir.path(), "s", "authors/馬欣宜/", Language::ZhHant).unwrap();
        assert_eq!(t.files.len(), 2);
        let home = &t.files[0];
        assert_eq!((home.dir.as_str(), home.name.as_str()), (dir.path().join("作者").to_string_lossy().as_ref(), "作者.md"));
        assert_eq!(serde_json::Value::Object(home.frontmatter.clone()), serde_json::json!({ "url": "authors", "listed": false }));
        let claim = &t.files[1];
        assert_eq!((claim.dir.as_str(), claim.name.as_str()), (home.dir.as_str(), "馬欣宜.md"));
    }

    #[test]
    fn a_directory_already_serving_the_namespace_wins_over_the_localized_name() {
        let dir = root();
        std::fs::create_dir(dir.path().join("作者群")).unwrap();
        let mut m = map();
        m.dir_overrides.insert("作者群".into(), "authors".into());
        m.pages.insert("authors/".into(), "作者群/作者群.md".into());
        let t = takeover_for(&m, dir.path(), "s", "authors/馬欣宜/", Language::ZhHant).unwrap();
        assert_eq!(only(&t).dir, dir.path().join("作者群").to_string_lossy());
        // …and so does a plain on-disk `authors/`.
        let m = map();
        std::fs::create_dir(dir.path().join("authors")).unwrap();
        let t = takeover_for(&m, dir.path(), "s", "authors/馬欣宜/", Language::ZhHant).unwrap();
        assert_eq!(only(&t).dir, dir.path().join("authors").to_string_lossy());
    }

    #[test]
    fn a_nested_url_override_serves_its_parent_not_the_namespace() {
        // `關於/作者群` with `url: authors` serves `/關於/authors/`: a home
        // note's url replaces its own segment only. The claim goes to the
        // localized top-level folder, with the note that routes it.
        let dir = root();
        let mut m = map();
        m.dir_overrides.insert("關於/作者群".into(), "authors".into());
        let t = takeover_for(&m, dir.path(), "s", "authors/馬欣宜/", Language::ZhHant).unwrap();
        assert_eq!(t.files.len(), 2);
        assert_eq!(t.files[0].name, "作者.md");
        assert!(t.files[1].dir.ends_with("作者"));
    }

    #[test]
    fn a_localized_folder_without_a_url_note_still_gets_one() {
        // `作者/` exists on disk but nothing routes it to /authors/: the claim
        // lands there AND the url note is written, so the page is served where
        // the generated one was. When the folder already has a home note of
        // its own, no second home is written and the root offers nothing.
        let dir = root();
        std::fs::create_dir(dir.path().join("作者")).unwrap();
        let t = takeover_for(&map(), dir.path(), "s", "authors/馬欣宜/", Language::ZhHant).unwrap();
        assert_eq!(t.files.len(), 2);
        assert_eq!((t.files[0].name.as_str(), t.files[1].name.as_str()), ("作者.md", "馬欣宜.md"));

        let mut m = map();
        m.pages.insert("people/".into(), "作者/作者.md".into());
        m.dir_overrides.insert("作者".into(), "people".into());
        let t = takeover_for(&m, dir.path(), "s", "authors/馬欣宜/", Language::ZhHant).unwrap();
        assert_eq!(t.files.len(), 1, "the claim still lands, by field; no second home note");
        assert!(takeover_for(&m, dir.path(), "s", "authors/", Language::ZhHant).is_none());
    }

    #[test]
    fn the_harbor_layout_claims_the_next_author_beside_the_first() {
        // `作者/作者.md` (`url: authors`) and a claimed `作者/馬欣宜.md` exist;
        // the next unclaimed author lands in `作者/` with no second home note,
        // found through the claim's source path even before any disk walk.
        let mut m = map();
        m.terms.insert("authors/王五".into(), TermSite { display: "王五".into(), claimed_by: None });
        m.terms.get_mut("authors/馬欣宜").unwrap().claimed_by = Some("authors/馬欣宜/".into());
        m.dir_overrides.insert("作者".into(), "authors".into());
        m.pages.insert("authors/".into(), "作者/作者.md".into());
        m.articles.insert(
            "authors/馬欣宜/".into(),
            serde_json::from_value::<ArticleInfo>(serde_json::json!({
                "source_path": "作者/馬欣宜.md", "title": "馬欣宜", "content": "", "url_path": "authors/馬欣宜/", "date": null
            })).unwrap(),
        );
        let dir = root();
        let t = takeover_for(&m, dir.path(), "s", "authors/王五/", Language::ZhHant).unwrap();
        let f = only(&t);
        assert_eq!((f.dir.as_str(), f.name.as_str()), (dir.path().join("作者").to_str().unwrap(), "王五.md"));
    }

    #[test]
    fn a_capitalized_authors_directory_is_reused_not_shadowed() {
        // `Authors/` on disk slugs to `authors` and serves the namespace; the
        // claim lands in it rather than in a second `authors/` beside it.
        let dir = root();
        std::fs::create_dir(dir.path().join("Authors")).unwrap();
        let t = takeover_for(&map(), dir.path(), "s", "authors/馬欣宜/", Language::En).unwrap();
        assert_eq!(only(&t).dir, dir.path().join("Authors").to_string_lossy());
    }

    #[test]
    fn a_language_directory_is_a_folder_not_a_root_translation() {
        // `zh-hans/` with pages but no index is a folder home inside it, not
        // `index.zh-hans.md` beside the root home.
        let dir = root();
        std::fs::create_dir(dir.path().join("zh-hans")).unwrap();
        let mut m = map();
        m.generated.push("zh-hans/".into());
        let t = takeover_for(&m, dir.path(), "s", "zh-hans/", Language::En).unwrap();
        assert_eq!(t.kind, TakeoverKind::FolderHome);
        assert!(only(&t).dir.ends_with("zh-hans"));
    }

    #[test]
    fn claimed_term_and_unknown_url_have_no_takeover() {
        let dir = root();
        assert!(takeover_for(&map(), dir.path(), "s", "authors/scarly/", Language::En).is_none());
        assert!(takeover_for(&map(), dir.path(), "s", "rss.xml", Language::En).is_none());
    }

    #[test]
    fn tag_page_claims_with_tag_page() {
        let dir = root();
        let t = takeover_for(&map(), dir.path(), "s", "tags/城市/", Language::ZhHans).unwrap();
        assert_eq!((t.kind, t.field.as_deref()), (TakeoverKind::TermPage, Some("tags")));
        let claim = t.files.last().unwrap();
        assert_eq!(serde_json::Value::Object(claim.frontmatter.clone()), serde_json::json!({ "tag_page": "城市" }));
        assert!(claim.dir.ends_with("标签"), "{}", claim.dir);
    }

    /// A site that declared `[terms.people] fields = ["author", "editor",
    /// "jury"]`, with one person reached only through `jury:`.
    fn declared_people_map() -> ArticleMap {
        let mut m = ArticleMap::default();
        m.kinds = vec![crate::build::terms::TermKind {
            key: "people".into(),
            fields: vec!["author".into(), "editor".into(), "jury".into()],
            title: "People".into(),
        }];
        m.terms.insert("people/kane".into(), TermSite { display: "Kane".into(), claimed_by: None });
        m.generated = vec!["people/".into(), "people/kane/".into()];
        m
    }

    #[test]
    fn unclaimed_declared_kind_term_offers_a_term_page_takeover_through_its_actual_field() {
        let dir = root();
        let mut m = declared_people_map();
        m.fields_with_members.insert("people/kane".into(), vec!["jury".into()]);
        let t = takeover_for(&m, dir.path(), "s", "people/kane/", Language::En).unwrap();
        assert_eq!((t.kind, t.field.as_deref()), (TakeoverKind::TermPage, Some("jury")));
        let claim = t.files.last().unwrap();
        assert_eq!(
            serde_json::Value::Object(claim.frontmatter.clone()),
            serde_json::json!({ "jury_page": "Kane" }),
            "a person who only ever sits on a jury is not offered authorship"
        );
    }

    #[test]
    fn no_members_falls_back_to_the_first_declared_field() {
        let dir = root();
        let t = takeover_for(&declared_people_map(), dir.path(), "s", "people/kane/", Language::En).unwrap();
        assert_eq!(t.field.as_deref(), Some("author"));
    }

    #[test]
    fn a_declared_namespace_root_is_titled_by_its_kind_not_the_site_language() {
        let dir = root();
        let t = takeover_for(&declared_people_map(), dir.path(), "s", "people/", Language::ZhHant).unwrap();
        assert_eq!((t.kind, t.display.as_str()), (TakeoverKind::FolderHome, "People"));
        assert!(t.field.is_none());
    }

    #[test]
    fn a_real_folder_in_an_unused_namespace_is_an_ordinary_folder() {
        // `[terms] author = false` leaves the kind in the map with no fields
        // and no terms under it. An `authors/` folder there is the author's
        // own, and gets the ordinary folder-home recipe, not a namespace one.
        let dir = root();
        std::fs::create_dir(dir.path().join("authors")).unwrap();
        let mut m = ArticleMap::default();
        m.kinds = vec![crate::build::terms::TermKind {
            key: "authors".into(),
            fields: Vec::new(),
            title: "作者".into(),
        }];
        m.generated = vec!["authors/".into()];
        let t = takeover_for(&m, dir.path(), "s", "authors/", Language::ZhHant).unwrap();
        assert_eq!((t.kind, t.display.as_str()), (TakeoverKind::FolderHome, "authors"));
        assert_eq!(only(&t).name, "authors.md", "its own name, not the namespace title");
    }

    #[test]
    fn namespace_root_is_a_folder_home_named_in_the_site_language() {
        let dir = root();
        let t = takeover_for(&map(), dir.path(), "s", "authors/", Language::ZhHant).unwrap();
        assert_eq!((t.kind, t.display.as_str()), (TakeoverKind::FolderHome, "作者"));
        let f = only(&t);
        assert_eq!(f.name, "作者.md");
        assert_eq!(serde_json::Value::Object(f.frontmatter.clone()), serde_json::json!({ "url": "authors", "listed": false }));
        let t = takeover_for(&map(), dir.path(), "s", "tags/", Language::En).unwrap();
        assert_eq!(serde_json::Value::Object(only(&t).frontmatter.clone()), serde_json::json!({ "home": true, "listed": false }));
    }

    #[test]
    fn language_home_is_a_translation_of_the_root_home() {
        let dir = root();
        let t = takeover_for(&map(), dir.path(), "s", "zh-hans/", Language::En).unwrap();
        assert_eq!((t.kind, t.display.as_str()), (TakeoverKind::LangHome, "简体中文"));
        assert_eq!(only(&t).name, "index.zh-hans.md");
        let mut m = map();
        m.pages.insert("".into(), "刘果.md".into());
        assert_eq!(only(&takeover_for(&m, dir.path(), "s", "zh-hans/", Language::En).unwrap()).name, "刘果.zh-hans.md");
        m.pages.clear();
        assert!(takeover_for(&m, dir.path(), "s", "zh-hans/", Language::En).is_none());
    }

    #[test]
    fn index_less_folder_gets_its_real_name_from_a_child_article() {
        let dir = root();
        let mut m = map();
        m.articles.insert(
            "my-blog/hello/".into(),
            serde_json::from_value::<ArticleInfo>(serde_json::json!({
                "source_path": "My Blog/hello.md", "title": "", "content": "", "url_path": "my-blog/hello/", "date": null
            })).unwrap(),
        );
        let t = takeover_for(&m, dir.path(), "s", "my-blog/", Language::En).unwrap();
        assert_eq!((t.kind, t.display.as_str()), (TakeoverKind::FolderHome, "My Blog"));
        let f = only(&t);
        assert_eq!((f.dir.as_str(), f.name.as_str()), (dir.path().join("My Blog").to_string_lossy().as_ref(), "My Blog.md"));
    }

    #[test]
    fn a_suffix_layout_folder_gets_a_translated_index_beside_its_bare_one() {
        let dir = root();
        std::fs::create_dir(dir.path().join("essays")).unwrap();
        let mut m = map();
        m.pages.insert("essays/".into(), "essays/index.md".into());
        let t = takeover_for(&m, dir.path(), "s", "zh-hans/essays/", Language::En).unwrap();
        assert_eq!((t.kind, t.display.as_str()), (TakeoverKind::FolderHome, "essays"));
        let f = only(&t);
        assert_eq!((f.dir.as_str(), f.name.as_str()), (dir.path().join("essays").to_string_lossy().as_ref(), "index.zh-hans.md"));
        // No default home yet, only articles: the bare index comes along, and
        // the translation is opened. The folder's first article is not its home.
        m.pages.clear();
        m.generated.push("essays/".into());
        std::fs::write(dir.path().join("essays/a.md"), "a").unwrap();
        let t = takeover_for(&m, dir.path(), "s", "zh-hans/essays/", Language::En).unwrap();
        assert_eq!(t.files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["index.md", "index.zh-hans.md"]);
        // A self-named home would be displaced by the suffixed index: nothing is
        // offered, whether the build recorded it or it just landed on disk.
        m.pages.insert("essays/".into(), "essays/essays.md".into());
        assert!(takeover_for(&m, dir.path(), "s", "zh-hans/essays/", Language::En).is_none());
        m.pages.clear();
        std::fs::write(dir.path().join("essays/essays.md"), "").unwrap();
        assert!(takeover_for(&m, dir.path(), "s", "zh-hans/essays/", Language::En).is_none());
        // …and a suffixed index alone is already the folder's one home.
        std::fs::remove_file(dir.path().join("essays/essays.md")).unwrap();
        std::fs::write(dir.path().join("essays/index.zh-hans.md"), "").unwrap();
        assert!(takeover_for(&m, dir.path(), "s", "zh-hans/essays/", Language::En).is_none());
    }

    #[test]
    fn a_language_directory_subfolder_is_an_ordinary_folder_home() {
        let dir = root();
        std::fs::create_dir_all(dir.path().join("zh-hans/essays")).unwrap();
        let mut m = map();
        m.articles.insert(
            "zh-hans/essays/a/".into(),
            serde_json::from_value::<ArticleInfo>(serde_json::json!({
                "source_path": "zh-hans/essays/a.md", "title": "", "content": "", "url_path": "zh-hans/essays/a/", "date": null
            })).unwrap(),
        );
        let t = takeover_for(&m, dir.path(), "s", "zh-hans/essays/", Language::En).unwrap();
        let f = only(&t);
        assert_eq!((f.dir.as_str(), f.name.as_str()), (dir.path().join("zh-hans").join("essays").to_string_lossy().as_ref(), "essays.md"));
    }

    #[test]
    fn a_name_that_cannot_be_a_filename_slugs_but_still_claims_the_real_name() {
        let dir = root();
        let mut m = map();
        m.terms.insert("authors/a-b".into(), TermSite { display: "A/B".into(), claimed_by: None });
        let t = takeover_for(&m, dir.path(), "s", "authors/a-b/", Language::En).unwrap();
        let f = only(&t);
        assert_eq!(f.name, "a-b.md");
        assert_eq!(serde_json::Value::Object(f.frontmatter.clone()), serde_json::json!({ "author_page": "A/B" }));
    }
}
