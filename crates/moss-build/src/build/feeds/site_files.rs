//! The site's derived files that no page on screen reads: the feeds,
//! `llms.txt` and `sitemap.xml`, and the copies a redirect keeps of them at a
//! former address. The render declares them and promises them to the
//! manifest; a background worker writes them after the preview paints. Both
//! read the one declaration, so a file the worker stops writing leaves a
//! promise unkept, and the seal withholds that generation instead of
//! publishing the site without it.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::mpsc::Sender;

use crate::build::background::BuildError;
use crate::build::coordinator::EmitMessage;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::ServedPath;
use crate::build::types::ParsedDocument;
use crate::system::folder_session::FolderSession;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Rss,
    JsonFeed,
    Llms,
    Sitemap,
}

impl Kind {
    fn path(self) -> ServedPath {
        match self {
            Kind::Rss => ServedPath::for_rss("").expect("the root feed path is valid"),
            Kind::JsonFeed => ServedPath::for_json_feed(),
            Kind::Llms => ServedPath::for_llms_txt(),
            Kind::Sitemap => ServedPath::for_sitemap(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SiteFiles {
    kinds: Vec<Kind>,
    /// `(former address, the declared file it copies)`.
    copies: Vec<(ServedPath, String)>,
    site_url: String,
    site_lang: crate::i18n::Language,
    dir_overrides: HashMap<String, String>,
    sitemap_pages: BTreeSet<String>,
    /// The site folder: a file it provides at one of these addresses is
    /// copied in as it is, and none is generated.
    folder: PathBuf,
    /// Variants this build will never make, each mapped to its original: a
    /// feed's `<img>` naming one names the original instead.
    never_made: HashMap<String, String>,
}

impl SiteFiles {
    pub(crate) fn new(
        folder: &Path,
        site_url: &str,
        site_lang: crate::i18n::Language,
        dir_overrides: &HashMap<String, String>,
    ) -> Self {
        Self {
            kinds: Vec::new(),
            copies: Vec::new(),
            site_url: site_url.to_string(),
            site_lang,
            dir_overrides: dir_overrides.clone(),
            sitemap_pages: BTreeSet::new(),
            folder: folder.to_path_buf(),
            never_made: HashMap::new(),
        }
    }

    /// See [`Self::never_made`]; read from the promise, before any encode.
    pub(crate) fn never_made(&mut self, originals: HashMap<String, String>) {
        self.never_made = originals;
    }

    fn declare(&mut self, kinds: &[Kind]) {
        let provided = |k: &Kind| crate::build::scan::path_collision::on_disk(&self.folder, k.path().as_str()).is_file();
        let generated: Vec<Kind> = kinds.iter().copied().filter(|k| !provided(k)).collect();
        self.kinds.extend(generated);
    }

    /// `rss.xml`, and `feed.json` with the same articles plus the fields RSS
    /// has no element for.
    pub(crate) fn feeds(&mut self) {
        self.declare(&[Kind::Rss, Kind::JsonFeed]);
    }

    /// `llms.txt`: an index of the listed pages.
    pub(crate) fn llms(&mut self) {
        self.declare(&[Kind::Llms]);
    }

    /// Declared before the redirect table, which keeps copies only of what is
    /// declared; its pages are known only after the table ([`Self::list_in_sitemap`]).
    pub(crate) fn sitemap(&mut self) {
        self.declare(&[Kind::Sitemap]);
    }

    pub(crate) fn list_in_sitemap(&mut self, pages: BTreeSet<String>) {
        self.sitemap_pages = pages;
    }

    /// The addresses of the declared files, for the redirect table, which
    /// counts them as served before they are written.
    pub(crate) fn addresses(&self) -> impl Iterator<Item = String> + '_ {
        self.kinds.iter().map(|k| k.path().as_str().to_string())
    }

    /// Keep `to`'s bytes at `from` too, when `to` is one of the declared files.
    pub(crate) fn keep_copy(&mut self, from: ServedPath, to: &str) {
        if self.kinds.iter().any(|k| k.path().as_str() == to) {
            self.copies.push((from, to.to_string()));
        }
    }

    /// Every file the worker writes, which the render promises to the manifest.
    pub(crate) fn promise(&self, pending: &mut PendingManifest) {
        for path in self.kinds.iter().map(|k| k.path()).chain(self.copies.iter().map(|(from, _)| from.clone())) {
            pending.promise(&path);
        }
    }

    /// The bytes of every declared file and copy, in memory.
    fn render(&self, documents: &[ParsedDocument]) -> Vec<(ServedPath, Vec<u8>)> {
        let home = documents.iter().find(|d| d.url_path == "index.html");
        let site_title = home.map(|d| d.title.as_str()).unwrap_or(crate::i18n::t(self.site_lang, "site"));
        let url = self.site_url.as_str();
        let items = self.kinds.iter().any(|k| matches!(k, Kind::Rss | Kind::JsonFeed)).then(|| crate::build::feeds::rss::feed_items(documents, url, &self.never_made));
        let mut out: Vec<(ServedPath, Vec<u8>)> = Vec::new();
        for &kind in &self.kinds {
            let bytes = match kind {
                Kind::Rss => crate::build::feeds::rss::rss_feed(
                    items.as_deref().unwrap_or_default(),
                    site_title,
                    url,
                    None,
                    home.and_then(|d| d.analytics.as_ref()),
                ),
                Kind::JsonFeed => crate::build::feeds::json_feed::json_feed(
                    items.as_deref().unwrap_or_default(),
                    site_title,
                    url,
                    self.site_lang.as_bcp47_attr(),
                    &self.dir_overrides,
                ),
                Kind::Llms => llms_txt(documents, site_title, url),
                Kind::Sitemap => crate::build::feeds::sitemap::generate_sitemap(
                    &crate::build::feeds::sitemap::entries_for_pages(&self.sitemap_pages, documents),
                    url,
                ),
            };
            out.push((kind.path(), bytes.into_bytes()));
        }
        for (from, to) in &self.copies {
            if let Some(bytes) = out.iter().find(|(path, _)| path.as_str() == to).map(|(_, b)| b.clone()) {
                out.push((from.clone(), bytes));
            }
        }
        out
    }

    /// Write every file now, for a caller with no background phase.
    pub(crate) fn write_now(
        &self,
        documents: &[ParsedDocument],
        output_dir: &Path,
        pending: &mut PendingManifest,
    ) -> Result<(), String> {
        for (path, bytes) in self.render(documents) {
            crate::build::context::BuildContext::for_render(output_dir, pending)
                .emit_held(&path, bytes, HashBucket::Files)
                .map_err(|e| format!("Failed to emit {}: {}", path.as_str(), e))?;
        }
        Ok(())
    }

    /// What the worker needs, taken at dispatch; `None` when nothing is declared.
    pub(crate) fn after_paint(
        self,
        documents: &Arc<Vec<ParsedDocument>>,
        output: &Path,
        session: Option<Arc<FolderSession>>,
    ) -> Option<AfterPaint> {
        (!self.kinds.is_empty()).then(|| AfterPaint {
            files: self,
            documents: documents.clone(),
            output: output.to_path_buf(),
            // Held from dispatch, so a publish waits for this worker even
            // before the blocking pool starts it.
            held: session.as_ref().map(|s| s.hold_ui_bound()),
            session,
        })
    }
}

/// The listed pages, with the home page's one-sentence description quoted in
/// a header line, reduced like the description tags so `_..._` does not ship
/// its markers.
fn llms_txt(documents: &[ParsedDocument], site_title: &str, site_url: &str) -> String {
    let homepage_desc: Option<String> = documents
        .iter()
        .find(|d| d.url_path == "index.html")
        .and_then(|d| d.description.as_deref())
        .map(crate::build::page::meta::strip_markdown_inline)
        .filter(|s| !s.trim().is_empty());
    crate::build::feeds::llms_txt::generate_llms_txt(documents, site_title, homepage_desc.as_deref(), site_url)
}

pub(crate) struct AfterPaint {
    files: SiteFiles,
    documents: Arc<Vec<ParsedDocument>>,
    output: PathBuf,
    session: Option<Arc<FolderSession>>,
    held: Option<crate::system::folder_session::UiBoundHold>,
}

impl AfterPaint {
    pub(crate) fn spawn(
        self,
        workers: &mut tokio::task::JoinSet<Result<(), BuildError>>,
        tx: Sender<EmitMessage>,
        render: crate::build::lifecycle::Render,
    ) {
        crate::build::background::spawn_traced_worker(workers, move || self.run(render, tx));
    }

    /// Make the bytes outside any lock, then, holding the stage write lock,
    /// write them only if no newer render has been admitted. A newer render's
    /// own worker writes these files; this one tells the coordinator, so its
    /// render is not promoted without them.
    fn run(self, render: crate::build::lifecycle::Render, tx: Sender<EmitMessage>) -> Result<(), BuildError> {
        let _phase = crate::build::phase::PhaseTrace::start("background_site_files");
        let session = self.session.as_ref();
        let files = self.files.render(&self.documents);
        let _stage_write = session.map(|s| s.blocking_lock_stage_write());
        if session.is_some_and(|s| s.cancel.is_cancelled()) || !render.is_latest() {
            return tx
                .blocking_send(EmitMessage::Superseded)
                .map_err(|_| BuildError::Worker("site files coordinator closed".into()));
        }
        for (path, bytes) in files {
            crate::build::io_utils::write_output_if_changed(&path.to_disk(&self.output), &bytes)
                .map_err(|e| BuildError::Worker(format!("Failed to write {}: {e}", path.as_str())))?;
            tx.blocking_send(EmitMessage::Held { path, bytes, bucket: HashBucket::Files })
                .map_err(|_| BuildError::Worker("site files coordinator closed".into()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "site_files_tests.rs"]
mod tests;
