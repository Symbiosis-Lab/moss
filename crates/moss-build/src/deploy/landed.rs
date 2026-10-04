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
//! set's two inputs, so it owes the same re-derivation a seal does.
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

    let summary = record_what_is_live(&mp, sealed, target, history).await;

    ports.after_landing(folder, target, &summary).await;

    summary
}

/// Write the publish record, including which note ID is live at which page.
///
/// The record is one artifact carrying two facts, because one publish
/// establishes both at the same instant: the page hashes the next change set
/// diffs against, and the `{source path → uid}` map that rename detection and
/// duplicate-uid resolution read (`manifest::live_baseline`).
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
async fn record_what_is_live(
    mp: &MossPaths,
    sealed: &SealedManifest,
    target: &str,
    history: &HistoryStore,
) -> PageChangeSummary {
    let now = chrono::Utc::now().to_rfc3339();
    let live = read_live_article_mapping(mp).await;

    let summary = match &live {
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
                    let renames = redirects::detect_renames(&projection, &live.article_map);
                    change_record::build_page_change_records(
                        &prev_change_set,
                        &renames,
                        &live.article_map,
                        &projection.entries,
                        &removed,
                    )
                }
                live_baseline::Baseline::Absent | live_baseline::Baseline::Unreadable(_) => {
                    change_record::build_page_change_records(
                        &prev_change_set,
                        &HashMap::new(),
                        &live.article_map,
                        &[],
                        &removed,
                    )
                }
            }
        }
        None => PageChangeSummary::default(),
    };

    let mut record = PublishedSnapshot::from_sealed(sealed, target, now.clone());
    // An unreadable article map must not CLEAR what is live: the standing
    // record's note IDs are stale by one publish, which costs a missed rename,
    // where an empty map costs the whole rename baseline and every
    // duplicate-uid tiebreak with it. `triples` falls back the same way and for
    // the same reason — a stale set of triples still answers; `None` does not.
    match live {
        Some(live) => {
            record.uids = live.uids;
            record.triples = Some(live.triples);
        }
        None => {
            if let Some(prev) = published_record::load_for(mp, Some(target)) {
                record.uids = prev.uids;
                record.triples = prev.triples;
            }
        }
    }

    // Rebuilt rather than moved: `MossPaths` is a path wrapper, and this keeps
    // the borrow out of the blocking task.
    let root = mp.project_root().to_path_buf();
    let sealed_for_history = sealed.clone();
    let target_owned = target.to_string();
    let history_owned = history.clone();
    let written = tokio::task::spawn_blocking(move || {
        let outcome = published_record::save(&MossPaths::new(&root), &record);
        // Best-effort and independent of the write above: a publish-history
        // failure must never suppress the rename baseline, and vice versa.
        if let Err(e) = history_owned.snapshot(&root, &sealed_for_history, &target_owned, &now) {
            log::warn!("deploy: could not snapshot publish history: {e}");
        }
        outcome
    })
    .await;
    match written {
        Ok(Ok(())) => {}
        Ok(Err(e)) => log::warn!("deploy: could not record what went live: {e}"),
        Err(e) => log::warn!("deploy: recording what went live did not finish: {e}"),
    }

    summary
}

/// Write the publish record for a prebuilt tree that just went live.
///
/// Another tool built it, so moss knows no pages behind it: `sources` and
/// `source_to_output` are empty, which is the truth. `files` is the tree as
/// sent to the server. The next moss-built publish therefore counts every page
/// as added, because to the live site every page is. The note IDs last seen
/// live are carried forward from the standing record, as
/// [`record_what_is_live`] does when it has no article map to read: forgetting
/// them would drop the rename forwarding the site has earned.
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

/// What one publish record advances together, read off the same article map so
/// none of it can land out of step with the rest (the sealless
/// writer, deleted at track P slice P3, used to advance `uids` alone and leave
/// `source_to_output` frozen as of the last SEALED publish). `triples` is that
/// pair's replacement as `live_baseline`'s source — read here
/// beside `uids` rather than derived from it, because deriving it would be
/// exactly the join this struct exists to make unnecessary.
///
/// `article_map` is the same parsed value `uids`/`triples` were themselves
/// derived from, kept whole here too (publish-receipt design, step 4) — it is
/// `change_record::build_page_change_records`'s `current_map`, and re-reading
/// the same bytes a second time to get it back would be the join this struct
/// already exists to avoid, one field over.
struct LiveArticleMapping {
    uids: HashMap<String, String>,
    triples: Vec<crate::build::manifest::change_set::LiveEntry>,
    article_map: crate::build::scan::article_map::ArticleMap,
}

/// Both halves of what the tree that just went live says about itself, or
/// `None` when this build wrote no article map to read it from.
///
/// `None` leaves the standing record alone rather than clearing it: an empty
/// map reads as "nothing was ever published" and would suppress every
/// redirect the site has earned.
///
/// Async fs so a slow (e.g. iCloud-syncing) file cannot block a runtime worker.
async fn read_live_article_mapping(mp: &MossPaths) -> Option<LiveArticleMapping> {
    let src = mp.article_map();
    // allow:raw_read regenerable build output under `.moss/build.nosync/`,
    // where dataless is absent — and an unreadable one is handled either way.
    let bytes = match tokio::fs::read(&src).await {
        Ok(bytes) => bytes,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!("deploy: could not read the article map to record what is live: {e}");
            }
            return None;
        }
    };
    match serde_json::from_slice::<crate::build::scan::article_map::ArticleMap>(&bytes) {
        Ok(map) => Some(LiveArticleMapping {
            uids: live_baseline::uids_of(&map),
            triples: live_baseline::from_article_map(&map).entries,
            article_map: map,
        }),
        Err(e) => {
            log::warn!(
                "deploy: the article map does not parse ({e}) — leaving the standing \
                 record of what is live alone"
            );
            None
        }
    }
}

#[cfg(test)]
#[path = "landed_tests.rs"]
mod tests;
