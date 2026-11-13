//! Media placement shared by live HTML, local captures and backup readers.

use std::collections::HashMap;
use std::path::Path;
use super::{ImportedPage, PageStyle};
use crate::vault::import::scrape::{
    crawl_state::{AssetMap, HostPacer}, crawler::host_of,
    fetch::{content_type_to_extension, download_asset, download_linked_file, hash_url, linked_filename, observe_pace, Downloaded, FetchError, FileProgress},
    mhtml::MhtmlResource, writer::rename_for_collision,
};

#[derive(Default)]
pub(crate) struct PageReport {
    pub pages_written: usize,
    pub media_downloaded: usize,
    pub media_failed: usize,
}

pub(crate) struct MediaWriter<'a> {
    pub directory: &'a Path,
    /// Prefix of saved filenames as seen from the page (or the crawl spool).
    pub prefix: &'a str,
    pub user_agent: &'a str,
    pub referer: Option<&'a str>,
}

impl MediaWriter<'_> {
    /// The map's lifetime defines deduplication: across a crawl, or within a
    /// page bundle. Failed URLs stay remote and are not fetched twice.
    pub async fn download(
        &self,
        images: &[(String, String)],
        assets: &mut AssetMap,
        mut pacing: Option<(&mut HostPacer, &mut FileProgress)>,
    ) -> PageReport {
        let mut report = PageReport::default();
        for (raw, url) in images {
            if assets.is_settled(raw) { continue; }
            let host = host_of(url);
            if let Some((pacer, _)) = pacing.as_mut() { pacer.wait(&host).await; }
            let result = download_asset(url, self.directory, self.user_agent, self.referer).await;
            if let Some((pacer, progress)) = pacing.as_mut() {
                observe_pace(pacer, &host, &result);
                progress.finished_one(pacer.current_interval(&host));
            }
            match result {
                Ok(file) => {
                    assets.insert(raw.clone(), format!("{}{file}", self.prefix));
                    report.media_downloaded += 1;
                }
                Err(e) => {
                    assets.keep_remote(raw.clone());
                    report.media_failed += 1;
                    log::warn!("import: asset download failed, keeping remote URL: {url}: {e}");
                }
            }
        }
        report
    }

    /// Download a file a page links to (a document, a score, a video) under
    /// its own name, paced, and settle it in `assets`: its local path when it
    /// landed, else its remote address. A file over `max_bytes` is not written.
    pub async fn linked(&self, url: &str, assets: &mut AssetMap, pacer: &mut HostPacer, max_bytes: u64) -> Result<Downloaded, FetchError> {
        let host = host_of(url);
        pacer.wait(&host).await;
        let filename = assets.claim_name(linked_filename(url), url);
        let result = download_linked_file(url, self.directory, self.user_agent, self.referer, max_bytes, filename).await;
        observe_pace(pacer, &host, &result);
        match &result {
            Ok(Downloaded::Saved(file)) => assets.insert(url.to_string(), format!("{}{file}", self.prefix)),
            _ => assets.keep_remote(url.to_string()),
        }
        result
    }

    /// Place bytes already held by a local reader through the same hash naming
    /// policy as captured resources. The extension is data, never a path.
    pub fn bytes(&self, key: &str, extension: &str, bytes: &[u8]) -> Result<String, String> {
        if extension.is_empty() || !extension.chars().all(|c| c.is_ascii_alphanumeric()) { return Err("Invalid imported media extension".into()); }
        std::fs::create_dir_all(self.directory).map_err(|e| e.to_string())?;
        let file = format!("{}.{}", hash_url(key), extension);
        // allow:raw_write imported source media, never generated build output
        std::fs::write(self.directory.join(&file), bytes).map_err(|e| format!("Failed to write asset: {e}"))?;
        Ok(format!("{}{file}", self.prefix))
    }

    /// Local captures never fetch: only the resources present in the archive
    /// become local paths. The content-type hash naming remains unchanged.
    pub fn embedded(
        &self,
        urls: &std::collections::HashSet<String>,
        resources: &HashMap<String, MhtmlResource>,
    ) -> Result<HashMap<String, String>, String> {
        std::fs::create_dir_all(self.directory)
            .map_err(|e| format!("Failed to create assets directory: {e}"))?;
        let mut local = HashMap::new();
        for url in urls {
            if local.contains_key(url) { continue; }
            if let Some(resource) = resources.get(url) {
                let path = self.bytes(url, content_type_to_extension(&resource.content_type), &resource.bytes)?;
                local.insert(url.clone(), path);
            }
        }
        Ok(local)
    }
}

/// Backups keep media beside their page. Reserve a fresh directory before
/// downloading, so two source rows with one id never share or replace assets.
pub(crate) async fn write_bundle_page(
    page: &mut ImportedPage,
    root: &Path,
    folder: &str,
    slug: &str,
    user_agent: &str,
) -> Result<PageReport, String> {
    let images = page.images();
    let bundle = !images.is_empty();
    let mut dir = if bundle { format!("{folder}/{slug}") } else { folder.to_string() };
    if bundle {
        let base = dir.clone();
        let mut n = 2;
        while root.join(&dir).exists() {
            dir = format!("{base} {n}");
            n += 1;
        }
        std::fs::create_dir_all(root.join(&dir))
            .map_err(|e| format!("Failed to create directory: {e}"))?;
    }
    let mut assets = AssetMap::default();
    let mut report = MediaWriter {
        directory: &root.join(&dir), prefix: "./", user_agent,
        referer: page.frontmatter.origin.as_deref(),
    }.download(&images, &mut assets, None).await;
    page.localize(&assets.local_map());
    let relative = if bundle { format!("{dir}/index.md") } else { format!("{folder}/{slug}.md") };
    let relative = rename_for_collision(root, &relative);
    page.write(root, &relative, PageStyle::Backup)?;
    report.pages_written = 1;
    Ok(report)
}
