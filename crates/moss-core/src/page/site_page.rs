//! What a whole-site pass reads about each page.
//!
//! The build's parsed document and a host's page record both implement
//! [`SitePage`], so a pass written once over the trait draws the same page
//! around an article for the build and for every host.

use std::collections::HashMap;

use crate::frontmatter_typed::{FrontMatter, SeriesField};
use crate::i18n::Language;
use crate::sort::{ResolvedSort, SortField, SortableDoc, SortableLabel};

/// The site's not-found page, served at `/404/`.
pub const NOT_FOUND_PAGE: &str = "404/index.html";

/// The per-page facts the whole-site passes read, beyond those sorting reads
/// (including the label chrome shows, [`SortableLabel`]).
///
/// Every method returns a borrow, so a pass that walks every page allocates
/// nothing per page.
pub trait SitePage: SortableDoc + SortableLabel {
    /// The layout the author wrote.
    fn layout(&self) -> Option<&str>;
    fn series(&self) -> Option<&SeriesField>;
    fn draft(&self) -> Option<bool>;
    fn listed(&self) -> Option<bool>;
    fn search_engines(&self) -> Option<bool>;
    /// A file whose content fills a slot (`footer.md`, a partial), not a page.
    fn slot_only(&self) -> bool;
    /// The hidden folder holding this page, as a url-path prefix.
    fn hidden_home(&self) -> Option<&str>;
    /// The folder's resolved sort for its direct children; `None` on a page.
    fn direct_children_sort(&self) -> Option<&ResolvedSort>;

    fn is_not_found_page(&self) -> bool {
        self.url_path() == NOT_FOUND_PAGE
    }

    /// Whether a listing of `folder` may name this page at all: false only
    /// for a page in a hidden folder that `folder` is outside of.
    fn reaches(&self, folder: &str) -> bool {
        self.hidden_home().is_none_or(|home| folder.starts_with(home))
    }

    /// Whether a listing of the folder `folder` (`a/b/`, url space; `""` for
    /// the root) shows this page. False for a draft, a page listed off-feed
    /// (`listed: false`) or kept from search engines, a slot file, the
    /// not-found page, and a page in a hidden folder `folder` is outside of.
    /// Every listing asks this, so a new visibility flag is added here once.
    fn is_listable_in(&self, folder: &str) -> bool {
        self.draft() != Some(true)
            && self.listed() != Some(false)
            && self.search_engines() != Some(false)
            && !self.slot_only()
            && !self.is_not_found_page()
            && self.reaches(folder)
    }

    /// Did the author write `layout: article` on this page? The authored
    /// intent, not the resolved template, which is also an article for every
    /// nested page that said nothing: a folder home is a step in its siblings'
    /// series only when its author called it a piece.
    fn is_article_layout(&self) -> bool {
        self.layout() == Some("article")
    }

    /// The folder's sort for its direct children, by date when none was resolved.
    fn resolve_for_direct_children(&self) -> ResolvedSort {
        self.direct_children_sort().cloned().unwrap_or(ResolvedSort {
            axis: crate::sort::SortAxis::Date,
            explicit_order: None,
            series_default: false,
        })
    }
}

/// Every fact a whole-site pass reads about one page, as owned values. A host
/// builds these from the site's texts; the build's documents produce the same
/// through [`PageFacts::from_page`], so a test can hold the two equal.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PageFacts {
    pub url_path: String,
    /// The event `start` if the page has one, else its `date`.
    pub date: Option<String>,
    pub weight: Option<i32>,
    pub declared_sort: Option<SortField>,
    pub clean_stem: String,
    pub is_folder_index: bool,
    pub order_match_name: String,
    pub label: String,
    pub layout: Option<String>,
    pub series: Option<SeriesField>,
    pub draft: Option<bool>,
    pub listed: Option<bool>,
    pub search_engines: Option<bool>,
    pub slot_only: bool,
    pub hidden_home: Option<String>,
    pub direct_children_sort: Option<ResolvedSort>,
}

impl PageFacts {
    pub fn from_page<P: SitePage + ?Sized>(page: &P) -> Self {
        Self {
            url_path: page.url_path().to_string(),
            date: page.date().map(str::to_string),
            weight: page.weight(),
            declared_sort: page.declared_sort().cloned(),
            clean_stem: page.clean_stem().to_string(),
            is_folder_index: page.is_folder_index(),
            order_match_name: page.order_match_name().to_string(),
            label: page.label().to_string(),
            layout: page.layout().map(str::to_string),
            series: page.series().cloned(),
            draft: page.draft(),
            listed: page.listed(),
            search_engines: page.search_engines(),
            slot_only: page.slot_only(),
            hidden_home: page.hidden_home().map(str::to_string),
            direct_children_sort: page.direct_children_sort().cloned(),
        }
    }
}

impl SortableDoc for PageFacts {
    fn url_path(&self) -> &str { &self.url_path }
    fn date(&self) -> Option<&str> { self.date.as_deref() }
    fn weight(&self) -> Option<i32> { self.weight }
    fn declared_sort(&self) -> Option<&SortField> { self.declared_sort.as_ref() }
    fn clean_stem(&self) -> &str { &self.clean_stem }
    fn is_folder_index(&self) -> bool { self.is_folder_index }
    fn order_match_name(&self) -> &str { &self.order_match_name }
}

impl SortableLabel for PageFacts {
    fn label(&self) -> &str { &self.label }
}

impl SitePage for PageFacts {
    fn layout(&self) -> Option<&str> { self.layout.as_deref() }
    fn series(&self) -> Option<&SeriesField> { self.series.as_ref() }
    fn draft(&self) -> Option<bool> { self.draft }
    fn listed(&self) -> Option<bool> { self.listed }
    fn search_engines(&self) -> Option<bool> { self.search_engines }
    fn slot_only(&self) -> bool { self.slot_only }
    fn hidden_home(&self) -> Option<&str> { self.hidden_home.as_deref() }
    fn direct_children_sort(&self) -> Option<&ResolvedSort> { self.direct_children_sort.as_ref() }
}

/// Every page of the site as the build sees it once all pages are read: its
/// address after home election, `url:` overrides and duplicate resolution,
/// its label, its hidden folder, its cascaded `sort`, and each folder's
/// resolved sort for its children. `texts` holds every page's source by
/// site-relative path; the bundle supplies the language and what the last
/// build inferred. `root_name` is the site folder's own name, which the
/// root's home may be named after.
pub fn site_pages(texts: &HashMap<String, String>, bundle: &super::SiteBundle, root_name: &str) -> SitePages {
    use super::page_map::{apply_cascading_dir_overrides, home_file_winners, home_overrides, page_map_entry, HomeClaim};
    let site_lang = Language::from_code(&bundle.lang).unwrap_or(Language::En);
    let mut paths: Vec<&str> = texts.keys().map(String::as_str).filter(|p| crate::source_edit::policy::is_page_path(std::path::Path::new(p))).collect();
    // Duplicate-address ties keep this order, so it is the build's walk order.
    paths.sort_by(|a, b| crate::slug::walk_order(a, b));
    let graph = crate::content_graph::ContentGraph::from_paths(&bundle.files.iter().map(String::as_str).collect::<Vec<_>>());
    let read: Vec<(FrontMatter, String)> = paths.iter()
        .map(|p| crate::frontmatter::read_page(&crate::resolve::with_frontmatter_links_resolved(&texts[*p], &graph, p)))
        .collect();

    // A folder with a verbatim `index.html` has no home election, as in the build.
    let html: Vec<&str> = bundle.files.iter().map(String::as_str)
        .filter(|f| std::path::Path::new(f).extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_lowercase().as_str(), "html" | "htm")))
        .collect();
    let reserved = crate::source_edit::policy::reserved_home_dirs(html.iter().copied(), &crate::source_edit::policy::compute_passthrough_roots(&html, &bundle.passthrough));
    let open = |path: &&str| !reserved.contains(crate::resolve::parent_dir(path));
    let claims: Vec<HomeClaim<'_>> = paths.iter().zip(&read).filter(|(path, _)| open(path))
        .map(|(path, (fm, _))| HomeClaim { path, home: fm.home, translation_key: fm.translation_key.as_deref() })
        .collect();
    let (overrides, _) = home_overrides(&claims, root_name);
    let winners = home_file_winners(paths.iter().copied().filter(open), root_name, &overrides);
    let mut dir_overrides = HashMap::new();
    let mut entries: Vec<(String, String, bool)> = paths.iter().zip(&read).map(|(path, (fm, _))| {
        let (url, is_index, dir_override) = page_map_entry(path, fm.url.as_deref(), root_name, &winners, &overrides);
        dir_overrides.extend(dir_override);
        (path.to_string(), url, is_index)
    }).collect();
    apply_cascading_dir_overrides(&mut entries, &dir_overrides);

    let mut pages = Vec::with_capacity(paths.len());
    let mut langs = Vec::with_capacity(paths.len());
    let mut cascades = Vec::with_capacity(paths.len());
    for ((path, url_path, _), (mut fm, body)) in entries.into_iter().zip(read) {
        cascades.push(fm.cascade.take());
        fill_inferred(&mut fm, &path, texts, &graph, bundle);
        let (page, lang) = one_page(&path, url_path, fm, &body, root_name, site_lang, bundle);
        pages.push(page);
        langs.push(lang);
    }

    let slug_entries: Vec<super::page_map::SlugEntry<'_>> = pages.iter().zip(&langs).zip(&paths)
        .map(|((p, lang), path)| super::page_map::SlugEntry {
            url_path: &p.url_path, is_folder: p.is_folder_index, lang: *lang, slot_only: p.slot_only, source_path: Some(path),
        })
        .collect();
    let (moved, _) = super::page_map::resolve_duplicate_slugs(&slug_entries, site_lang);
    for (i, url_path) in moved {
        pages[i].url_path = url_path;
    }

    let hidden = super::inherit::hidden_homes(pages.iter().map(|p| (p.url_path.as_str(), p.is_folder_index && p.search_engines == Some(false))));
    for (page, home) in pages.iter_mut().zip(hidden) {
        page.hidden_home = home;
    }

    // Of the keys a folder cascades, only `sort` is a fact a pass reads.
    let plan = super::inherit::cascade_plan(pages.iter().zip(&cascades).map(|(p, c)| (p.url_path.as_str(), c.is_some())));
    for step in plan {
        let Some(sort) = cascades[step.source].as_ref().and_then(|c| c.get("sort")) else { continue };
        let Ok(sort) = serde_yaml::from_value::<SortField>(sort.clone()) else { continue };
        for i in step.targets {
            pages[i].declared_sort.get_or_insert_with(|| sort.clone());
        }
    }

    let sorts: Vec<(usize, ResolvedSort)> = crate::sort::direct_children_sorts(&pages).into_iter().map(|(i, s, _)| (i, s)).collect();
    for (i, sort) in sorts {
        pages[i].direct_children_sort = Some(sort);
    }
    SitePages { paths: paths.into_iter().map(str::to_string).collect(), pages, langs }
}

/// Every page of a site, in the build's walk order: each one's source path,
/// facts and language at the same index.
#[derive(Debug, Clone, Default)]
pub struct SitePages {
    pub paths: Vec<String>,
    pub pages: Vec<PageFacts>,
    pub langs: Vec<Language>,
}

impl SitePages {
    /// The series navigation drawn on the page at `path`, as the build draws it.
    /// `url` turns a url path into a link.
    pub fn series_nav(&self, path: &str, url: &dyn Fn(&str) -> String, typesetting: Option<&str>) -> Option<String> {
        use super::series;
        let i = self.paths.iter().position(|p| p == path)?;
        let page = &self.pages[i];
        let parent = series::series_parent(page, &self.pages)?;
        series::nav(page, parent, &series::chain(&self.pages, parent), url, self.langs[i], typesetting)
    }
}

/// The `title` and `date` a page lists under `inferred:`, filled from the
/// files it still embeds, through the resolver the build uses: the first ready
/// derivation that has a field gives it, and the page's own value wins.
fn fill_inferred(fm: &mut FrontMatter, path: &str, texts: &HashMap<String, String>, graph: &crate::content_graph::ContentGraph, bundle: &super::SiteBundle) {
    use crate::resolve::embeds::derived::{Derivations, DerivedFragment, DerivedTable};
    let Some(listed) = fm.inferred.as_deref().filter(|l| !l.is_empty() && !bundle.derived.is_empty()) else { return };
    let mut table = DerivedTable::new();
    for (ext, fragment) in bundle.derivable.iter().flat_map(|(ext, fragments)| fragments.iter().map(move |f| (ext, f))) {
        table.declare(ext, fragment);
    }
    let reader = |file: &str, fragment: &str| match bundle.derived.get(file) {
        None => DerivedFragment::Pending,
        Some(record) if record.fragments.iter().any(|f| f == fragment) => DerivedFragment::Ready { markdown: String::new(), facts: record.facts.clone() },
        Some(_) => DerivedFragment::Failed { message: String::new() },
    };
    let facts = crate::resolve::resolve_content_with_derivations(
        path, &texts[path], graph, &|note| texts.get(note).cloned(), &Default::default(),
        &crate::asset_snapshot::AssetSnapshot::new(), Some(&Derivations { table: &table, reader: &reader }),
    ).derived_facts;
    if let Some(title) = facts.fact_for("title", fm.title.as_deref(), listed) {
        fm.title = Some(title);
    }
    if let Some(date) = facts.fact_for("date", fm.date.as_deref(), listed) {
        fm.date = Some(date);
    }
}

/// One page's own facts, before any other page is consulted, and its language.
fn one_page(
    path: &str,
    url_path: String,
    mut fm: FrontMatter,
    body: &str,
    root_name: &str,
    site_lang: Language,
    bundle: &super::SiteBundle,
) -> (PageFacts, Language) {
    use crate::heading;
    fm.normalize();
    let event_when = fm.start.as_deref().and_then(|s| crate::event::EventTime::parse(s).ok()).map(|t| t.sort_key());
    let dated = fm.date.is_some() || fm.start.is_some();
    let stem = std::path::Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if !dated {
        fm.date = crate::date::parse_filename_stamp(stem).map(|s| s.page_date());
    }
    let slot_only = crate::source_edit::policy::is_excluded_from_pages(path) || fm.partial == Some(true);
    let folder = path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let inferred = bundle.folder_langs.get(folder).and_then(|code| Language::from_code(code));
    let ancestor = crate::i18n::folder_language(path, inferred, slot_only);
    let (lang, clean_stem) = crate::i18n::resolve_document_language(fm.lang.as_deref(), if stem.is_empty() { "untitled" } else { stem }, site_lang, ancestor);

    let parent_name = folder.rsplit('/').next().filter(|_| !folder.is_empty()).unwrap_or(root_name);
    let by_name = crate::home::is_home_file(&stem.to_lowercase(), parent_name);
    let home_url = if folder.is_empty() { "index.html".to_string() } else { format!("{folder}/index.html") };
    let is_home_override = !by_name && url_path == home_url;
    let is_folder_index = by_name || is_home_override;

    let filename_title = heading::filename_text_with_root(path, Some(root_name));
    let label = if heading::is_untitled_stamp(path, fm.title.as_deref(), dated) {
        // The build reads the body after source hygiene, before link resolution.
        let (source, _) = crate::resolve::prepare_markdown_source(body, 0);
        heading::opening_words(&super::excerpt::extract_description(&source, bundle.math.unwrap_or(true))).unwrap_or(filename_title)
    } else {
        let state = heading::compute(heading::HeadingInputs {
            file_path: path,
            frontmatter_title: fm.title.as_deref(),
            body_markdown: body,
            root_folder_name: Some(root_name),
            is_home_override,
            slot_only,
            dated,
        });
        if state.text.is_empty() { filename_title } else { state.text }
    };
    let order_match_name = crate::sort::order_match_name(is_folder_index, Some(path), &url_path, &clean_stem).to_string();
    let page = PageFacts {
        date: event_when.or(fm.date),
        weight: fm.weight,
        declared_sort: fm.sort,
        is_folder_index,
        order_match_name,
        clean_stem,
        label,
        layout: fm.layout,
        series: fm.series,
        draft: fm.draft,
        listed: fm.listed,
        search_engines: fm.search_engines,
        slot_only,
        hidden_home: None,
        direct_children_sort: None,
        url_path,
    };
    (page, lang)
}

#[cfg(test)]
#[path = "site_page_tests.rs"]
mod tests;
