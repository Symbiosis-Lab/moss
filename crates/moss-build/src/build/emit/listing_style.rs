//! A folder listed in a style a plugin draws.
//!
//! The render phase draws moss's own listing for every folder home page, and
//! when the page's `children_style` is not one of moss's styles it wraps that
//! listing in a pair of marker comments. After render, [`apply`] hands the
//! plugin that owns the style the pages the listing shows, and the slot pass
//! ([`settle_spans`]) replaces the span with what the plugin returns. When
//! nothing owns the style, or the plugin fails, the markers are stripped and
//! the default listing stays, so a missing plugin costs a warning and never a
//! page.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use serde::Serialize;

use crate::build::enhance::{EnhanceContent, EnhanceResult, ResolvedSlots};
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::ServedPath;
use crate::build::folder_embed::members::{in_listing_order, is_folder_home, listing_members, own_dir};
use crate::build::types::ParsedDocument;
use crate::plugins::contributions::listing_style::{is_builtin_style, ListingStylePlan};
use crate::plugins::manager::listing::{
    HeadEntry, ListingContext, ListingResult, ListingWarning, PageRecord, TranslationRecord, LISTING_MODEL,
};
use crate::types::content::ProjectStructure;

const BEGIN: &str = "<!-- moss:listing:begin ";
const END: &str = "<!-- moss:listing:end -->";

/// How to reach the plugins that may draw listings; absent where plugins are
/// off. Nothing is started until a page actually names a plugin style.
#[derive(Clone)]
pub(crate) struct ListingHost {
    plugins: Arc<crate::plugins::manager::ManagerCache>,
    folder: String,
    runtime: tokio::runtime::Handle,
}

/// The loaded plugins: who owns each style, and the manager to call.
struct Loaded {
    plan: ListingStylePlan,
    manager: Arc<crate::plugins::manager::PluginManager>,
}

impl std::fmt::Debug for ListingHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListingHost").field("folder", &self.folder).finish()
    }
}

impl ListingHost {
    /// Call from inside the async build: it captures the runtime the
    /// pipeline's blocking thread will `block_on`.
    pub(crate) fn new(plugins: &Arc<crate::plugins::manager::ManagerCache>, folder: &str) -> Self {
        Self { plugins: plugins.clone(), folder: folder.to_string(), runtime: tokio::runtime::Handle::current() }
    }

    /// `None` when the plugin layer cannot start (the process hook reports that).
    fn load(&self) -> Option<Loaded> {
        let manager = self.plugins.get_or_create(&self.folder).ok()?;
        let plan = ListingStylePlan::from_plugins(&manager.plugins());
        for conflict in &plan.conflicts {
            crate::build::cli_output::log_warn_problem!("{conflict}");
        }
        Some(Loaded { plan, manager })
    }

    fn call(&self, loaded: &Loaded, plugin: &str, ctx: &ListingContext) -> Result<ListingResult, String> {
        self.runtime.block_on(loaded.manager.execute_listing(plugin, ctx))
    }
}

/// What replaces a span: the plugin's HTML and the page width it asked for.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ListingFill {
    html: String,
    content_width: Option<String>,
}

impl ParsedDocument {
    /// The plugin-style id this folder home page asks for, when it draws a
    /// listing and names a style other than moss's own.
    pub(crate) fn plugin_listing_style(&self) -> Option<&str> {
        let draws = self.children != Some(false)
            && self.children_in.as_deref() != Some("sidebar")
            && !self.from_sidebar_alias.unwrap_or(false);
        let style = self.children_style.as_ref()?.value.as_str();
        (is_folder_home(self) && draws && !style.is_empty() && !is_builtin_style(style)).then_some(style)
    }
}

/// The non-built-in `children_style` a page's own frontmatter writes.
fn written_style(doc: &ParsedDocument) -> Option<&str> {
    doc.raw_frontmatter.get("children_style")?.as_str().filter(|s| !s.is_empty() && !is_builtin_style(s))
}

/// The page's listing, marked so the slot pass can swap it.
pub(crate) fn wrap(doc: &ParsedDocument, listing_html: String) -> String {
    match doc.plugin_listing_style() {
        Some(style) => format!("{BEGIN}{style} -->{listing_html}{END}"),
        None => listing_html,
    }
}

/// Replace (or unwrap) the marked span in a rendered page.
pub(crate) fn settle_spans(html: &str, fill: Option<&ListingFill>) -> String {
    let Some(start) = html.find(BEGIN) else { return html.to_string() };
    let Some(open_end) = html[start..].find(" -->").map(|i| start + i + " -->".len()) else {
        return html.to_string();
    };
    let Some(end) = html[open_end..].find(END).map(|i| open_end + i) else { return html.to_string() };
    let after = end + END.len();
    let mut out = String::with_capacity(html.len());
    out.push_str(&html[..start]);
    match fill {
        Some(fill) => out.push_str(&fill.html),
        None => out.push_str(&html[open_end..end]),
    }
    out.push_str(&settle_spans(&html[after..], fill));
    match fill.and_then(|f| f.content_width.as_deref()) {
        Some(width) => with_content_width(out, width),
        None => out,
    }
}

/// Give the `<body>` a width when the page, its folders and the site set none.
fn with_content_width(html: String, width: &str) -> String {
    let Some(at) = html.find("<body") else { return html };
    let tag_end = html[at..].find('>').map_or(html.len(), |i| at + i);
    if html[at..tag_end].contains("data-content-width") {
        return html;
    }
    let insert = at + "<body".len();
    format!("{} data-content-width=\"{}\"{}", &html[..insert], width, &html[insert..])
}

/// Directory of a served page, as segments: `board/sub/index.html` is
/// `["board", "sub"]`.
fn dir_segments(url_path: &str) -> Vec<String> {
    let dir = url_path.rsplit_once('/').map_or("", |(dir, _)| dir);
    dir.split('/').filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// The path from directory `from` to `to_segments`, `../` as needed.
fn relative(from: &[String], to: &[String]) -> String {
    let common = from.iter().zip(to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<&str> = vec![".."; from.len() - common];
    parts.extend(to[common..].iter().map(String::as_str));
    parts.join("/")
}

/// Rewrite the relative references in a page body so they resolve from
/// `to_dir` instead of from the page `page_url_path`.
fn rebase_html(html: &str, page_url_path: &str, to_dir: &[String]) -> String {
    static ATTR: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r#"((?:src|srcset|poster|href)=")([^"]*)""#).unwrap());
    let page_dir = dir_segments(page_url_path);
    let rebase_one = |url: &str| -> String {
        let skip = url.is_empty()
            || url.starts_with('/')
            || url.starts_with('#')
            || url.contains("://")
            || url.split(':').next().is_some_and(|scheme| url.contains(':') && scheme.chars().all(|c| c.is_ascii_alphabetic()));
        if skip {
            return url.to_string();
        }
        let (path, suffix) = match url.find(['?', '#']) {
            Some(i) => url.split_at(i),
            None => (url, ""),
        };
        let mut segments = page_dir.clone();
        for part in path.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    segments.pop();
                }
                other => segments.push(other.to_string()),
            }
        }
        let mut rebased = relative(to_dir, &segments);
        if path.ends_with('/') && !rebased.is_empty() {
            rebased.push('/');
        }
        if rebased.is_empty() {
            rebased.push_str("./");
        }
        format!("{rebased}{suffix}")
    };
    ATTR.replace_all(html, |caps: &regex::Captures| {
        let value = if caps[1].starts_with("srcset") {
            caps[2]
                .split(", ")
                .map(|candidate| match candidate.split_once(' ') {
                    Some((url, descriptor)) => format!("{} {descriptor}", rebase_one(url)),
                    None => rebase_one(candidate),
                })
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            rebase_one(&caps[2])
        };
        format!("{}{}\"", &caps[1], value)
    })
    .into_owned()
}

fn lang_tag(doc: &ParsedDocument) -> String {
    doc.lang_tag.clone().unwrap_or_else(|| doc.lang.as_bcp47_attr().to_string())
}

/// The page's frontmatter as moss resolved it: the inherited values applied
/// and the title filled in.
fn resolved_frontmatter(doc: &ParsedDocument) -> BTreeMap<String, serde_json::Value> {
    let mut frontmatter = doc.raw_frontmatter.clone();
    for (key, value) in doc.cascade.iter().flatten() {
        if let (false, Ok(json)) = (frontmatter.contains_key(key), serde_json::to_value(value)) {
            frontmatter.insert(key.clone(), json);
        }
    }
    frontmatter.insert("title".to_string(), serde_json::Value::String(doc.title.clone()));
    frontmatter
}

/// The body without the heading moss draws from the page's title: the record
/// carries the title as a field, and a plugin that lists the page draws its own.
fn without_title(html: &str) -> String {
    static HEADING: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r#"(?s)<h1 class="moss-article-title"[^>]*>.*?</h1>\n?"#).unwrap());
    // The editor's source-line markers are not part of a published page either.
    crate::build::ship::strip_source_annotations(&HEADING.replacen(html, 1, "")).into_owned()
}

/// A page's directory address ("a/b/") as seen from the listing directory.
fn relative_url(listing_dir: &[String], served: &str) -> String {
    let to: Vec<String> = served.trim_end_matches("index.html").split('/').filter(|s| !s.is_empty()).map(str::to_string).collect();
    match relative(listing_dir, &to) {
        url if url.is_empty() => "./".to_string(),
        url if served.ends_with('/') || served.ends_with("index.html") => format!("{url}/"),
        url => url,
    }
}

fn record(doc: &ParsedDocument, listing_dir: &[String], with_body: bool) -> PageRecord {
    let mut own_dir = dir_segments(&doc.url_path);
    let url = relative_url(listing_dir, &doc.url_path);
    // The folder the page sits in, inside the listed folder.
    own_dir.pop();
    let folder = if own_dir.starts_with(listing_dir) { own_dir[listing_dir.len()..].join("/") } else { String::new() };
    PageRecord {
        title: doc.title.clone(),
        url_path: doc.url_path.clone(),
        url,
        lang: lang_tag(doc),
        translations: doc
            .translations
            .iter()
            .map(|t| TranslationRecord { lang: t.lang_tag.clone(), url_path: t.url_path.clone() })
            .collect(),
        frontmatter: resolved_frontmatter(doc),
        html: if with_body { rebase_html(&without_title(&doc.html_content), &doc.url_path, listing_dir) } else { String::new() },
        folder,
    }
}

/// `terms[namespace][name]` for every name the listed pages use.
fn term_addresses(
    pages: &[&ParsedDocument],
    index: &crate::build::terms::TermIndex,
    listing_dir: &[String],
) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for doc in pages {
        for kind in index.kinds() {
            for field in &kind.fields {
                for name in crate::build::terms::field_names(doc, field) {
                    let name = name.trim();
                    let Some(url) = index.term_url(&kind.key, name) else { continue };
                    out.entry(kind.key.clone()).or_default().insert(name.to_string(), relative_url(listing_dir, url.trim_start_matches('/')));
                }
            }
        }
    }
    out
}

/// Why a file the plugin returned is not written, or `None` when it may be.
fn refusal(path: &str, listing_dir: &str, is_taken: impl Fn(&str) -> bool) -> Option<&'static str> {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') || path.contains('\0') {
        return Some("the path must be relative to the listing page");
    }
    if path.split('/').any(|part| part == "..") {
        return Some("the path leaves the listing page's folder");
    }
    let served = if listing_dir.is_empty() { path.to_string() } else { format!("{listing_dir}/{path}") };
    if path.starts_with("_moss/") || served.starts_with("_moss/") {
        return Some("`_moss/` is reserved for moss");
    }
    if is_taken(&served) {
        return Some("something else in the build is already served there");
    }
    None
}

/// Run the owning plugin for every folder home page that names one of its
/// styles, and record what replaces each listing. A page whose style nothing
/// provides gets one warning and keeps the default listing.
pub(crate) fn apply(
    documents: &[ParsedDocument],
    project: &ProjectStructure,
    terms: &crate::build::terms::TermIndex,
    stage_dir: &Path,
    pending: &mut PendingManifest,
    slots: &mut ResolvedSlots,
) {
    let hosts: Vec<&ParsedDocument> = documents.iter().filter(|d| d.plugin_listing_style().is_some()).collect();
    // Pages that write a style of their own but draw no listing with it: the
    // static schema no longer judges the value, so a typo is caught here.
    let strays: Vec<(&ParsedDocument, &str)> = documents
        .iter()
        .filter(|d| d.plugin_listing_style().is_none())
        .filter_map(|d| Some((d, written_style(d)?)))
        .collect();
    if hosts.is_empty() && strays.is_empty() {
        return;
    }
    let host = slots.listing_host().cloned();
    let loaded = host.as_ref().and_then(ListingHost::load);
    for (page, style) in strays {
        if !loaded.as_ref().is_some_and(|l| l.plan.owner(style).is_some()) {
            crate::build::cli_output::log_warn_problem!(
                "[{}] `children_style: {style}` is not one of moss's styles or a listing style any installed plugin provides here",
                page.source_path.as_deref().unwrap_or(&page.url_path)
            );
        }
    }
    // Every address the build serves that is not yet in the manifest: the
    // pages, and the source files copied through under their served names.
    let sources = [&project.html_files, &project.notebook_files, &project.other_files]
        .into_iter()
        .flatten()
        .map(|f| &f.path)
        .chain(project.image_files.iter().chain(&project.video_files).map(|m| &m.path));
    let mut taken: HashSet<String> = documents.iter().map(|d| d.url_path.clone()).collect();
    taken.extend(sources.filter_map(|p| ServedPath::from_source(p).ok()).map(|sp| sp.as_str().to_string()));
    // A plugin that failed once is not asked again this build.
    // plugin -> (where and why it failed first, pages that fell back).
    let mut failed: BTreeMap<String, (String, usize)> = BTreeMap::new();
    for page in hosts {
        let style = page.plugin_listing_style().unwrap_or_default();
        let owner = loaded.as_ref().and_then(|l| l.plan.owner(style));
        let (Some(loaded), Some(owner)) = (loaded.as_ref(), owner) else {
            crate::build::cli_output::log_warn_problem!(
                "[{}] `children_style: {style}` is not a listing style any installed plugin provides here, so moss's own listing is drawn",
                page.source_path.as_deref().unwrap_or(&page.url_path)
            );
            continue;
        };
        let plugin = owner.plugin.as_str();
        let name = page.source_path.as_deref().unwrap_or(&page.url_path);
        if let Some((_, fell_back)) = failed.get_mut(plugin) {
            *fell_back += 1;
            continue;
        }
        let dir = own_dir(page).unwrap_or_default();
        let listing_dir: Vec<String> = dir.split('/').filter(|s| !s.is_empty()).map(str::to_string).collect();
        let children = listing_members(page, documents, project);
        // The root's address prefix is empty, so without the tree check a
        // root listing would take in every language tree's folders as well.
        let tree = moss_core::home::lang_tree_prefix(&page.url_path);
        let folders: Vec<&ParsedDocument> = documents
            .iter()
            .filter(|d| {
                d.kind == moss_core::PageKind::Folder
                    && d.source_path.is_some()
                    && d.url_path != page.url_path
                    && d.is_public_page()
                    && dir_segments(&d.url_path).starts_with(&listing_dir)
                    && moss_core::home::lang_tree_prefix(&d.url_path) == tree
            })
            .collect();
        let ctx = ListingContext {
            model: LISTING_MODEL,
            style: style.to_string(),
            page: record(page, &listing_dir, true),
            pages: children.iter().map(|d| record(d, &listing_dir, true)).collect(),
            folders: in_listing_order(page, documents, &folders).iter().map(|d| record(d, &listing_dir, true)).collect(),
            terms: term_addresses(&children, terms, &listing_dir),
        };
        let result = match host.as_ref().map_or(Err("plugins are off".to_string()), |h| h.call(loaded, plugin, &ctx)) {
            Ok(result) => result,
            Err(e) => {
                failed.insert(plugin.to_string(), (format!("the `{style}` listing of {name} failed ({e})"), 1));
                continue;
            }
        };
        for warning in &result.warnings {
            match warning {
                ListingWarning::Plain(text) => crate::build::cli_output::log_warn_problem!("[{plugin}] {text}"),
                ListingWarning::About { text, page } => {
                    let source = page
                        .as_deref()
                        .and_then(|url_path| documents.iter().find(|d| d.url_path == url_path))
                        .and_then(|d| d.source_path.as_deref());
                    match source {
                        Some(source) => crate::build::cli_output::log_warn_problem!("[{plugin}] {source}: {text}"),
                        None => crate::build::cli_output::log_warn_problem!("[{plugin}] {text}"),
                    }
                }
            }
        }
        let mut head = String::new();
        for entry in &result.head {
            match entry {
                HeadEntry::Stylesheet(href) => head.push_str(&format!(
                    "<link rel=\"stylesheet\" href=\"{}\">\n",
                    moss_core::media::html_escape(href)
                )),
                HeadEntry::Script(src) => {
                    head.push_str(&format!("<script src=\"{}\" defer></script>\n", moss_core::media::html_escape(src)))
                }
            }
        }
        if !head.is_empty() {
            let pages = std::collections::HashMap::from([(page.url_path.clone(), head)]);
            let mut declared = std::collections::HashMap::new();
            declared.insert("head-end".to_string(), EnhanceContent::PerPage { pages });
            slots.merge(&EnhanceResult { success: true, slots: declared }, 500, plugin);
        }
        for file in &result.files {
            if let Some(reason) = refusal(&file.path, &dir, |served| taken.contains(served) || pending.is_registered(served)) {
                crate::build::cli_output::log_warn_problem!(
                    "[{plugin}] the file `{}` from the `{style}` listing of {name} was not written: {reason}",
                    file.path
                );
                continue;
            }
            let bytes = match (&file.text, &file.base64) {
                (Some(text), None) => text.clone().into_bytes(),
                (None, Some(encoded)) => {
                    use base64::Engine as _;
                    match base64::engine::general_purpose::STANDARD.decode(encoded) {
                        Ok(bytes) => bytes,
                        Err(_) => {
                            crate::build::cli_output::log_warn_problem!(
                                "[{plugin}] the file `{}` from the `{style}` listing of {name} was not written: its base64 is not valid",
                                file.path
                            );
                            continue;
                        }
                    }
                }
                _ => {
                    crate::build::cli_output::log_warn_problem!(
                        "[{plugin}] the file `{}` from the `{style}` listing of {name} was not written: give exactly one of `text` and `base64`",
                        file.path
                    );
                    continue;
                }
            };
            let served = if dir.is_empty() { file.path.clone() } else { format!("{dir}/{}", file.path) };
            let written = ServedPath::from_source(&served).map_err(|e| e.to_string()).and_then(|sp| {
                crate::build::context::BuildContext::for_render(stage_dir, pending)
                    .emit(&sp, &bytes, HashBucket::Files)
                    .map(|()| sp.as_str().to_string())
                    .map_err(|e| e.to_string())
            });
            match written {
                Ok(path) => {
                    taken.insert(path);
                }
                Err(e) => crate::build::cli_output::log_warn_problem!(
                    "[{plugin}] the file `{}` from the `{style}` listing of {name} was not written: {e}",
                    file.path
                ),
            }
        }
        let content_width = result.content_width.clone().or_else(|| owner.style.content_width.clone());
        slots.set_listing(&page.url_path, ListingFill { html: result.html, content_width });
    }
    for (plugin, (first, fell_back)) in failed {
        let pages = if fell_back == 1 { "its page".to_string() } else { format!("all {fell_back} of its pages") };
        crate::build::cli_output::log_warn_problem!("[{plugin}] {first}, so moss's own listing is drawn for {pages}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segs(path: &str) -> Vec<String> {
        path.split('/').filter(|s| !s.is_empty()).map(str::to_string).collect()
    }

    #[test]
    fn a_body_is_rebased_from_its_page_to_the_listing() {
        let html = r##"<img src="../pic.jpg"> <a href="../../beta/#x">b</a> <a href="/abs/">a</a> <a href="https://e.com/">e</a> <a href="#top">t</a>"##;
        let out = rebase_html(html, "board/sub/alpha/index.html", &segs("board"));
        assert!(out.contains(r#"src="sub/pic.jpg""#), "{out}");
        assert!(out.contains(r#"href="beta/#x""#), "{out}");
        assert!(out.contains(r#"href="/abs/""#) && out.contains(r#"href="https://e.com/""#) && out.contains(r##"href="#top""##), "{out}");
    }

    #[test]
    fn urls_are_relative_to_the_listing_page() {
        assert_eq!(relative(&segs("board"), &segs("board/sub")), "sub");
        assert_eq!(relative(&segs("board"), &segs("other/x")), "../other/x");
        assert_eq!(relative(&segs(""), &segs("board")), "board");
    }

    #[test]
    fn a_span_is_filled_or_unwrapped() {
        let page = format!("<body><main>{BEGIN}board -->DEFAULT{END}</main></body>");
        let fill = Some(ListingFill { html: "PLUGIN".into(), content_width: Some("full".into()) });
        let filled = settle_spans(&page, fill.as_ref());
        assert!(filled.contains("<main>PLUGIN</main>") && !filled.contains("moss:listing"), "{filled}");
        assert!(filled.contains(r#"<body data-content-width="full">"#), "{filled}");
        let kept = settle_spans(&page, None);
        assert_eq!(kept, "<body><main>DEFAULT</main></body>");
    }

    #[test]
    fn files_that_leave_the_folder_or_collide_are_refused() {
        let taken: HashSet<String> = ["board/index.html".to_string()].into();
        assert!(refusal("a.css", "board", |p| taken.contains(p)).is_none());
        assert!(refusal("/a.css", "board", |p| taken.contains(p)).is_some());
        assert!(refusal("../a.css", "board", |p| taken.contains(p)).is_some());
        assert!(refusal("_moss/a.css", "board", |p| taken.contains(p)).is_some());
        assert!(refusal("index.html", "board", |p| taken.contains(p)).is_some());
        assert!(refusal("_moss/a.css", "", |p| taken.contains(p)).is_some());
    }
}
