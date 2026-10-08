//! Output-side proof for leaving a verdict-approved page on disk.

use std::path::Path;

use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::media::qr;
use crate::build::render::incremental::RenderVerdict;
use crate::build::served_path::ServedPath;
use crate::build::site_url::SiteUrl;
use crate::build::types::ParsedDocument;
use crate::types::content::SiteHashes;

#[derive(Debug, Clone)]
struct CarriedEntry {
    path: ServedPath,
    manifest_value: String,
}

/// Entries proved to exist unchanged by both the render verdict and last
/// build's sealed manifest/output tree.
#[derive(Debug, Clone)]
pub struct CarryProof {
    html: CarriedEntry,
    qr: Option<CarriedEntry>,
}

impl CarryProof {
    /// Prove a document's outputs can be left in place. A missing HTML or
    /// expected QR output falls back to rendering, which repairs the stage.
    pub fn for_document(
        doc: &ParsedDocument,
        verdict: &RenderVerdict,
        previous: &SiteHashes,
        output_dir: &Path,
        site_url: &SiteUrl,
    ) -> Option<Self> {
        if !verdict.may_skip_document(doc) {
            return None;
        }

        let html = proved_entry(&doc.url_path, previous, output_dir)?;
        let qr = match qr::share_qr_for_page(&doc.url_path, doc.is_public_page(), site_url) {
            Some(qr) => Some(proved_entry(&qr.served_path, previous, output_dir)?),
            None => None,
        };

        Some(Self { html, qr })
    }

    pub fn html_path(&self) -> &ServedPath {
        &self.html.path
    }

    pub fn register_all(&self, pending: &mut PendingManifest) {
        pending.register_hashed(
            &self.html.path,
            &self.html.manifest_value,
            HashBucket::Files,
        );
        if let Some(qr) = &self.qr {
            pending.register_hashed(&qr.path, &qr.manifest_value, HashBucket::Files);
        }
    }
}

fn proved_entry(path: &str, previous: &SiteHashes, output_dir: &Path) -> Option<CarriedEntry> {
    let served_path = ServedPath::from_source(path).ok()?;
    let manifest_value = previous.files.get(served_path.as_str())?.clone();
    crate::build::io_utils::output_present(&output_dir.join(served_path.as_str())).then_some(
        CarriedEntry {
            path: served_path,
            manifest_value,
        },
    )
}
