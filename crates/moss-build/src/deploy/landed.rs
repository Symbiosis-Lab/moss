//! What every publish does once it has landed, whoever shipped it.
//!
//! moss publishes two ways — [`super::push`] for moss hosting and
//! [`super::plugin_push`] for a deploy plugin (an OnionPress publish takes the
//! second; both the app and `moss deploy` enter the same two bodies). Both owe
//! the same two things afterwards, and they live here rather than twice
//! because the copies had already drifted:
//! the plugin one gated its change-set clear on having a seal, the seta one
//! did not, and only one of them was right.
//!
//! The two things are: record what went live, and **recompute** what is left to
//! publish. Recompute, not discard — a landed publish moves one of the change
//! set's two inputs, so it owes the same re-derivation a seal does. Recording
//! includes writing each published page's uid into the page itself: a build
//! never writes a source, so the publish is the first moment a uid is anchored
//! to something outside the folder.
//!
//! **Call only once the publish is known to have landed.** Each path has its
//! own proof of that — the server's commit returning 200, or the plugin
//! reporting success with a deployment (for OnionPress, after the receiver's
//! `/commit` returned). A record written for a publish that did not land makes
//! the next change set under-report: it tells the author nothing will change
//! when something will, which is the one direction of wrongness they cannot
//! correct from the UI.

use std::collections::HashMap;
use std::path::Path;

use crate::build::feeds::redirects;
use crate::build::manifest::{change_set, change_set::PublishedSnapshot, live_baseline, published_record, SealedManifest};
use crate::build::scan::article_map::ArticleMap;
use crate::build::scan::path_collision::on_disk;
use crate::deploy::change_record::{self, PageChangeSummary};
use crate::deploy::history::HistoryStore;
use crate::moss_paths::MossPaths;

/// Record what went live and recompute what is left to publish.
///
/// `sealed` is required: both plugin callers go through
/// `plugin_push::run_plugin_deploy_inner`, which refuses a sealless publish
/// before any bytes move — the directory it hands the plugin is
/// `.moss/build.nosync/current`, and without the manifest nothing can say which
/// generation that is.
///
/// `target` is `<method>:<id>` — `moss:<site_id>`, `onionpress:<url>`. It says
/// which host the record describes, so a site that switches hosts does not
/// diff against the other one's tree.
///
/// Both halves are best-effort in the same direction: a failed write costs one
/// degraded change set, never a failed deploy.
///
/// The recompute reaches the process through `ports`; a headless publish
/// answers the same question by having nothing to tell, which is an
/// implementation rather than an absence.
///
/// `history` is a plain `&HistoryStore`, not an `Option` — `HistoryStore::
/// in_vault` is infallible, a fixed join against the vault rather than an
/// app-data lookup, so every caller already has one to give; a test passes a
/// `HistoryStore::at(tempdir)`.
///
/// Returns the same [`PageChangeSummary`] it hands [`DeployPorts::
/// after_landing`](crate::build::ports::deploy::DeployPorts::after_landing) —
/// `push_site` needs it a second time, to verify the pages it names, and
/// reading it back off the return value avoids computing it twice.
pub async fn record_landed(
    folder: &Path,
    sealed: &SealedManifest,
    target: &str,
    ports: &dyn crate::build::ports::deploy::DeployPorts,
    history: &HistoryStore,
) -> PageChangeSummary {
    let mp = MossPaths::new(folder);

    // Read before the record moves: a copy is due only for a post that was
    // not live before.
    let copies_before = crate::syndication::landing::Before::capture(&mp, sealed, Some(target));
    let published = live_baseline::from_sealed(sealed);
    let live = match crate::build::manifest::publish_view::read_articles(&mp.generations_dir(), sealed.generation_id()) {
        Some(map) => Some(map),
        // A generation sealed before its article map was kept beside it. Delete
        // this arm, `read_live_article_map` and `pinned_to` once no such
        // generation can still be published.
        None => read_live_article_map(&mp).await.map(|map| pinned_to(map, &published)),
    };
    let copies = match (&copies_before, &live) {
        (Some(before), Some(live)) => before.decide(folder, live),
        _ => Vec::new(),
    };
    // Stamped before the record is written, so the record holds the stamped
    // bytes' hash: otherwise the next change set reads every newly published
    // page as edited, and syndication updates its copies.
    let stamps = stamp_published_uids(folder, sealed).await;

    let summary = record_what_is_live(&mp, sealed, target, history, live.as_ref(), published, &stamps.hashes).await;

    if let Some(before) = copies_before {
        before.apply(folder, copies, live.as_ref(), &stamps.failed);
    }

    ports.after_landing(folder, target, &summary).await;

    summary
}

/// Write the publish record, including which note ID is live at which page.
///
/// The record is one artifact carrying two facts, because one publish
/// establishes both at the same instant: the page hashes the next change set
/// diffs against, and `published`, the `{source path → uid}` map that rename
/// detection and duplicate-uid resolution read (`manifest::live_baseline`).
///
/// There is no sealless variant: a publish with no manifest is refused
/// upstream, and a branch that quietly degraded the rename baseline would
/// report nothing.
///
/// Best-effort throughout: a failed write costs the next build's rename
/// detection, never the publish. See [`record_landed`]'s doc for why
/// `history` is a plain `&HistoryStore` rather than an `Option`.
///
/// Returns the completion-scoped Added/Moved/Removed page summary a publish
/// receipt renders. It has to be computed
/// from the reads below BEFORE the write further down overwrites the record
/// they read — the same "before" the redirect stubs above already read for
/// the same reason. `PageChangeSummary::default()` when this build's own
/// article map cannot be read: with no `current_map` there is nothing to
/// diff, and guessing here would corrupt the rename baseline.
///
/// `stamped` is source path → the hash of the bytes the uid stamp left, for a
/// page whose bytes were the sealed ones: the record takes that hash, so the
/// next build sees the page unchanged and plans no resend for the stamp.
async fn record_what_is_live(
    mp: &MossPaths,
    sealed: &SealedManifest,
    target: &str,
    history: &HistoryStore,
    live: Option<&ArticleMap>,
    published: live_baseline::LiveBaseline,
    stamped: &HashMap<String, String>,
) -> PageChangeSummary {
    let now = chrono::Utc::now().to_rfc3339();

    let summary = match live {
        Some(live) => {
            // Both pure reads of the standing record, made before `save`
            // below moves it — `prev_snapshot` is this target's own last
            // publish (what `change_set::classify` diffs against);
            // `prev_baseline` is the cross-target union `detect_renames`
            // reads, the same baseline `feeds::redirects::emit_redirect_table`
            // loads at build time for the identical reason.
            let prev_snapshot = published_record::load_for(mp, Some(target));
            let prev_change_set = change_set::classify(prev_snapshot.as_ref(), sealed);
            // The list this build computed at its seal, against the baseline
            // the gate used. Recomputing here could disagree with it if another
            // target's publish moved the baseline since; only a landing with no
            // build in this process (nothing recorded) computes it afresh.
            let removed = crate::system::build_records::records()
                .removed_addresses(&mp.project_root().to_string_lossy())
                .unwrap_or_else(|| crate::build::manifest::backfill::removed_for_seal(mp, sealed));
            match live_baseline::load(mp) {
                live_baseline::Baseline::Present(projection) => {
                    let renames = redirects::detect_renames(&projection, live);
                    change_record::build_page_change_records(
                        &prev_change_set,
                        &renames,
                        live,
                        &projection.entries,
                        &removed,
                    )
                }
                live_baseline::Baseline::Absent | live_baseline::Baseline::Unreadable(_) => {
                    change_record::build_page_change_records(
                        &prev_change_set,
                        &HashMap::new(),
                        live,
                        &[],
                        &removed,
                    )
                }
            }
        }
        None => PageChangeSummary::default(),
    };

    let mut record = PublishedSnapshot::from_sealed(sealed, target, now.clone());
    for (path, hash) in stamped {
        if let Some(recorded) = record.sources.get_mut(path) {
            recorded.clone_from(hash);
        }
    }
    record.uids = published.entries.iter().map(|e| (e.source_path.clone(), e.uid.clone())).collect();
    record.triples = Some(published.entries);

    // Rebuilt rather than moved: `MossPaths` is a path wrapper, and this keeps
    // the borrow out of the blocking task.
    let root = mp.project_root().to_path_buf();
    let tree = crate::deploy::history::SourceTree::from_sealed(sealed);
    let target_owned = target.to_string();
    let history_owned = history.clone();
    let written = tokio::task::spawn_blocking(crate::build::phase::inherit_blocking(move || {
        let _phase = crate::build::phase::PhaseTrace::start("history_snapshot");
        let outcome = published_record::save(&MossPaths::new(&root), &record);
        // Best-effort and independent of the write above: a publish-history
        // failure must never suppress the rename baseline, and vice versa.
        if let Err(e) = history_owned.snapshot(&root, &tree, &target_owned, &now) {
            log::warn!("deploy: could not snapshot publish history: {e}");
        }
        outcome
    }))
    .await;
    match written {
        Ok(Ok(())) => {}
        Ok(Err(e)) => log::warn!("deploy: could not record what went live: {e}"),
        Err(e) => log::warn!("deploy: recording what went live did not finish: {e}"),
    }

    summary
}

/// What the uid stamp did, by source path.
#[derive(Default)]
struct Stamps {
    /// The hash of the stamped bytes, which the record takes.
    hashes: HashMap<String, String>,
    /// Why the uid could not be written; the next publish tries again.
    failed: HashMap<String, String>,
}

/// Write each published page's uid into its frontmatter, creating the block
/// when the note has none, so from now on the uid moves with the file through
/// renames. The uid is the one the sealed generation was built under, so the
/// file and its live comment thread agree.
///
/// Only a file that still holds the bytes that were published is stamped: one
/// edited since the build may carry a newer uid, or an edit the record must
/// still read as unpublished. It keeps the record's uid until a later publish
/// stamps it.
async fn stamp_published_uids(folder: &Path, sealed: &SealedManifest) -> Stamps {
    let pages: Vec<(String, String, String)> = sealed
        .published_uids()
        .filter_map(|(source, meta)| Some((source.to_string(), meta.uid.clone()?, sealed.sources().get(source)?.hash.clone())))
        .collect();
    let root = folder.to_path_buf();
    let stamped = tokio::task::spawn_blocking(move || {
        let mut stamps = Stamps::default();
        for (source, uid, published_hash) in pages {
            match stamp_one(&root, &source, &uid, &published_hash) {
                Ok(Some(hash)) => {
                    stamps.hashes.insert(source, hash);
                }
                Ok(None) => {}
                Err(why) => {
                    log::warn!("deploy: could not stamp the note ID into '{source}': {why}");
                    stamps.failed.insert(source, why);
                }
            }
        }
        if !stamps.hashes.is_empty() {
            log::info!("deploy: stamped the note ID into {} newly published page(s)", stamps.hashes.len());
        }
        stamps
    })
    .await;
    stamped.unwrap_or_else(|e| {
        log::warn!("deploy: stamping note IDs did not finish: {e}");
        Stamps::default()
    })
}

/// Write `uid` into the page at `source_path` if it still holds the published
/// bytes: the stamped bytes' hash, or `None` when the page was edited since
/// the build or already names the uid.
fn stamp_one(root: &Path, source_path: &str, uid: &str, published_hash: &str) -> Result<Option<String>, String> {
    let hash = |b: &[u8]| crate::build::render::blocking::source_metadata(None, b).hash;
    let written = crate::build::cloud_readiness::rewrite_input(&on_disk(root, source_path), |bytes| {
        if hash(bytes) != published_hash {
            return Ok(None);
        }
        let before = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
        if crate::build::markdown::parse_typed_frontmatter(before).uid.as_deref() == Some(uid) {
            return Ok(None);
        }
        let set = moss_core::frontmatter::FieldEdit::Set { key: "uid".into(), value: serde_yaml::Value::String(uid.to_string()) };
        moss_core::frontmatter::edit_fields(before, &[set]).map(Some)
    })?;
    Ok(written.map(|after| hash(after.as_bytes())))
}

/// Write the publish record for a prebuilt tree that just went live.
///
/// Another tool built it, so moss knows no pages behind it: `sources` and
/// `source_to_output` are empty, which is the truth. `files` is the tree as
/// sent to the server. The next moss-built publish therefore counts every page
/// as added, because to the live site every page is. The note IDs last seen
/// live are carried forward from the standing record: forgetting them would
/// drop the rename forwarding the site has earned.
///
/// Best-effort like the hosted record: a failed write costs a degraded change
/// set, never the publish.
pub fn record_prebuilt_landed(folder: &Path, generation_id: &str, target: &str, files: &HashMap<String, String>) {
    let mp = MossPaths::new(folder);
    let standing = published_record::load_for(&mp, Some(target));
    let record = PublishedSnapshot {
        generation_id: generation_id.to_string(),
        target: target.to_string(),
        published_at: chrono::Utc::now().to_rfc3339(),
        files: files.clone(),
        // Always `Some`: `None` would ask `live_baseline` to rebuild the
        // triples from `uids` and `source_to_output`, and this record has no
        // pages to rebuild them from.
        triples: Some(standing.and_then(|s| s.triples).unwrap_or_default()),
        ..Default::default()
    };
    if let Err(e) = published_record::save(&mp, &record) {
        log::warn!("deploy(prebuilt): could not record what went live: {e}");
    }
}

/// The article map the latest build wrote, or `None` when there is none to
/// read: for a generation sealed before its own map was kept beside it.
///
/// Async fs so a slow (e.g. iCloud-syncing) file cannot block a runtime worker.
async fn read_live_article_map(mp: &MossPaths) -> Option<ArticleMap> {
    let src = mp.article_map();
    // allow:raw_read regenerable build output under `.moss/build.nosync/`,
    // where dataless is absent — and an unreadable one is handled either way.
    let bytes = match tokio::fs::read(&src).await {
        Ok(bytes) => bytes,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!("deploy: could not read the article map: {e}");
            }
            return None;
        }
    };
    serde_json::from_slice(&bytes)
        .map_err(|e| log::warn!("deploy: the article map does not parse ({e})"))
        .ok()
}

/// The article map cut to the generation being published: its articles only,
/// at the addresses and under the uids it gave them. The map on disk belongs to
/// the latest build, and a preview rebuild may have replaced it while this
/// generation uploaded.
fn pinned_to(mut map: ArticleMap, published: &live_baseline::LiveBaseline) -> ArticleMap {
    let by_source: HashMap<&str, &live_baseline::LiveEntry> =
        published.entries.iter().map(|e| (e.source_path.as_str(), e)).collect();
    map.articles = std::mem::take(&mut map.articles)
        .into_values()
        .filter_map(|mut article| {
            let entry = by_source.get(article.source_path.as_str())?;
            article.uid = Some(entry.uid.clone());
            article.url_path.clone_from(&entry.url);
            Some((entry.url.clone(), article))
        })
        .collect();
    map
}

#[cfg(test)]
#[path = "landed_tests.rs"]
pub(crate) mod tests;
