//! Duplicate-uid resolution.
//!
//! ## Why a uid is not a disposable identifier
//!
//! `uid` is 8 random hex characters minted once by
//! [`generate_uid`](crate::build::markdown::generate_uid) and written back into
//! the note's frontmatter. It is **random, not derived** — the path argument is
//! ignored — so a uid that gets rewritten is gone: nothing can recompute it.
//!
//! It is also the join key for three things that live outside the vault and
//! cannot be repaired from here:
//!
//! - Artalk comment threads, keyed by `page_key` and baked into every deployed
//!   page as `data-page-key`.
//! - Schnorr-signed moderation events — the uid is *inside* the signed message,
//!   so changing it invalidates every prior hide/unhide.
//! - Deployed redirect stubs, which join last-deploy to current build by uid
//!   (`build::feeds::redirects::detect_renames`).
//!
//! ## Why the old rule was unsafe
//!
//! Two files share a uid on the most ordinary Obsidian action there is:
//! duplicating a note. The copy carries the original's frontmatter, uid and
//! all. One of them has to be reassigned.
//!
//! The rule used to be "earlier frontmatter `date` keeps it, tie-broken by
//! earlier file birth time". **iCloud Drive and Obsidian Sync reset both.**
//! btime is rewritten on every device that receives the file, and the copy
//! carries the original's `date` verbatim — so on a synced vault the copy can
//! look strictly older on both signals, and moss would rewrite the uid of the
//! *published* note, orphaning its comments server-side and invalidating its
//! moderation history. It reported this only through `log::warn!`.
//!
//! ## The rule now
//!
//! Published identity wins first. The publish record says which note ID was
//! live at which page, from which source file (`manifest::live_baseline`,
//! written when a publish lands); whichever contender it records under the
//! contested uid keeps it. The join is by exact source path, with a
//! case-insensitive **basename fallback** for the ordinary Obsidian actions
//! that change the path between deploys (folder move, case-only rename) — a
//! landed publish is the record's only writer, so any such action would
//! otherwise silently void the signal. Only when the record has no opinion —
//! neither file published, or both under one uid — does the date/btime
//! heuristic decide, and only when the record could be READ at all.
//!
//! ## Picking a keeper and reporting the risk are separate questions
//!
//! The two joins are not the same evidence at different confidences, and
//! [`PublishedIdentity`] keeps them apart. An exact path match *proves* which
//! file is live. A basename match is merely **consistent** with the published
//! file having moved — and just as consistent with the published file having
//! been renamed while a duplicate kept its old name, in which case the match
//! lands on the copy. The two histories leave identical evidence here.
//!
//! So the fallback decides the keeper (it beats the timestamp heuristic, which
//! on a synced vault is systematically wrong rather than merely uninformative)
//! but never clears [`UidReassignment::live_thread_at_risk`]. That flag asks
//! "was the published file *proven*?", not "did we manage to pick someone?" —
//! and the inferred case is precisely the one where the keeper may be the copy,
//! so it is the case the author most needs told. Whenever the snapshot shows
//! the uid live and no exact match identified it, the advisory says a live
//! deployment exists under this id and its comments may follow the wrong file.
//!
//! And when the snapshot cannot be READ at all — the provider is withholding
//! it, or it is corrupt — the pass defers: no keeper election, no reassignment,
//! no frontmatter write, and an advisory saying so. The heuristic is not a
//! safe floor here. On a synced vault it is reversed, so the case where moss
//! knows least is exactly the case where it was most likely to rewrite the
//! published note's uid (moss#1079).
//!
//! Every reassignment is returned to the caller so it can be surfaced to the
//! user (`progress::make_duplicate_uid_advisory`); a silent identity change is
//! how this class of bug stays invisible.

use crate::build::manifest::live_baseline::{Baseline, Unreadable};
use crate::build::types::ParsedDocument;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::time::SystemTime;

/// How strongly the deployed snapshot ties a contender to the contested uid.
///
/// The distinction exists because the two joins carry different *evidence*, not
/// different confidence about the same evidence. An exact path match is proof:
/// that path is what the last deploy recorded. A basename match is only
/// consistent with the published file having moved — and equally consistent
/// with the published file having been **renamed** while some other duplicate
/// kept the old name. Those two histories are indistinguishable from here (both
/// leave exactly one contender whose basename matches and whose directory
/// differs), so the basename join can pick a keeper but can never license the
/// claim that nothing is at stake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishedIdentity {
    /// The snapshot does not tie this contender to the uid.
    No,
    /// Basename-only match: the deployed path no longer exists and this is the
    /// only contender named like it. Enough to choose a keeper, never enough to
    /// clear [`UidReassignment::live_thread_at_risk`].
    Inferred,
    /// The snapshot records this exact source path under this uid.
    Exact,
}

/// One file contending for a contested uid.
#[derive(Debug, Clone)]
pub struct UidContender {
    /// Vault-relative source path, as stored on [`ParsedDocument::source_path`].
    pub source_path: String,
    /// Frontmatter `date`, if the note declares one.
    pub date: Option<String>,
    /// Filesystem birth time, if the platform reports one. Unreliable on
    /// synced vaults — see the module docs.
    pub birth_time: Option<SystemTime>,
    /// How the last-deployed article-map ties this file to the contested uid —
    /// i.e. how strongly it claims this is the page whose comment thread and
    /// signed moderation events are live.
    pub published: PublishedIdentity,
}

/// One resolved collision: who kept the uid, who was given a fresh one.
#[derive(Debug, Clone, PartialEq)]
pub struct UidReassignment {
    /// The contested uid — the one the keeper retained.
    pub uid: String,
    /// Source path of the file that kept `uid`.
    pub keeper_path: String,
    /// Source path of the file that was given a new uid.
    pub reassigned_path: String,
    /// The fresh uid written into the reassigned file's frontmatter.
    pub new_uid: String,
    /// True when the deployed snapshot records this uid as LIVE but the
    /// exact-path join did not identify the published file among the
    /// contenders — so the keeper was chosen by the basename inference or by
    /// the date/btime heuristic, and the live comment thread may follow the
    /// wrong file. The advisory must say so.
    ///
    /// A successful basename fallback does **not** clear this: it picks a
    /// keeper without proving one, and the history it cannot rule out (the
    /// published file renamed, a copy left holding its old name) is exactly the
    /// one where the keeper is the wrong file. See [`PublishedIdentity`].
    pub live_thread_at_risk: bool,
}

/// Decide which contender keeps the contested uid; returns an index into
/// `contenders`.
///
/// Precedence, highest first:
///
/// 1. **Proven published identity** — exactly one contender recorded in the
///    deployed article-map under this uid at that exact path. That file owns
///    the live comment thread and the signed moderation history, and neither
///    can be moved, so it keeps the uid no matter what the timestamps say.
/// 2. **Inferred published identity** — exactly one contender matching the
///    deployed *basename* when the deployed path itself is gone. Weaker
///    evidence (see [`PublishedIdentity::Inferred`]), but still strictly better
///    than the timestamp heuristic, which on a synced vault is not merely
///    uninformative but systematically wrong: the copy carries the original's
///    `date` and gets a fresh btime on every device that receives it, so it
///    tends to look *older* than the original on both signals.
/// 3. Earlier frontmatter `date` (missing date sorts last).
/// 4. Earlier file birth time.
/// 5. Lexicographically smaller source path — so the outcome does not depend
///    on filesystem scan order.
///
/// Steps 3–5 only ever run when the snapshot has no opinion at all. An empty
/// slice returns 0; callers must not index with it.
pub fn pick_uid_keeper(contenders: &[UidContender]) -> usize {
    // 1–2. Published identity — but only when it is unambiguous. Exactly one
    //    contender at a given strength means that file is the one the snapshot
    //    points to; anything else is a copy. Zero (never deployed) or more than
    //    one (corrupt snapshot, or two files sharing a basename) carries no
    //    signal at that strength, so fall through rather than guess. Exact is
    //    tried first and independently: one proven match wins even when several
    //    contenders would also match by basename.
    for strength in [PublishedIdentity::Exact, PublishedIdentity::Inferred] {
        let mut matches = contenders
            .iter()
            .enumerate()
            .filter(|(_, c)| c.published == strength);
        if let Some((idx, _)) = matches.next() {
            if matches.next().is_none() {
                return idx;
            }
        }
    }

    // 3–5. The snapshot could not identify anyone; order by the local signals.
    let mut order: Vec<usize> = (0..contenders.len()).collect();
    order.sort_by(|&a, &b| {
        let date_a = contenders[a].date.as_deref().unwrap_or("9999");
        let date_b = contenders[b].date.as_deref().unwrap_or("9999");
        date_a
            .cmp(date_b)
            .then_with(|| contenders[a].birth_time.cmp(&contenders[b].birth_time))
            .then_with(|| contenders[a].source_path.cmp(&contenders[b].source_path))
    });
    order.first().copied().unwrap_or(0)
}

/// One uid two or more files still share, because moss would not guess which
/// of them is the published one.
#[derive(Debug, Clone, PartialEq)]
pub struct DeferredCollision {
    /// The contested note ID.
    pub uid: String,
    /// Every source path carrying it, in `documents` order.
    pub paths: Vec<String>,
}

/// What one build's duplicate-uid pass did — and, when it did nothing, why.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UidResolution {
    /// Collisions that were resolved: who kept the uid, who was given a new one.
    pub reassignments: Vec<UidReassignment>,
    /// Contested uids left alone because the record of what is live could not
    /// be read. Empty in every ordinary build.
    ///
    /// Carries the colliding source paths, not just the uid: the files are what
    /// the author is told about, and only this pass knows them — recomputing
    /// them from `documents` in the caller would be the same join written twice.
    pub deferred: Vec<DeferredCollision>,
    /// Why they were deferred. `Some` exactly when `deferred` is non-empty.
    pub deferred_because: Option<Unreadable>,
}

/// Resolve every duplicate uid across `documents`.
///
/// Mutates the losers' `uid` in place and rewrites their frontmatter under
/// `source_root`. The keeper is chosen by [`pick_uid_keeper`].
///
/// `load_deployed` yields the record of what is live
/// ([`crate::build::manifest::live_baseline::load`]) and is called **only when a
/// collision actually exists** — this runs inside the latency-sensitive
/// blocking phase of a build that usually has zero duplicates.
///
/// ## An unreadable record defers the whole pass
///
/// Not "falls back to the heuristic" — **defers**. Reassignment writes a fresh
/// uid into a source file's frontmatter, and a uid is random, minted once, and
/// the join key for live comment threads and signed moderation events: there is
/// no undo. The fallback the code used to take in this case (frontmatter `date`,
/// then file birth time) is not merely uninformative on a synced vault, it is
/// systematically *reversed* — the copy carries the original's date and gets a
/// fresh btime on every device that receives it — so an unreadable record made
/// moss most likely to rewrite the uid of the published note (moss#1079).
///
/// It defers less than it used to. `built_before` answers whether a source path
/// was in the previous build's manifest, and a path this install has never
/// built has never been rendered, let alone published — so its uid can be
/// re-minted with no record at all. Duplicating a note produces exactly one
/// unknown path beside one known one, which is why the ordinary case now
/// resolves silently even while the record is unreadable (2026-08-30). Only a
/// group where every file is already known still defers.
///
/// Deferring costs one build's collision handling and self-heals: the next
/// build retries, which is already this module's model for a write that failed.
///
/// Frontmatter write-back is best-effort: a source file that cannot be read or
/// written must not fail the build, and the next build retries.
pub fn resolve_duplicate_uids(
    documents: &mut [ParsedDocument],
    source_root: &Path,
    load_deployed: impl FnOnce() -> Baseline,
    built_before: impl Fn(&str) -> bool,
) -> UidResolution {
    // BTreeMap, not HashMap: collision groups are processed in a stable uid
    // order so a multi-collision build produces the same result every run.
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, doc) in documents.iter().enumerate() {
        if let Some(uid) = doc.uid.as_ref() {
            groups.entry(uid.clone()).or_default().push(i);
        }
    }
    if !groups.values().any(|indices| indices.len() >= 2) {
        return UidResolution::default();
    }

    let baseline = load_deployed();
    // `Unreadable` is not an answer about what is live; it is the absence of
    // one. Kept separately from `published` so the group loop can tell "nothing
    // is published under this uid" from "moss cannot see what is published".
    let unreadable = match &baseline {
        Baseline::Unreadable(why) => Some(*why),
        _ => None,
    };
    let published = match &baseline {
        Baseline::Present(projection) => projection.paths_by_uid(),
        // Absent is a real answer — nothing has ever been published, so no
        // contender can own a live thread and the heuristic is all there is.
        Baseline::Absent | Baseline::Unreadable(_) => HashMap::new(),
    };
    let mut reassignments = Vec::new();
    let mut deferred = Vec::new();
    for (uid, indices) in groups {
        if indices.len() < 2 {
            continue;
        }
        let live = published.get(&uid);
        let mut contenders: Vec<UidContender> = indices
            .iter()
            .map(|&i| {
                let source_path = documents[i].source_path.clone().unwrap_or_default();
                let birth_time = std::fs::metadata(source_root.join(&source_path))
                    .ok()
                    .and_then(|m| m.created().ok());
                UidContender {
                    published: if live.is_some_and(|paths| paths.contains(&source_path)) {
                        PublishedIdentity::Exact
                    } else {
                        PublishedIdentity::No
                    },
                    date: documents[i].date.clone(),
                    birth_time,
                    source_path,
                }
            })
            .collect();

        // Exact-path join came up empty while the uid IS live: a deploy is
        // the snapshot's only writer, so a folder move or case-only rename
        // since the last deploy — the ordinary Obsidian actions — voids the
        // join even though the published file is right here. Fall back to a
        // case-insensitive basename match against the deployed path(s), and
        // keep the exactly-one-match rule: an ambiguous basename is no
        // signal, same as an ambiguous exact match.
        //
        // The result is `Inferred`, never `Exact`, and the difference is the
        // whole point. A basename match cannot distinguish "the published file
        // moved" from "the published file was RENAMED and a copy kept its old
        // name" — in the second history the match lands on the copy, and
        // treating that as proof would rewrite the published file's uid while
        // reporting that nothing was at stake. It picks a keeper; it does not
        // clear the flag.
        if let Some(live_paths) = live {
            if !contenders
                .iter()
                .any(|c| c.published == PublishedIdentity::Exact)
            {
                let live_names: HashSet<String> =
                    live_paths.iter().filter_map(|p| basename_lower(p)).collect();
                let matched: Vec<usize> = contenders
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| {
                        basename_lower(&c.source_path).is_some_and(|n| live_names.contains(&n))
                    })
                    .map(|(i, _)| i)
                    .collect();
                if let [only] = matched[..] {
                    contenders[only].published = PublishedIdentity::Inferred;
                }
            }
        }

        // Live-but-unproven: the snapshot says something IS deployed under this
        // uid, yet no contender could be identified as the published file by
        // the one join that proves it. A keeper still gets picked — by the
        // basename inference or, failing that, the heuristic — but the advisory
        // must not pretend nothing was at stake. Counting only `Exact` is
        // deliberate: an inferred match is exactly the case where the keeper
        // may be the copy, so it is the case the author most needs told.
        let (keeper, live_thread_at_risk) = match unreadable {
            None => (
                pick_uid_keeper(&contenders),
                live.is_some()
                    && contenders
                        .iter()
                        .filter(|c| c.published == PublishedIdentity::Exact)
                        .count()
                        != 1,
            ),
            // No record to read, so the question "which of these is published?"
            // has no evidence — but "which of these could NOT be published?"
            // still does, and it is enough. A source path this install has
            // never built has never been rendered, let alone deployed, so
            // re-minting its uid cannot orphan a comment thread. That is the
            // whole safety argument, and it holds without any timestamp: the
            // signal is moss's own build manifest, not file metadata that
            // iCloud and Obsidian Sync rewrite (moss#1079).
            //
            // Duplicating a note — the action that causes essentially every
            // collision — produces exactly this shape: one path moss knows and
            // one it has never seen. When the shape is anything else (all of
            // them known, none of them known) there is no safe re-mint, and
            // the group defers as before.
            Some(_) => {
                let known: Vec<usize> = contenders
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| built_before(&c.source_path))
                    .map(|(i, _)| i)
                    .collect();
                match known[..] {
                    [only] => (only, false),
                    _ => {
                        deferred.push(DeferredCollision {
                            uid: uid.clone(),
                            paths: contenders.iter().map(|c| c.source_path.clone()).collect(),
                        });
                        continue;
                    }
                }
            }
        };
        let Some(keeper_path) = contenders.get(keeper).map(|c| c.source_path.clone()) else {
            continue;
        };

        for (pos, &idx) in indices.iter().enumerate() {
            if pos == keeper {
                continue;
            }
            let Some(reassigned_path) = contenders.get(pos).map(|c| c.source_path.clone()) else {
                continue;
            };
            let new_uid = crate::build::markdown::generate_uid(&reassigned_path);
            let file_path = source_root.join(&reassigned_path);
            if !persist_reassigned_uid(&file_path, &reassigned_path, &new_uid) {
                continue;
            }
            documents[idx].uid = Some(new_uid.clone());
            reassignments.push(UidReassignment {
                uid: uid.clone(),
                keeper_path: keeper_path.clone(),
                reassigned_path,
                new_uid,
                live_thread_at_risk,
            });
        }
    }
    if let (Some(why), false) = (unreadable, deferred.is_empty()) {
        // The one place this is logged, and it reaches only the log: nothing
        // here is the author's to fix. `log_warn_problem!` carries it to the
        // log ring, Send Logs and `moss build --strict`'s problem count.
        crate::build::cli_output::log_warn_problem!(
            "{} duplicate note ID(s) left unresolved: {} — and every file carrying \
             them was already known to a previous build, so none could be re-minted \
             without risking a published note's ID.",
            deferred.len(),
            why.describe()
        );
    }
    UidResolution {
        reassignments,
        deferred_because: unreadable.filter(|_| !deferred.is_empty()),
        deferred,
    }
}

/// Writes `new_uid` into the file's frontmatter, returning whether the file on
/// disk is now known to carry it.
///
/// The return value gates the in-memory reassignment. A uid the build believes
/// it assigned but never persisted is worse than no reassignment at all: the
/// next build reads the old uid and collides again, while this build has
/// already emitted — and signed — moderation events against a uid no source
/// file claims. So an unreadable or unwritable file leaves the document on its
/// original uid and the collision advisory unreported for that file.
fn persist_reassigned_uid(file_path: &Path, display_path: &str, new_uid: &str) -> bool {
    let content = match crate::build::cloud_readiness::read_to_string_with_materialize_wait(
        file_path,
        crate::build::cloud_readiness::MATERIALIZE_DEADLINE,
    ) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Cannot read '{}' to reassign its uid: {}", display_path, e);
            return false;
        }
    };

    let updated = crate::build::markdown::replace_uid_in_frontmatter(&content, new_uid);
    if updated == content {
        // Either the file already carries this uid — nothing to write — or it
        // has no `uid:` line for the replacement to land on, in which case the
        // reassignment silently did not happen.
        return content.contains(&format!("uid: \"{new_uid}\""));
    }

    match std::fs::write(file_path, &updated) {  // allow:raw_write the user's own markdown source — 'unreadable is not absent' applies unqualified
        Ok(()) => true,
        Err(e) => {
            log::warn!(
                "Failed to write reassigned uid into '{}': {}",
                display_path,
                e
            );
            false
        }
    }
}

/// Lower-cased final path component — the rename-tolerant join key for the
/// deployed-snapshot fallback (case-insensitive because APFS is, and a
/// case-only rename is one of the actions the fallback exists to survive).
fn basename_lower(path: &str) -> Option<String> {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
}

#[cfg(test)]
#[path = "uid_dedup_tests.rs"]
mod tests;
