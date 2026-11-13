//! The one write path for a source of items (a Ghost bundle, a WordPress
//! export): which pages the folder already has, the pages' final paths and
//! files, old addresses as redirects, and the import report.
use std::{collections::HashMap, path::Path};
use crate::{build::feeds::redirect_table::{Addr, Target}, vault::config::{load_managed_toml, subtable, write_managed_toml}};
use super::source::ImportResult;
use super::scrape::{run::{ScrapeProgress, ScrapeResult}, scope::UrlScope, writer::{origin_key, OutputPaths, PendingPage}};

/// How an import reports progress to its host.
pub(crate) type Progress<'a> = &'a (dyn Fn(ScrapeProgress) + Send + Sync);

/// The address a source's pages are placed under: its site, or a stand-in
/// when the source names none, in which case no page gets an `origin:`.
pub(crate) fn base_of(site: Option<&url::Url>) -> url::Url {
    site.cloned().unwrap_or_else(|| url::Url::parse("https://import.invalid/").expect("static url"))
}

/// A page of the folder an import names by its address.
pub(crate) struct HeldPage {
    pub path: String,
    /// A failed fetch's stub, which the page replaces once it imports.
    pub placeholder: bool,
    /// Written by this import, so it takes every field the source has.
    pub this_run: bool,
}

/// The folder's imported pages by the address each came from, read from the
/// one record of that: each page's own `origin:` field (a failed fetch's stub,
/// by the `external_url` it failed on). Read once per import from each file's
/// frontmatter and kept current as pages land; never stored, so a page whose
/// `origin:` the author removed, or one still in the cloud, is not recognised.
#[derive(Default)]
pub(crate) struct ImportedPages {
    pages: HashMap<String, HeldPage>,
}

impl ImportedPages {
    pub fn scan(folder: &Path) -> Self {
        let mut held = Self::default();
        let walk = walkdir::WalkDir::new(folder).into_iter()
            .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'));
        for entry in walk.filter_map(Result::ok) {
            let path = entry.path();
            if !entry.file_type().is_file() || path.extension().is_none_or(|e| e != "md") || crate::build::icloud::is_evicted(path) { continue; }
            let (Ok(fields), Ok(relative)) = (crate::editor::filesystem::read_frontmatter_only(path), path.strip_prefix(folder)) else { continue };
            let field = |key: &str| fields.get(key).and_then(moss_core::frontmatter::value_as_string);
            let (address, placeholder) = match (field("origin"), fields.contains_key("scrape_error")) {
                (Some(origin), _) => (origin, false),
                (None, true) => match field("external_url") { Some(url) => (url, true), None => continue },
                _ => continue,
            };
            let Some(key) = origin_key(&address) else { continue };
            let relative = relative.to_string_lossy().replace('\\', "/");
            // Of two pages from one address (copies an older import made), the shorter name.
            if held.pages.get(&key).is_none_or(|p| p.path.len() > relative.len()) {
                held.pages.insert(key, HeldPage { path: relative, placeholder, this_run: false });
            }
        }
        held
    }

    pub fn get(&self, key: &str) -> Option<&HeldPage> {
        self.pages.get(key)
    }

    /// The file holding the page imported from `address`, if the folder has one.
    pub fn path_of(&self, address: &str) -> Option<&str> {
        self.pages.get(&origin_key(address)?).filter(|p| !p.placeholder).map(|p| p.path.as_str())
    }

    /// Record the pages `paths` placed this run.
    pub fn note(&mut self, pages: &[PendingPage], paths: &OutputPaths) {
        for page in pages {
            let (Some(key), Some(path)) = (page.identity(), paths.get(&page.source)) else { continue };
            if !paths.kept(path) {
                self.pages.insert(key, HeldPage { path: path.clone(), placeholder: !page.imported, this_run: true });
            }
        }
    }

    /// Remove the failed fetches' stubs this run wrote, for a source that is
    /// written from elsewhere when its site cannot be reached.
    pub fn remove_stubs_of_this_run(&mut self, folder: &Path) {
        self.pages.retain(|_, page| {
            let stub = page.placeholder && page.this_run;
            if stub { let _ = std::fs::remove_file(folder.join(&page.path)); }
            !stub
        });
    }
}

/// Write a source's pages and old addresses into `folder`, add the report's
/// last line, write the report, and send the one `complete` event.
pub(crate) fn write_items(
    folder: &Path, site: Option<&url::Url>, held: &mut ImportedPages, pages: &[PendingPage],
    redirects: Vec<(String, String)>, mut report: Vec<String>, mut scrape: ScrapeResult, progress: Progress<'_>,
) -> Result<(ImportResult, OutputPaths), String> {
    let scope = UrlScope::new(base_of(site).as_str()).map_err(|e| e.to_string())?;
    let paths = OutputPaths::reserve_with(folder, &scope, pages, held)?;
    let written = paths.write(folder, pages, &HashMap::new(), true)?;
    held.note(pages, &paths);
    add_redirects(folder, site, &paths, redirects, &mut report)?;
    report.push(format!("Pages written: {}; pages the folder already had, their text kept: {}.", written.len(), pages.len() - written.len()));
    let lines: Vec<_> = report.iter().map(|s| format!("- {s}")).collect();
    super::page::write_note(folder, ".moss/data/import/report.md", &format!("# Import report\n\n{}\n", lines.join("\n")))?;
    scrape.success = true;
    scrape.total_pages += written.len();
    progress(ScrapeProgress { pages_scraped: scrape.total_pages, pages_failed: scrape.failed_pages, current_url: None, complete: true });
    Ok((ImportResult { scrape, report, subscribers: None }, paths))
}

/// Add `(from, to)` rows to `[redirects]`: `from` an exact path on the old
/// site, `to` an address on it, pointed at the page the import wrote for it.
/// A redirect the folder already has stays.
pub(crate) fn add_redirects(folder: &Path, site: Option<&url::Url>, paths: &OutputPaths, rows: Vec<(String, String)>, report: &mut Vec<String>) -> Result<(), String> {
    let config_path = folder.join(".moss/config.toml");
    let mut config = load_managed_toml(&config_path)?;
    let redirects = subtable(&mut config.root, "redirects")?;
    for (from, to) in rows {
        let Some(site) = site else { report.push(format!("Redirect {from} skipped: the source host is unknown.")); continue };
        let from = match own_path(&from, site) {
            Some(from) if !from.contains(['*', '[', ']', '(', ')', '^', '$', '|']) && Addr::parse(&from).is_ok() => from,
            _ => { report.push(format!("Redirect {from} skipped: source must be an exact path on the original host, without queries or fragments.")); continue; }
        };
        let to = own_path(&to, site).unwrap_or(to);
        let to = if let Ok(mut address) = site.join(&to) {
            let suffix = format!("{}{}", address.query().map(|q| format!("?{q}")).unwrap_or_default(), address.fragment().map(|f| format!("#{f}")).unwrap_or_default());
            address.set_query(None); address.set_fragment(None);
            paths.get(address.as_str()).map(|p| format!("{}{suffix}", served_path(p))).unwrap_or(to)
        } else { to };
        if Target::parse(&to).is_err() { report.push(format!("Redirect {from} skipped: unsupported target.")); continue; }
        let decoded = |p: &str| urlencoding::decode(p).map(|d| d.into_owned()).unwrap_or_else(|_| p.to_string());
        if decoded(&from) == decoded(&to) { continue; }
        if redirects.contains_key(&from) { report.push(format!("Existing redirect {from} was preserved.")); continue; }
        redirects.insert(from, toml::Value::String(to));
    }
    std::fs::create_dir_all(folder.join(".moss")).map_err(|e| e.to_string())?;
    write_managed_toml(&config_path, &config.original, &config.root, crate::config::migrations::CURRENT_VERSION)
}

fn own_path(value: &str, site: &url::Url) -> Option<String> {
    if value.starts_with('/') && !value.starts_with("//") { return Some(value.into()); }
    let url = url::Url::parse(value).ok()?;
    (url.origin() == site.origin() && url.query().is_none() && url.fragment().is_none()).then(|| url.path().into())
}

fn served_path(path: &str) -> String {
    let stem = Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or("index");
    let output = moss_core::frontmatter_typed::compute_url_path(path, stem == "index", None, stem);
    format!("/{}", crate::build::scan::article_map::to_pretty_url(&output))
}
