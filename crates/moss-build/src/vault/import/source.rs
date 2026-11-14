//! The host-neutral URL-or-file import entry, shared by CLI and native hosts.
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use specta::Type;
use super::scrape::{run::{ScrapeProgress, ScrapeResult}, service::ScrapeConfig};

#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    pub recursive: bool,
    /// A persistent private directory outside the project. Native hosts can
    /// supply their own app-data directory; the CLI uses the app-data default.
    pub private_members_dir: Option<PathBuf>,
    /// A WordPress export added to an address: read after the address is crawled.
    pub export: Option<PathBuf>,
}

/// One import run into one folder, across the sources given to it: the
/// folder's imported pages are read once and kept current as pages land.
#[derive(Default)]
pub struct ImportSession {
    held: Option<(PathBuf, super::items::ImportedPages)>,
}

impl ImportSession {
    fn pages(&mut self, folder: &Path) -> &mut super::items::ImportedPages {
        if self.held.as_ref().is_none_or(|(f, _)| f != folder) {
            self.held = Some((folder.to_path_buf(), super::items::ImportedPages::scan(folder)));
        }
        &mut self.held.as_mut().expect("just set").1
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SubscriberHandoff {
    /// Retained outside the project until list import consumes or discards it.
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ImportResult {
    pub scrape: ScrapeResult,
    pub report: Vec<String>,
    pub subscribers: Option<SubscriberHandoff>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind { Ghost, Wordpress, Site, File }

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RecognisedSource {
    pub kind: SourceKind,
    pub host: Option<String>,
    pub title: Option<String>,
}

/// Inspect the origin homepage for URL sources, or export contents for local sources.
/// A subpage need not repeat its platform's generator metadata.
pub async fn recognise_source(source: &str) -> Result<RecognisedSource, String> {
    let source = source.trim();
    if is_remote_url(source) {
        super::scrape::fetch::refuse_unsafe_scrape_url(source)?;
        let mut home = url::Url::parse(source).map_err(|e| e.to_string())?;
        home.set_path("/");
        home.set_query(None);
        home.set_fragment(None);
        let (html, _, final_url) = super::scrape::fetch::fetch_page(home.as_str(), &super::scrape::service::default_user_agent(), None)
            .await.map_err(|e| e.to_string())?;
        let doc = scraper::Html::parse_document(&html);
        let generator = scraper::Selector::parse("meta[name=generator]").unwrap();
        let generator = doc.select(&generator).filter_map(|n| n.value().attr("content")).collect::<Vec<_>>().join(" ").to_lowercase();
        let kind = if generator.contains("ghost") { SourceKind::Ghost } else if generator.contains("wordpress") { SourceKind::Wordpress } else { SourceKind::Site };
        let title = scraper::Selector::parse("title").unwrap();
        return Ok(RecognisedSource { kind, host: url::Url::parse(&final_url).ok().and_then(|u| u.host_str().map(str::to_string)), title: doc.select(&title).next().map(|n| n.text().collect()) });
    }
    let path = local_path(source)?;
    if let Some(head) = super::wordpress::wxr::sniff(&path) {
        return Ok(RecognisedSource { kind: SourceKind::Wordpress, host: head.site.and_then(|u| url::Url::parse(&u).ok()).and_then(|u| u.host_str().map(str::to_string)), title: head.title });
    }
    if let Some(bundle) = super::ghost::bundle::Bundle::open(&path)? {
        return Ok(RecognisedSource { kind: SourceKind::Ghost, host: bundle.site_url().and_then(|u| url::Url::parse(&u).ok()).and_then(|u| u.host_str().map(str::to_string)), title: bundle.setting("title") });
    }
    Ok(RecognisedSource { kind: SourceKind::File, host: None, title: path.file_name().map(|s| s.to_string_lossy().into_owned()) })
}

pub async fn import_source<F>(source: &str, folder: &Path, options: ImportOptions, on_progress: F) -> Result<ImportResult, String>
where F: Fn(ScrapeProgress) + Send + Sync + 'static {
    import_source_in(&mut ImportSession::default(), source, folder, options, &on_progress).await
}

/// [`import_source`] as one of several sources imported into one folder.
pub async fn import_source_in(
    session: &mut ImportSession, source: &str, folder: &Path, options: ImportOptions,
    on_progress: &(dyn Fn(ScrapeProgress) + Send + Sync),
) -> Result<ImportResult, String> {
    let source = source.trim();
    if !folder.is_dir() { return Err("Output directory does not exist".into()); }
    if is_remote_url(source) {
        super::scrape::fetch::refuse_unsafe_scrape_url(source)?;
        if let Some(export) = &options.export {
            if super::wordpress::wxr::sniff(export).is_none() { return Err("--export takes a WordPress export file (Tools → Export).".into()); }
            return super::wordpress::import(export, Some(source), folder, session.pages(folder), on_progress).await;
        }
        let mut config = ScrapeConfig::new(source, folder);
        config.recursive = options.recursive;
        let scrape = super::scrape::run::scrape_into(config, session.pages(folder), on_progress).await?.result;
        return Ok(ImportResult { report: if scrape.restricted_pages > 0 { vec![format!("{} posts are for subscribers only; add a Ghost export to bring their text.", scrape.restricted_pages)] } else { Vec::new() }, subscribers: None, scrape });
    }
    let path = local_path(source)?;
    if super::wordpress::wxr::sniff(&path).is_some() {
        return super::wordpress::import(&path, None, folder, session.pages(folder), on_progress).await;
    }
    if let Some(bundle) = super::ghost::bundle::Bundle::open(&path)? {
        return super::ghost::import(bundle, folder, options, session.pages(folder), on_progress).await;
    }
    let (scrape, report) = if super::douban::is_tofu_backup(&path) {
        let (scrape, report) = super::douban::import_tofu_backup(&path, folder).await?;
        (scrape, report.lines())
    } else { (super::scrape::import_local::import_local_file(&path, folder).await?, Vec::new()) };
    let mut report = report;
    if scrape.restricted_pages > 0 { report.push("This saved page contains a Ghost lock box; add the export to bring its text.".into()); }
    Ok(ImportResult { scrape, report, subscribers: None })
}

pub(crate) fn is_remote_url(source: &str) -> bool { let source = source.trim().to_ascii_lowercase(); source.starts_with("https://") || source.starts_with("http://") }
fn local_path(source: &str) -> Result<PathBuf, String> {
    if source.starts_with("file://") {
        return url::Url::parse(source).map_err(|e| e.to_string())?.to_file_path().map_err(|_| "Invalid local file URL".into());
    }
    Ok(PathBuf::from(source))
}
