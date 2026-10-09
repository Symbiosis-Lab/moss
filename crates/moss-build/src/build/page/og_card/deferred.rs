//! Title-only sharing cards settle after preview HTML is available.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use tokio::sync::mpsc::Sender;
use super::{CardInputs, CardOutput};
use crate::build::background::BuildError;
use crate::build::coordinator::EmitMessage;
use crate::build::manifest::HashBucket;
use crate::build::served_path::ServedPath;

/// Owned inputs for a title-only card. Plated covers retain their synchronous
/// decode and original-cover fallback in the page renderer.
#[derive(Debug, Clone)]
pub struct CardRequest {
    pub(super) served_path: ServedPath,
    title: String,
    site_name: String,
    bg_color: String,
    fg_color: String,
    accent_color: String,
    lang: crate::i18n::Language,
    vertical: bool,
}

impl CardRequest {
    pub(super) fn new(inputs: &CardInputs, served_path: ServedPath) -> Self {
        Self {
            served_path,
            title: inputs.title.into(),
            site_name: inputs.site_name.into(),
            bg_color: inputs.bg_color.into(),
            fg_color: inputs.fg_color.into(),
            accent_color: inputs.accent_color.into(),
            lang: inputs.lang,
            vertical: inputs.vertical,
        }
    }

    fn inputs(&self) -> CardInputs<'_> {
        CardInputs {
            title: &self.title, site_name: &self.site_name,
            bg_color: &self.bg_color, fg_color: &self.fg_color,
            accent_color: &self.accent_color, lang: self.lang,
            vertical: self.vertical, plate: None,
        }
    }
}

/// One blocking worker, one card allocation at a time. Keeping its sender until
/// completion makes every promised sharing image part of the existing seal.
pub(crate) fn render_requests(
    requests: Vec<CardRequest>,
    output_root: &Path,
    tx: Sender<EmitMessage>,
    cancelled: impl Fn() -> bool,
) -> Result<(), BuildError> {
    let _phase = crate::build::phase::PhaseTrace::start("render_sharing_cards");
    let mut emitted = HashSet::new();
    for request in requests {
        if cancelled() {
            return Err(BuildError::Worker("sharing card rendering cancelled".into()));
        }
        if !emitted.insert(request.served_path.as_str().to_string()) {
            continue;
        }
        let card = super::render_card(&request.inputs(), output_root, &HashMap::new())
            .map_err(|error| BuildError::Worker(format!("sharing card failed: {error}")))?;
        let hash = match &card {
            CardOutput::Rendered { bytes, .. } => crate::build::assets::paths::compute_binary_hash(bytes),
            CardOutput::Carried { entry, .. } => entry.clone(),
        };
        tx.blocking_send(EmitMessage::File {
            rel_path: card.served_path().as_str().into(),
            hash, bucket: HashBucket::ImageOutputs, oid: None,
        }).map_err(|_| BuildError::Worker("sharing card coordinator closed".into()))?;
    }
    if cancelled() {
        return Err(BuildError::Worker("sharing card rendering cancelled".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::background::BackgroundHandle;
    use crate::build::manifest::PendingManifest;
    use crate::types::content::SiteHashes;

    fn inputs() -> CardInputs<'static> {
        CardInputs {
            title: "Empty garden", site_name: "Garden", bg_color: "#faf8f5",
            fg_color: "#1a1816", accent_color: "#d97706",
            lang: crate::i18n::Language::En, vertical: false, plate: None,
        }
    }

    fn planned(root: &Path) -> (ServedPath, Vec<CardRequest>) {
        let previous = HashMap::new();
        let covers = crate::build::page::cover::FilenameCovers::default();
        let mut sink = super::super::OgSink::deferred(&previous, &covers);
        let path = sink.render(&inputs(), root).unwrap().clone();
        let (cards, requests) = sink.into_parts();
        assert!(cards.is_empty(), "foreground must not render PNG bytes");
        assert!(!path.to_disk(root).exists(), "foreground must not write a PNG");
        (path, requests)
    }

    #[test]
    fn deferred_empty_and_authored_home_plan_cards_before_preview() {
        for authored in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            if authored {
                std::fs::write(dir.path().join("index.md"), "# Garden").unwrap();
                std::fs::write(dir.path().join("leaf.md"), "# A new leaf").unwrap();
            }
            let output = dir.path().join(".moss/build.nosync/site");
            std::fs::create_dir_all(&output).unwrap();
            let project = crate::build::scan_folder(dir.path().to_str().unwrap()).unwrap();
            let mut pending = PendingManifest::for_build(SiteHashes::default(),
                crate::build::cloud_ledger::InputEvidence::new(dir.path()));
            let (_, background, _, _) = crate::build::render::generate_blocking_content_for_build(
                &crate::vault::paths::VaultRoot::resolve(dir.path()), &project, &output,
                None, None, false, crate::build::render::SiteConfig::default(),
                &mut pending, false, true,
            ).unwrap();
            assert_eq!(background.og_requests.len(), if authored { 2 } else { 1 });
            let pages = pending.take_unwritten_pages();
            for request in background.og_requests {
                assert!(!request.served_path.to_disk(&output).exists());
                assert!(pages.values().any(|html| html.contains(request.served_path.as_str())));
            }
        }
    }

    #[tokio::test]
    async fn deferred_card_seals_with_synchronous_bytes() {
        let deferred = tempfile::tempdir().unwrap();
        let synchronous = tempfile::tempdir().unwrap();
        let (path, requests) = planned(deferred.path());
        let output = deferred.path().to_path_buf();
        let handle = BackgroundHandle::spawn_with_pending(
            PendingManifest::new(SiteHashes::default()),
            move |tx, workers| {
                workers.spawn(async move {
                    tokio::task::spawn_blocking(move || render_requests(requests, &output, tx, || false))
                        .await.map_err(BuildError::from)?
                });
            },
        );
        let (sealed, _) = handle.await_completion().await.unwrap();
        let expected = super::super::render_card(&inputs(), synchronous.path(), &HashMap::new()).unwrap();
        assert_eq!(expected.served_path(), &path);
        let actual = std::fs::read(path.to_disk(deferred.path())).unwrap();
        assert_eq!(actual, std::fs::read(path.to_disk(synchronous.path())).unwrap());
        assert!(sealed.image_outputs().contains(path.as_str()));
        assert_eq!(sealed.files()[path.as_str()], crate::types::content::file_entry(
            &crate::build::assets::paths::compute_binary_hash(&actual)));
    }

    #[tokio::test]
    async fn cancellation_during_last_card_withholds_seal() {
        let dir = tempfile::tempdir().unwrap();
        let (path, requests) = planned(dir.path());
        let output = dir.path().to_path_buf();
        let handle = BackgroundHandle::spawn_with_pending(
            PendingManifest::new(SiteHashes::default()), move |tx, workers| {
                workers.spawn(async move {
                    tokio::task::spawn_blocking(move || {
                        let checks = std::sync::atomic::AtomicUsize::new(0);
                        render_requests(requests, &output, tx, ||
                            checks.fetch_add(1, std::sync::atomic::Ordering::Relaxed) > 0)
                    }).await.map_err(BuildError::from)?
                });
            },
        );
        assert!(matches!(handle.await_completion().await, Err(BuildError::Worker(_))));
        assert!(path.to_disk(dir.path()).exists(), "atomic write completes before cancellation is observed");
    }

    async fn settle_failure(cancelled: bool) {
        let dir = tempfile::tempdir().unwrap();
        let (path, requests) = planned(dir.path());
        // A file at the output directory forces card writing to fail.
        let output = dir.path().join("blocked");
        std::fs::write(&output, b"not a directory").unwrap();
        let handle = BackgroundHandle::spawn_with_pending(
            PendingManifest::new(SiteHashes::default()), move |tx, workers| {
                workers.spawn(async move {
                    tokio::task::spawn_blocking(move || render_requests(requests, &output, tx, || cancelled))
                        .await.map_err(BuildError::from)?
                });
            },
        );
        let result = handle.await_completion().await;
        assert!(matches!(result, Err(BuildError::Worker(_))), "failure must not return a sealed manifest");
        assert!(!path.to_disk(dir.path()).exists());
    }

    #[tokio::test]
    async fn rendering_failure_withholds_seal() { settle_failure(false).await; }

    #[tokio::test]
    async fn cancellation_withholds_seal() { settle_failure(true).await; }
}
