//! `[site] archival = true`: a site that names its own build.
//!
//! Three things happen when the option is on. Every page carries an edition
//! line (build date and a short id), `dcterms.modified` / `moss:edition` meta
//! tags, a plain-HTML citation block and the site's `license`. The build also publishes the site's
//! source, Markdown pages and data files in their own folders, under
//! `_moss/source/` with a README that says what the files are.
//!
//! The id is a hash of the source copy, not of the finished output: pages carry
//! the id, so an output hash would have to contain itself. The same source is
//! the same edition on any day; a changed file is a new one.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::build::assets::paths::compute_binary_hash;
use crate::build::context::BuildContext;
use crate::build::features::html_escape;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::{served_address, ServedPath};
use crate::build::site_url::SiteUrl;
use crate::build::types::ParsedDocument;
use crate::i18n::{t, Language};
use crate::types::content::ProjectStructure;

/// Extensions copied into the source folder: the pages and the data they use.
const SOURCE_EXTENSIONS: &[&str] = &["md", "markdown", "csv", "tsv", "json", "yaml", "yml", "bib"];

/// Where each page's edition block points.
const SOURCE_README: &str = "_moss/source/README.md";

const FOOTER_CLOSE: &str = "</footer><!-- /moss:footer -->";
const CLOSING_PICTURE: &str = "<section class=\"moss-hero moss-hero-closing\"";
const SECTION_OPEN: &str = "<section";
const SECTION_CLOSE: &str = "</section>";

/// What every page of an edition-marked build says about its build.
#[derive(Debug, Clone, Serialize)]
pub struct Edition {
    date: String,
    id: String,
    site_url: Option<String>,
    license: Option<String>,
    style_link: String,
    site_title: String,
    /// Each page the build rendered, by output path.
    pages: BTreeMap<String, CitedPage>,
}

/// What one page's citation names, escaped for HTML.
#[derive(Debug, Clone, Serialize)]
struct CitedPage {
    title: String,
    lang: Language,
    /// The page's own authors, else the site's.
    author: Option<String>,
}

impl CitedPage {
    fn of(doc: &ParsedDocument, site_author: Option<&str>) -> Self {
        let own = (!doc.author.is_empty()).then(|| doc.author.join(t(doc.lang, "name_separator")));
        Self { title: html_escape(&doc.title), lang: doc.lang, author: own.as_deref().or(site_author).map(html_escape) }
    }
}

/// The markup one page receives.
pub(crate) struct Rendered {
    pub head: String,
    pub block: String,
}

/// The date an edition carries: `SOURCE_DATE_EPOCH` when set (the
/// reproducible-builds convention, and how a test pins it), otherwise the
/// build's own local date.
pub fn build_date(source_date_epoch: Option<&str>, now: chrono::NaiveDateTime) -> String {
    let date = source_date_epoch
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0))
        .map_or(now.date(), |moment| moment.date_naive());
    date.format("%Y-%m-%d").to_string()
}

/// The files the source copy holds, relative to the site root, sorted. A map
/// layer's source folder is out of the scanned structure but is source a
/// rebuilt folder needs, so all its files join the copy here, scans included:
/// the published site carries a scan only as warped tiles.
fn source_files(structure: &ProjectStructure) -> Vec<String> {
    let root = Path::new(&structure.root_path);
    let layer_files = crate::config::map_layers::declared_sources(root).into_iter().flat_map(|source| {
        walkdir::WalkDir::new(root.join(source))
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| e.path().strip_prefix(root).ok().map(|r| moss_core::slug::normalize_separators(&r.to_string_lossy())))
            .collect::<Vec<_>>()
    });
    let mut files: Vec<String> = structure
        .markdown_files
        .iter()
        .chain(&structure.other_files)
        .filter(|file| SOURCE_EXTENSIONS.contains(&file.file_type.as_str()))
        .map(|file| file.path.clone())
        .chain(layer_files)
        .filter(|path| !path.split('/').any(|segment| segment == "_moss" || segment == "node_modules"))
        .collect();
    files.sort_unstable();
    files.dedup();
    files
}

/// What the pipeline knows about the site when it builds the edition.
pub struct SiteFacts<'a> {
    pub structure: &'a ProjectStructure,
    pub documents: &'a [ParsedDocument],
    pub site_url: Option<String>,
    pub author: Option<String>,
    pub license: Option<String>,
    pub lang: &'a str,
    pub folder_name: Option<&'a str>,
    pub now: chrono::NaiveDateTime,
}

/// What the build knows about the site, for the README and the citation.
struct EditionInputs<'a> {
    structure: &'a ProjectStructure,
    documents: &'a [ParsedDocument],
    site_url: Option<&'a SiteUrl>,
    author: Option<String>,
    license: Option<String>,
    date: String,
    site_title: String,
    style_link: String,
}

/// The edition of an `[site] archival` build, or `None` when the option is off.
/// Writes the source copy and its README into the stage first, because the id
/// is a hash of the copy. `style_link` is called only when the edition is on.
pub fn from_build(
    enabled: bool,
    site: SiteFacts<'_>,
    stage_dir: &Path,
    pending: &mut PendingManifest,
    style_link: impl FnOnce() -> String,
) -> std::io::Result<Option<Edition>> {
    if !enabled {
        return Ok(None);
    }
    let site_url = site.site_url.as_deref().and_then(|url| SiteUrl::parse(url).ok());
    let homepage_title = site.documents.iter().find(|d| d.url_path == "index.html").map(|d| d.title.as_str());
    let site_title = moss_core::home::site_name(
        site.structure.homepage_file.as_deref(),
        homepage_title,
        site.folder_name.unwrap_or_else(|| t(Language::from_bcp47_lenient(site.lang), "site")),
    );
    let inputs = EditionInputs {
        structure: site.structure,
        documents: site.documents,
        site_url: site_url.as_ref(),
        author: site.author,
        license: site.license,
        date: build_date(std::env::var("SOURCE_DATE_EPOCH").ok().as_deref(), site.now),
        site_title,
        style_link: style_link(),
    };
    emit_source_copy(inputs, stage_dir, pending).map(Some)
}

/// Write the source copy and its README into the stage and return the edition
/// every page will carry.
fn emit_source_copy(inputs: EditionInputs<'_>, stage_dir: &Path, pending: &mut PendingManifest) -> std::io::Result<Edition> {
    let root = Path::new(&inputs.structure.root_path);
    let mut inventory = String::new();
    let mut copied = Vec::new();
    for rel in source_files(inputs.structure) {
        // A file the sync client has not materialised yet is left out of this
        // edition rather than failing the build.
        let Ok(bytes) = std::fs::read(root.join(&rel)) else { continue };
        let Ok(served) = ServedPath::for_source_copy(&rel) else { continue };
        BuildContext::for_render(stage_dir, pending).emit(&served, &bytes, HashBucket::Files)?;
        inventory.push_str(&format!("{rel}\0{}\n", compute_binary_hash(&bytes)));
        copied.push(rel);
    }
    let id: String = compute_binary_hash(inventory.as_bytes()).chars().take(7).collect();
    let site_url = inputs.site_url.filter(|url| url.is_deployed()).map(|url| url.as_str().to_string());
    if site_url.is_none() {
        crate::build::cli_output::log_note!("no published site address is known ([site] domain), so the page citations name no address");
    }
    let readme = readme(Path::new(&inputs.structure.root_path), &copied, inputs.documents, inputs.license.as_deref(), &inputs.date, &id);
    let served = ServedPath::from_cached(SOURCE_README).map_err(std::io::Error::other)?;
    BuildContext::for_render(stage_dir, pending).emit(&served, readme.as_bytes(), HashBucket::Files)?;
    Ok(Edition {
        date: inputs.date,
        id,
        site_url,
        license: inputs.license.as_deref().map(html_escape),
        style_link: inputs.style_link,
        site_title: html_escape(&inputs.site_title),
        pages: inputs.documents.iter().map(|doc| (doc.url_path.clone(), CitedPage::of(doc, inputs.author.as_deref()))).collect(),
    })
}

fn readme(root: &Path, files: &[String], documents: &[ParsedDocument], license: Option<&str>, date: &str, id: &str) -> String {
    let mut fields: BTreeMap<&str, usize> = BTreeMap::new();
    for document in documents {
        for key in document.raw_frontmatter.keys() {
            *fields.entry(key.as_str()).or_default() += 1;
        }
    }
    let mut out = format!(
        "# Source files\n\nThe source of this site as of edition {date} ({id}): the Markdown pages and data files it is built from, in their original folders. They are plain text. Copy the folder anywhere and a Markdown editor, or moss, reads it as it is.\n\n## Files\n\n"
    );
    for file in files {
        out.push_str(&format!("- `{file}`\n"));
    }
    if !fields.is_empty() {
        out.push_str("\n## Front matter fields\n\nA page starts with a block between two `---` lines. The fields this site uses, with the number of pages that set each:\n\n");
        for (field, count) in &fields {
            out.push_str(&format!("- `{field}` ({count})\n"));
        }
    }
    out.push_str(&layer_sections(root));
    if let Some(license) = license {
        out.push_str(&format!("\n## Licence\n\n{license}\n"));
    }
    out
}

/// One section per map layer folder: what its three kinds of file are, and
/// where each plate came from, read from the plate's `region.json`.
fn layer_sections(root: &Path) -> String {
    let mut out = String::new();
    for source in crate::config::map_layers::declared_sources(root) {
        out.push_str(&format!(
            "\n## Map layer `{source}`\n\nA historical map the site draws under its places. For each plate, `<id>.annotation.json` is its georeference (control points tying the scan to places on the earth), the image is the part of the scan the site uses, and `<id>.region.json` says which part, at what scale, where it came from and under what rights.\n\n"
        ));
        let mut regions: Vec<_> = walkdir::WalkDir::new(root.join(&source))
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file() && e.file_name().to_string_lossy().ends_with(".region.json"))
            .map(|e| e.into_path())
            .collect();
        regions.sort();
        for path in regions {
            let id = path.file_name().map(|n| n.to_string_lossy().trim_end_matches(".region.json").to_string()).unwrap_or_default();
            let region: serde_json::Value = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
            let field = |k: &str| region.get(k).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(str::to_string);
            let mut line = format!("- `{id}`: from {}", field("source").unwrap_or_else(|| "a source not stated".into()));
            if let Some(credit) = field("credit") {
                line.push_str(&format!("; {credit}"));
            }
            if let Some(rights) = field("rights") {
                line.push_str(&format!("; rights: {rights}"));
            }
            if let Some(by) = field("georeferenced_by") {
                line.push_str(&format!("; georeferenced by {by}"));
            }
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

impl Edition {
    /// The markup for one stage page, or `None` for a page the build did not
    /// render from a document (a redirect stub, an author's own HTML file) or
    /// that has no moss footer.
    pub(crate) fn render(&self, html: &str, page_path: &str) -> Option<Rendered> {
        if !html.contains(FOOTER_CLOSE) || !html.contains("</head>") {
            return None;
        }
        let page = self.pages.get(page_path)?;
        let (title, lang, site) = (page.title.as_str(), page.lang, self.site_title.as_str());
        let word = t(lang, "edition");
        let (date, id) = (&self.date, &self.id);
        let mut cite = String::new();
        if let Some(author) = &page.author {
            cite.push_str(&format!("{author}. "));
        }
        cite.push_str(&format!("\u{201c}{title}.\u{201d} "));
        if !site.is_empty() && site != title {
            cite.push_str(&format!("{site}. "));
        }
        cite.push_str(&format!("{word} {date} ({id})."));
        // A site-relative path is no address outside the site, so an undeployed build cites none.
        if let Some(base) = &self.site_url {
            cite.push_str(&format!(" {}", html_escape(&format!("{base}{}", served_address(page_path)))));
        }
        let head = format!(
            "{}\n<meta name=\"dcterms.modified\" content=\"{date}\">\n<meta name=\"moss:edition\" content=\"{date} {id}\">\n",
            self.style_link
        );
        let rights = self.license.as_ref().map(|license| format!("<p class=\"moss-edition-rights\">{license}</p>\n")).unwrap_or_default();
        let block = format!(
            "<div class=\"moss-edition\">\n<p class=\"moss-edition-line\">{word} {date} · {id} · <a href=\"/{SOURCE_README}\">{}</a></p>\n<div class=\"moss-cite\">\n<p class=\"moss-cite-label\">{}</p>\n<p class=\"moss-cite-text\">{cite}</p>\n</div>\n{rights}</div>",
            t(lang, "source_files"),
            t(lang, "cite_this_page"),
        );
        Some(Rendered { head, block })
    }

    /// The page's HTML with its edition markup placed once, after the slots have
    /// filled: the head tags, then the block at the end of a closing picture when
    /// the page closes on one, otherwise at the end of the moss footer, after the
    /// footer-end slot's content. `None` for a page the edition does not cover.
    pub(crate) fn place(&self, html: &str, page_path: &str) -> Option<String> {
        let rendered = self.render(html, page_path)?;
        let mut out = html.replacen("</head>", &format!("{}</head>", rendered.head), 1);
        match closing_picture_end(&out) {
            Some(close) => out.insert_str(close, &format!("{}\n", rendered.block)),
            None => out.insert_str(out.find(FOOTER_CLOSE)?, &rendered.block),
        }
        Some(out)
    }
}

/// The offset of the `</section>` that closes the page's closing picture, found
/// by counting nested sections so a `<section>` inside the picture does not end it early.
fn closing_picture_end(html: &str) -> Option<usize> {
    let start = html.find(CLOSING_PICTURE)?;
    let (mut depth, mut at) = (0usize, start);
    loop {
        let close = at + html[at..].find(SECTION_CLOSE)?;
        match html[at..close].find(SECTION_OPEN).map(|open| at + open) {
            Some(open) => {
                depth += 1;
                at = open + SECTION_OPEN.len();
            }
            None => {
                depth -= 1;
                if depth == 0 {
                    return Some(close);
                }
                at = close + SECTION_CLOSE.len();
            }
        }
    }
}

#[cfg(test)]
#[path = "archival_tests.rs"]
mod tests;
