//! The address the build serves each page at, before and after a rename.
//!
//! A link written as `/notes/beta/` is kept verbatim by the build, so after a
//! rename it has to carry the address the build serves the page at, which is
//! not the page's file name: the build slugs names, honours a `url:` in the
//! page's frontmatter, cascades a folder home's `url:` to its children, and
//! numbers or language-prefixes two pages that would share an address. Rather
//! than derive that a second time, this runs the build's own page-map pass and
//! slug deduplication over the build's own file list. The pass for the tree
//! after the rename reads each page from the file where it is now, so working
//! the addresses out reads the site and writes nothing.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::build::icloud::is_evicted;
use crate::build::markdown::resolve_duplicate_slugs_with_lang;
use crate::build::scan::article_map::to_pretty_url;
use crate::build::scan::classify::{classify_extension, is_agent_config_name, is_excluded_dir_name, ScanBucket};
use crate::build::scan::page_map::pages_before_dedup;
use crate::build::types::ParsedDocument;
use crate::i18n::Language;
use crate::types::content::FileInfo;
use crate::vault_root::VaultRoot;

/// Why a page has no address to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NoAddress {
    /// The build would not make a page of the file.
    NotAPage,
    /// The file is in the cloud and not downloaded; the build leaves it out
    /// until it is, and working the addresses out never downloads it.
    Offline,
    /// The file could not be read, so the build leaves it out.
    Unreadable,
    /// Another page would claim the same address, and which of the two keeps
    /// it depends on the order the folder lists them, which is not known for a
    /// name that does not exist yet.
    Shared,
}

impl NoAddress {
    pub(super) fn reason(self) -> &'static str {
        match self {
            NoAddress::NotAPage => "would not be served as a page",
            NoAddress::Offline => "is in the cloud and not downloaded, so the address it is served at is not known",
            NoAddress::Unreadable => "could not be read, so the address it is served at is not known",
            NoAddress::Shared => "would claim the same address as another page, and which of them keeps it depends on the order the folder lists them",
        }
    }
}

/// The addresses of the site's pages before and after a set of moves, each
/// worked out on first use: only a link written as a published address needs
/// them, and they read every page's frontmatter.
pub(super) struct Addresses<'a> {
    root: VaultRoot,
    pre_files: &'a [String],
    post_files: &'a [String],
    before: OnceCell<ServedAddresses>,
    after: OnceCell<ServedAddresses>,
}

impl<'a> Addresses<'a> {
    /// `pre_files` and `post_files` are every file of the site before and
    /// after the moves, pairwise, in the order the site folder lists them.
    pub(super) fn new(root: &Path, pre_files: &'a [String], post_files: &'a [String]) -> Self {
        Self { root: VaultRoot::resolve(root), pre_files, post_files, before: OnceCell::new(), after: OnceCell::new() }
    }

    /// The page the site serves at `address` (`/a/b/`) before the moves.
    pub(super) fn page_at(&self, address: &str) -> Option<&str> {
        self.before.get_or_init(|| self.served(self.pre_files)).page_at(address)
    }

    /// The address the page at `post_path` is served at after the moves.
    pub(super) fn after_moves(&self, post_path: &str) -> Result<&str, NoAddress> {
        self.after.get_or_init(|| self.served(self.post_files)).of(post_path)
    }

    /// The addresses in `tree`, which is `pre_files` or `post_files`.
    fn served(&self, tree: &[String]) -> ServedAddresses {
        let mut nested = NestedSites::default();
        let pairs: Vec<(&str, &str)> = self
            .pre_files
            .iter()
            .zip(tree)
            .filter(|(_, then)| in_site(then) && !nested.holds(self.root.path(), then))
            .map(|(now, then)| (now.as_str(), then.as_str()))
            .collect();
        ServedAddresses::build(&self.root, site_language(&self.root, &pairs), &pairs)
    }
}

/// Page path -> the address it is served at (`/a/b/`), for one tree.
struct ServedAddresses {
    by_path: HashMap<String, Result<String, NoAddress>>,
    by_address: HashMap<String, String>,
}

impl ServedAddresses {
    /// `pairs` are `(path now, path in the tree this map is for)` for every
    /// file of the site, in the order the site folder lists them.
    fn build(root: &VaultRoot, site_lang: Language, pairs: &[(&str, &str)]) -> Self {
        let mut now_of: HashMap<&str, &str> = HashMap::new();
        let mut files: Vec<FileInfo> = Vec::new();
        for &(now, then) in pairs.iter().filter(|(_, then)| bucket(then) == ScanBucket::Page) {
            now_of.insert(then, now);
            files.push(file_info(then));
        }
        let locate = |then: &str| -> PathBuf { root.path().join(now_of.get(then).copied().unwrap_or(then)) };
        let pages = pages_before_dedup(&files, root, site_lang, &locate, &is_evicted);

        // A page moved under a new name takes a place in the folder listing
        // that is not known yet; an address that depends on that place is
        // `Shared`. The two extreme places bound every outcome that matters.
        let moved = |p: &ParsedDocument| p.source_path.as_deref().is_some_and(|s| now_of.get(s) != Some(&s));
        let (first, last): (Vec<_>, Vec<_>) = pages.iter().cloned().partition(|p| moved(p));
        let late = (!first.is_empty()).then(|| final_addresses([last.clone(), first.clone()].concat(), site_lang));
        let early = final_addresses([first, last].concat(), site_lang);
        let late = late.as_ref().unwrap_or(&early);

        // A page the passes left out was offline or unreadable.
        let left_out = |path: &str| if is_evicted(&locate(path)) { NoAddress::Offline } else { NoAddress::Unreadable };
        let mut by_path: HashMap<String, Result<String, NoAddress>> =
            files.iter().map(|f| (f.path.clone(), Err(left_out(&f.path)))).collect();
        for page in &pages {
            let Some(path) = page.source_path.clone() else { continue };
            let answer = match (early.get(&path), late.get(&path)) {
                _ if page.slot_only => Err(NoAddress::NotAPage),
                (Some(a), Some(b)) if a == b => Ok(a.clone()),
                _ => Err(NoAddress::Shared),
            };
            by_path.insert(path, answer);
        }
        let by_address = by_path.iter().filter_map(|(p, a)| Some((a.clone().ok()?, p.clone()))).collect();
        Self { by_path, by_address }
    }

    /// The address the page at `path` is served at.
    fn of(&self, path: &str) -> Result<&str, NoAddress> {
        match self.by_path.get(path) {
            Some(Ok(address)) => Ok(address),
            Some(Err(why)) => Err(*why),
            None => Err(NoAddress::NotAPage),
        }
    }

    fn page_at(&self, address: &str) -> Option<&str> {
        self.by_address.get(address).map(String::as_str)
    }
}

/// The site's language for one tree, as the build resolves it, which decides
/// which of two pages sharing an address keeps it: `[site] lang`, else the
/// `lang:` of the build's home page, else the language of the pages' text.
/// The home page is chosen the way the build's scan chooses it; a root page
/// moved under a new name is not at that name yet, so a `home: true` it
/// carries is not seen there.
fn site_language(root: &VaultRoot, pairs: &[(&str, &str)]) -> Language {
    let tree: Vec<FileInfo> = pairs
        .iter()
        .filter(|(_, then)| matches!(bucket(then), ScanBucket::Page | ScanBucket::Html | ScanBucket::Other))
        .map(|(_, then)| file_info(then))
        .collect();
    let homepage = crate::build::scan::scan::detect_homepage_file_in_folder_marked(&tree, root.name(), root.path());
    let homepage_now = homepage.and_then(|h| pairs.iter().find(|(_, then)| *then == h).map(|(now, _)| *now));
    let pages_now: Vec<FileInfo> =
        pairs.iter().filter(|(_, then)| bucket(then) == ScanBucket::Page).map(|(now, _)| file_info(now)).collect();
    let root_str = root.path().to_string_lossy();
    let declared = crate::build::site_config::read_project_config(&root_str).ok();
    let code = crate::i18n::detect::resolve_site_default_lang_with(
        declared.as_ref().and_then(|c| c.site_str("lang")),
        homepage_now,
        &pages_now,
        &root_str,
        &is_evicted,
    );
    Language::from_code(&code).unwrap_or(Language::En)
}

/// Each page's final address after slug deduplication, slot files left out.
fn final_addresses(mut pages: Vec<ParsedDocument>, site_lang: Language) -> HashMap<String, String> {
    resolve_duplicate_slugs_with_lang(&mut pages, site_lang);
    pages
        .into_iter()
        .filter(|p| !p.slot_only)
        .filter_map(|p| Some((p.source_path?, format!("/{}", to_pretty_url(&p.url_path)))))
        .collect()
}

/// Does the build's folder walk keep `rel`: in no hidden or excluded folder,
/// and not an agent instruction file in the site folder. Asked of a path, not
/// of a walk, because a path after the moves does not exist yet; the files
/// before the moves were walked by the same rule.
fn in_site(rel: &str) -> bool {
    let (dirs, name) = rel.rsplit_once('/').map_or(("", rel), |(d, n)| (d, n));
    !dirs.split('/').any(|d| !d.is_empty() && is_excluded_dir_name(d)) && !(dirs.is_empty() && is_agent_config_name(name))
}

/// The scan's category for `rel`, by its extension.
fn bucket(rel: &str) -> ScanBucket {
    classify_extension(&Path::new(rel).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase())
}

/// `rel` as the scan lists it.
fn file_info(rel: &str) -> FileInfo {
    let ext = Path::new(rel).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    FileInfo { path: rel.to_string(), file_type: ext, size: 0, modified: None }
}

/// Folders below the root that hold a site of their own, which the build
/// walks around. Looked up on disk once per folder.
#[derive(Default)]
struct NestedSites(HashMap<String, bool>);

impl NestedSites {
    fn holds(&mut self, root: &Path, rel: &str) -> bool {
        let mut dir = rel;
        while let Some((parent, _)) = dir.rsplit_once('/') {
            let is_site = *self.0.entry(parent.to_string()).or_insert_with(|| root.join(parent).join(".moss").is_dir());
            if is_site {
                return true;
            }
            dir = parent;
        }
        false
    }
}
