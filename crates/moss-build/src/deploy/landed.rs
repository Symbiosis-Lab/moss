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
/// `sealed` was an `Option` until track P slice P3, because the plugin path
/// could arrive without a manifest and would then advance only what it could.
/// It cannot any more: both plugin callers go through
/// `plugin_push::run_plugin_deploy_inner`, which refuses a sealless publish
/// before any bytes move — the directory it hands the plugin is
/// `.moss/build/current`, and without the manifest nothing can say which
/// generation that is. The sealless writer went with the `Option`; its
/// behaviour is in git history and in `one_shot::require_sealed`'s doc.
///
/// `target` is `<method>:<id>` — `moss:<site_id>`, `onionpress:<url>`. It says
/// which host the record describes, so a site that switches hosts does not
/// diff against the other one's tree.
///
/// Both halves are best-effort in the same direction: a failed write costs one
/// degraded change set, never a failed deploy.
///
/// The recompute reaches the process through `ports` rather than an
/// `Option<&AppState>`. The `Option` was there because a caller might have no
/// state to recompute against; a headless publish answers the same question by
/// having nothing to tell, which is an implementation rather than an absence —
/// so the guard that used to return early is gone.
///
/// `history` stays `Option<&HistoryStore>` for a caller with none to give,
/// but `HistoryStore::in_vault` is infallible — a fixed join against the
/// vault, not an app-data lookup — so every production caller now has one:
/// they construct it once, right beside `ports`, and a test passes a
/// `HistoryStore::at(tempdir)`. There is no second, test-only entry point:
/// whichever store a caller has is the one this function uses.
///
/// Returns the same [`PageChangeSummary`] it hands [`DeployPorts::
/// after_landing`](crate::build::ports::deploy::DeployPorts::after_landing) —
/// `push_site` (task 4-6) needs it a second time, to verify the pages it
/// names, and reading it back off the return value is the one way to give it
/// that without computing it twice.
pub async fn record_landed(
    folder: &Path,
    sealed: &SealedManifest,
    target: &str,
    ports: &dyn crate::build::ports::deploy::DeployPorts,
    history: Option<&HistoryStore>,
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
/// duplicate-uid resolution read (`manifest::live_baseline`). They were two
/// files until moss#1079, and the second one was a byte copy of the whole
/// article map — 4.83 MB on a real vault to carry about 7 KB.
///
/// One shape since track P slice P3. There was a second — a publish with no
/// manifest advanced the uids and the page mapping on the STANDING record and
/// left the hashes alone — for the deploy-plugin path, which used to publish a
/// directory it could not name a generation for. That path now refuses instead,
/// so the branch was unreachable, and an unreachable branch that quietly
/// degrades the rename baseline is worse than absent: nothing would have
/// reported it running.
///
/// Best-effort throughout: a failed write costs the next build's rename
/// detection, never the publish. See [`record_landed`]'s doc for why
/// `history` is `Option` at all now that its construction cannot fail.
///
/// Returns the completion-scoped Added/Moved/Removed page summary a publish
/// receipt renders (publish-receipt design, step 4). It has to be computed
/// from the reads below BEFORE the write further down overwrites the record
/// they read — the same "before" the redirect stubs above already read for
/// the same reason. `PageChangeSummary::default()` when this build's own
/// article map cannot be read: with no `current_map` there is nothing to
/// diff, and guessing here is the moss#1079 mistake one call site over.
async fn record_what_is_live(
    mp: &MossPaths,
    sealed: &SealedManifest,
    target: &str,
    history: Option<&HistoryStore>,
) -> PageChangeSummary {
    let now = chrono::Utc::now().to_rfc3339();
    let live = read_live_article_mapping(mp).await;

    let summary = match &live {
        Some(live) => {
            // Both pure reads of the standing record, made before `save`
            // below moves it — `prev_snapshot` is this target's own last
            // publish (what `change_set::classify` diffs against);
            // `prev_baseline` is the cross-target union `detect_renames`
            // reads, the same baseline `feeds::redirects::emit_redirect_stubs`
            // loads at build time for the identical reason.
            let prev_snapshot = published_record::load_for(mp, Some(target));
            let prev_change_set = change_set::classify(prev_snapshot.as_ref(), sealed);
            match live_baseline::load(mp) {
                live_baseline::Baseline::Present(projection) => {
                    let renames = redirects::detect_renames(&projection, &live.article_map);
                    change_record::build_page_change_records(
                        &prev_change_set,
                        &renames,
                        &live.article_map,
                        &projection.entries,
                    )
                }
                live_baseline::Baseline::Absent | live_baseline::Baseline::Unreadable(_) => {
                    change_record::build_page_change_records(
                        &prev_change_set,
                        &HashMap::new(),
                        &live.article_map,
                        &[],
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
    let history_owned = history.cloned();
    let written = tokio::task::spawn_blocking(move || {
        let outcome = published_record::save(&MossPaths::new(&root), &record);
        // Best-effort and independent of the write above: a publish-history
        // failure must never suppress the rename baseline, and vice versa.
        // `None` (no app-data directory on this platform) is not a failure
        // to warn about — there is nowhere history could have lived.
        if let Some(store) = &history_owned {
            if let Err(e) = store.snapshot(&root, &sealed_for_history, &target_owned, &now) {
                log::warn!("deploy: could not snapshot publish history: {e}");
            }
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

/// What one publish record advances together, read off the same article map so
/// none of it can land out of step with the rest (moss#1089: the sealless
/// writer, deleted at track P slice P3, used to advance `uids` alone and leave
/// `source_to_output` frozen as of the last SEALED publish). `triples` is that
/// pair's replacement as `live_baseline`'s source (moss#1093) — read here
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
    // allow:raw_read regenerable build output under `.moss/build/` (ADR-043),
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
