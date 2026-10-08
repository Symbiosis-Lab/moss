//! The sweep's per-folder loop — one task, ticking every [`super::TICK`],
//! for the life of the `FolderSession`.
//!
//! Split out of `sweep.rs` by responsibility: this file is the LIFECYCLE
//! loop (state threading, cancellation, panic isolation); the walk and the
//! progress/pacing helpers it calls live in the sibling `walk`/`progress`
//! modules.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::FutureExt;

use crate::build::watch::{drift, scope};
use crate::ops::watch::cadence::{Cadence, CadenceTicker, Tick};
use crate::ops::watch::{supervision, worker};
use crate::system::folder_session::FolderSession;

use super::{progress, walk};
use super::{CLOUD_WALK_EVERY_TICKS, HEARTBEAT_EVERY, LOCAL_WALK_EVERY_TICKS, UNAVAILABLE_AFTER};

pub(crate) async fn run(
    session: Arc<FolderSession>,
    host: super::SweepHost,
    mut unavailable_rx: tokio::sync::mpsc::Receiver<super::UnavailableFilesQuery>,
) {
    let super::SweepHost { dispatch, reporter, emit, cadence } = host;
    let folder = session.folder.clone();
    let folder_str = folder.to_string_lossy().to_string();
    let social = folder.join(".moss").join("data").join("social");
    let cloud_root = crate::build::cloud_provider::detect_from_path(&folder).is_some();
    let (tick_every, walk_every) =
        super::tick_and_walk_every(if cloud_root { CLOUD_WALK_EVERY_TICKS } else { LOCAL_WALK_EVERY_TICKS });

    let mut focused_revision = 0;
    promote_focused_source(&session, &mut focused_revision);

    // Seed observations before the first sleep. Requests enter the shared
    // pool in small batches so foreground page inputs retain capacity.
    let seed = while_servicing_queries(
        walk::run_pass(&folder, None, None, None, false),
        &mut unavailable_rx,
        &folder,
        None,
    )
    .await;
    let mut pending: HashSet<PathBuf> = seed.walk.dataless.iter().cloned().collect();
    let mut background_cursor = 0;
    progress::request_downloads(&pending, &mut background_cursor);
    if !pending.is_empty() {
        log::info!("cloud-sync: supervising {} file(s) still in the cloud", pending.len());
    }
    log::info!(
        target: "moss::build::watch",
        "Sweep active for '{}' ({} root, walk every {}s)",
        folder_str,
        if cloud_root { "cloud" } else { "local" },
        tick_every.as_secs_f32() * walk_every as f32
    );

    let mut total_seen: usize = pending.len();
    let mut observed_arrivals: usize = 0;
    let mut last_arrival = Instant::now();
    let mut arrivals_since_rebuild: usize = 0;
    let mut first_arrival_at: Option<Instant> = None;
    let mut stall_reported = false;
    let mut last_rebuild_at: Option<Instant> = None;
    let mut last_rebuild_duration = Duration::ZERO;
    let mut prev_targets: Option<Vec<PathBuf>> = None;
    // Every path already dispatched, with the stat it was dispatched at. A
    // rebuild is supposed to clear a drift (the file lands in the new
    // manifest); one that comes back UNCHANGED on disk after its rebuild is a
    // walk-vs-scan filter mismatch, and re-dispatching it every pass is a
    // rebuild storm — the exact failure this loop replaces. Sticky paths are
    // suppressed (one WARN) until they change again or a pass comes back
    // quiet. Keyed per PATH, never per set: see `undispatched_paths`.
    let mut dispatched_drift: std::collections::BTreeMap<String, drift::DriftStamp> =
        std::collections::BTreeMap::new();
    let mut sticky_warned = false;
    // Watcher supervision. Armed by a FRESH drift dispatch — a change the
    // watcher apparently missed — carrying the health counters at that
    // moment; matured at the next walk pass. Sticky drift never arms (a
    // walk-vs-scan filter mismatch is the sweep's own defect, not the
    // watcher's), and neither does the top-level entry diff (on macOS the
    // root is legitimately unwatched, so its silence proves nothing).
    let mut strike = supervision::StrikeArm::new();
    // A strike's recreation is itself a brief event-loss window, so the pass
    // after it runs immediately, whatever the local cadence.
    let mut force_walk = false;
    let mut unreadable_streak: u32 = u32::from(seed.walk.root_unreadable);
    // Consecutive blown passes. Separate from `unreadable_streak` on
    // purpose: the root check keeps passing on a chronically-stalling
    // filesystem, so a shared streak that section (1) resets every readable
    // tick could never reach the threshold — the folder would ERROR forever
    // without ever going unavailable.
    let mut blown_streak: u32 = 0;
    // First blow of an episode is an ERROR; the rest ride the heartbeat.
    let mut deadline_warned = false;
    // The previous pass's baseline, carrying the sweep's own stat-identity
    // refreshes between passes (a build's new manifest supersedes it — see
    // `drift::select_baseline`).
    let mut baseline_cache: Option<crate::types::content::SiteHashes> = None;
    // Where a blown pass stopped; the next pass resumes there.
    let mut resume_cursor: Option<String> = None;
    let mut panic_warned = false;
    let mut ticks: u64 = 0;
    let mut passes: u64 = 1;
    let mut drift_catches: u64 = 0;
    // Consecutive passes with no baseline to judge against — none sealed yet,
    // or a seal landing (`drift::select_baseline`). A streak, not a total: the
    // pause after every build is normal and resets it, so a large number in
    // one heartbeat line means a seal that never landed and drift silently
    // off — diagnosable from a production log (INFO+).
    let mut passes_without_baseline: u64 = 0;
    // The last (unavailable, degraded) pair told to the preview. Seeded
    // healthy: a webview mounts assuming health, so only genuine transitions
    // are worth an event — including the all-clear back to (false, false),
    // which is itself a transition from anything else.
    let mut told_health = (false, false);

    // The sleep follows the host's cadence (a backgrounded window ticks
    // slowly) unless the test seam shortened the tick, which wins.
    let tick_seam = tick_every != super::TICK;
    let mut ticker = CadenceTicker::new(cadence);
    let mut last_heartbeat = Instant::now();

    loop {
        let timer = async {
            if tick_seam {
                tokio::time::sleep(tick_every).await;
                Tick::Elapsed
            } else {
                ticker.next_tick().await
            }
        };
        let tick = while_servicing_queries(
            timer,
            &mut unavailable_rx,
            &folder,
            Some(&pending),
        )
        .await;
        if !tick_seam && matches!(tick, Tick::CadenceChanged(Cadence::Background)) {
            // A fresh flip to Background lets the newly armed slow sleep
            // govern instead of running this tick's body immediately.
            continue;
        }
        if session.cancel.is_cancelled() {
            return;
        }
        ticks += 1;

        // The tick body is panic-isolated: a panic anywhere in it killed the
        // sweep task silently in the session's JoinSet, with nothing to
        // restart it. A panicked tick is skipped; the loop, and the
        // invariant, continue.
        let tick = std::panic::AssertUnwindSafe(async {
        let now = Instant::now();
        promote_focused_source(&session, &mut focused_revision);
        // What this tick found, as the narrowest honest trigger — not a bool.
        // A drift pass that saw only content edits can take the SAME
        // incremental path an event-driven rebuild takes; dispatching it as
        // `Full` (which is what a bool forces) made a sweep catch cost a
        // full-site render where the pump's classification would have cost
        // one page. Contributions merge through `BuildTrigger::merge`, so
        // the structural sources below still win over a content-only drift.
        let mut dirty: Option<crate::build::BuildTrigger> = None;
        // Whether THIS tick's dirt (if any) came from the top-level read_dir
        // diff — the one change source the watcher never subscribes to, and
        // therefore must not be blamed for (see the StrikeArm decl above).
        let mut top_level_dirty = false;

        // ── (1) Root health + top-level target reconcile (cheap read_dir) ──
        let target_snapshot = scope::watch_targets_with_error(&folder);
        let root_readable = target_snapshot.is_ok();
        if root_readable {
            unreadable_streak = 0;
            // A readable root clears the verdict only when the PASSES are
            // healthy too — a folder whose every pass blows its deadline is
            // unavailable in every sense the user cares about, however fast
            // its root read_dir answers.
            if blown_streak < UNAVAILABLE_AFTER && session.set_unavailable(false) {
                log::info!(
                    target: "moss::build::watch",
                    "Project at '{}' is reachable again",
                    folder_str
                );
            }
            match target_snapshot {
                Ok(targets) => {
                    let desired: Vec<PathBuf> = targets.into_iter().map(|(p, _)| p).collect();
                    if let Some(prev) = &prev_targets {
                        if scope::watch_set_content_change(prev, &desired, root_readable, &social) {
                            log::info!(
                                target: "moss::build::watch",
                                "Sweep: top-level entry set changed — rebuilding"
                            );
                            // The entry set moved with no per-path evidence to pass,
                            // so this source is honestly `Full`.
                            dirty = Some(crate::build::BuildTrigger::Full);
                            top_level_dirty = true;
                        }
                    }
                    prev_targets = Some(desired);
                }
                Err(error) => log::debug!(
                    target: "moss::build::watch",
                    "Sweep: retaining prior watcher-target snapshot for {} after incomplete listing: {}",
                    folder.display(), error
                ),
            }
        } else {
            note_failed_pass(&session, &folder_str, &mut unreadable_streak, "root unreadable");
            // Keep the last complete target snapshot. The next successful
            // enumeration can compare against it; this failed read cannot
            // establish that anything disappeared.
        }

        // ── (2) Evicted-set arrival scan (every tick — the old fast path) ──
        // Arrivals BEFORE any walk: a walk returns what is in the cloud NOW,
        // so a file that arrived since last tick is simply missing from it —
        // walking first would drop it uncounted, and when that file is the
        // last one (the home page), nothing else would ever notice.
        let (arrived, deleted) = observe_pending(&mut pending);
        observed_arrivals += arrived;
        total_seen = total_seen.saturating_sub(deleted);
        // A deletion counts too: the set the gate waits on just shrank, and
        // only a build can lower the gate. Left uncounted, deleting the
        // awaited file from another device wedges the waiting screen at
        // "0 remaining" forever.
        let structural_arrivals = crate::build::cloud_readiness::storage::take_arrivals(&folder);
        let structural_wakes = structural_arrivals.wake_count();
        if arrived + deleted + structural_wakes > 0 {
            if deleted > 0 {
                log::info!("cloud-sync: {} awaited file(s) were deleted upstream", deleted);
            }
            arrivals_since_rebuild += arrived + deleted + structural_wakes;
            last_arrival = now;
            first_arrival_at.get_or_insert(now);
            stall_reported = false;
        }

        // ── (3) The stat pass (every Nth tick) ─────────────────────────────
        // "No worker registered" gates the DRIFT half, not the walk: a
        // drift dispatch in that window takes the inline-build fallback
        // instead of racing the first build. Downloads and arrivals keep
        // flowing regardless (a cloud-gated first build depends on them, and
        // arrivals deliberately keep the inline path — it is the only build
        // path that exists in that state).
        let worker = worker::get(&folder_str);
        let worker_busy = worker
            .as_ref()
            .map(|h| h.is_building() || h.slot_occupied())
            .unwrap_or(false);
        if (ticks % walk_every == 0 || force_walk)
            && root_readable
            && !worker_busy
            && !crate::deploy::freeze::publish_in_flight()
        {
            force_walk = false;
            let drift_allowed = worker.is_some();
            let stash = walk::stashed_baseline(&folder_str);
            let mut outcome = while_servicing_queries(
                walk::run_pass(
                    &folder,
                    stash,
                    baseline_cache.take(),
                    resume_cursor.take(),
                    drift_allowed,
                ),
                &mut unavailable_rx,
                &folder,
                Some(&pending),
            )
            .await;
            if session.cancel.is_cancelled() {
                return true;
            }
            passes += 1;
            baseline_cache = outcome.baseline.take();
            resume_cursor = outcome.resume.clone();

            if outcome.walk.root_unreadable {
                note_failed_pass(&session, &folder_str, &mut unreadable_streak, "root unreadable");
                // A vanished root is the environment failing, not the watcher;
                // striking on it would recreate a watcher whose targets are
                // gone. The unavailable verdict owns this failure.
                strike.disarm();
            } else {
                // ── Watcher supervision: mature the armed candidate ────────
                // This pass IS the aging boundary: a full sweep interval (at
                // least) has elapsed since the drift dispatch that armed it,
                // so an event that was merely in flight has long since landed.
                // Judged before this pass's own drift can re-arm.
                if strike.mature(&folder_str) {
                    // Cover the recreation's own event-loss gap with an
                    // immediate follow-up pass.
                    force_walk = true;
                }
                if outcome.blown() {
                    if !deadline_warned {
                        deadline_warned = true;
                        log::error!(
                            target: "moss::build::watch",
                            "Sweep pass for '{}' blew its {}s budget — I/O is too slow here \
                             (network filesystem?); resuming next pass at {:?}",
                            folder_str,
                            super::WALK_DEADLINE.as_secs(),
                            outcome.resume
                        );
                    }
                    note_failed_pass(
                        &session,
                        &folder_str,
                        &mut blown_streak,
                        "pass deadline blown",
                    );
                } else {
                    blown_streak = 0;
                    deadline_warned = false;
                }

                if outcome.complete && outcome.walk.files_watchable == 0 {
                    supervision::note_blind_folder(&folder_str, outcome.walk.files_seen);
                }

                if outcome.complete {
                    // Files that left the cloud between the arrival scan and
                    // this walk arrived unobserved; a COMPLETE walk is the
                    // authority, so anything it dropped counts as an arrival
                    // here (covers the upstream-delete case too — still a
                    // structural change).
                    let mut next: HashSet<PathBuf> = outcome.walk.dataless.iter().cloned().collect();
                    next.extend(pending.iter().filter(|p| pending_still_present(p) && !readable_arrival(p)).cloned());
                    let swallowed = pending.iter().filter(|p| !next.contains(*p)).count();
                    let arrived_unseen = pending.iter().filter(|p| !next.contains(*p) && readable_arrival(p)).count();
                    let deleted_unseen = pending.iter().filter(|p| !next.contains(*p) && pending_confirmed_absent(p)).count();
                    pending = next;
                    if swallowed > 0 {
                        log::debug!(
                            "cloud-sync: walk saw {} file(s) leave the cloud set",
                            swallowed
                        );
                        arrivals_since_rebuild += swallowed;
                        last_arrival = now;
                        first_arrival_at.get_or_insert(now);
                        stall_reported = false;
                    }
                    observed_arrivals += arrived_unseen;
                    total_seen = total_seen.saturating_sub(deleted_unseen);
                    total_seen = total_seen.max(observed_arrivals + pending.len());
                    progress::request_downloads(&pending, &mut background_cursor);
                } else {
                    // A partial pass (blown or resumed) has no authority
                    // over absences, but what it DID see dataless still
                    // feeds the readers — a blown pass that discards its
                    // partial set would starve exactly the slow trees that
                    // blow deadlines.
                    for p in &outcome.walk.dataless {
                        pending.insert(p.clone());
                    }
                    total_seen = total_seen.max(observed_arrivals + pending.len());
                    progress::request_downloads(&pending, &mut background_cursor);
                }

                // Baseline drift — the design's reason to exist. Partial
                // passes contribute modification/new-file verdicts for the
                // shard they covered; only a complete pass certifies quiet.
                if outcome.drift.is_some() {
                    passes_without_baseline = 0;
                }
                match outcome.drift {
                    Some(report) if report.is_quiet() => {
                        if outcome.complete {
                            // A clean full pass certifies the last dispatch
                            // worked; the next drift is a fresh incident.
                            dispatched_drift.clear();
                            sticky_warned = false;
                        }
                    }
                    Some(report) => {
                        let fresh = undispatched_paths(&report.drifted, &dispatched_drift);
                        if fresh.is_empty() {
                            // Every drifted path was dispatched already and is
                            // untouched since — a rebuild did not clear them,
                            // so another one won't.
                            if !sticky_warned {
                                sticky_warned = true;
                                log::warn!(
                                    target: "moss::build::watch",
                                    "Sweep: {} drifted path(s) survived their rebuild unchanged (e.g. {:?}) — \
                                     a walk-vs-scan filter mismatch; suppressing re-dispatch until they change",
                                    report.drifted.len(),
                                    report.drifted.keys().next()
                                );
                            }
                        } else {
                            for rel in &fresh {
                                if let Some(d) = report.drifted.get(rel) {
                                    dispatched_drift.insert(rel.clone(), d.stamp);
                                }
                            }
                            sticky_warned = false;
                            drift_catches += fresh.len() as u64;
                            log::info!(
                                target: "moss::build::watch",
                                "Sweep caught {} drifted path(s) the watcher never delivered (e.g. {:?}) — rebuilding{}",
                                fresh.len(),
                                fresh.first(),
                                if fresh.len() < report.drifted.len() {
                                    format!(" ({} suppressed as sticky)", report.drifted.len() - fresh.len())
                                } else {
                                    String::new()
                                }
                            );
                            // The trigger describes THE BATCH BEING SENT, so
                            // it is read off the filtered set: a create or a
                            // delete that is being suppressed must not make
                            // this dispatch structural, which would close both
                            // incremental gates over an ordinary edit.
                            let structural = report.any_structural(&fresh);
                            let paths: Vec<std::path::PathBuf> =
                                fresh.iter().map(|r| folder.join(r)).collect();
                            let found = if structural {
                                crate::build::BuildTrigger::Structural(paths)
                            } else {
                                crate::build::BuildTrigger::ContentOnly(paths)
                            };
                            dirty = Some(match dirty.take() {
                                Some(t) => t.merge(found),
                                None => found,
                            });
                            // Every drift catch is a change the watcher failed
                            // to deliver — arm the strike candidate with the
                            // health counters as they stand (unless this
                            // tick's top-level diff already explains it). If
                            // the watcher stays totally silent through the
                            // next pass too, the change cannot have been in
                            // flight, and the silence is the aged strike.
                            strike.arm(&folder_str, top_level_dirty);
                        }
                    }
                    None if drift_allowed => {
                        passes_without_baseline += 1;
                        // `passes_without_baseline` still counts every pass
                        // for whatever else reads it below; only the LOG is
                        // gated to the streak's first pass.
                        if walk::should_log_no_baseline_transition(passes_without_baseline) {
                            log::debug!(
                                target: "moss::build::watch",
                                "Sweep: no baseline this pass (nothing sealed yet, or a seal is landing) — drift skipped"
                            );
                        }
                    }
                    None => {
                        log::debug!(
                            target: "moss::build::watch",
                            "Sweep: drift suspended until the rebuild worker registers"
                        );
                    }
                }
            }
        }

        // ── (4) Progress / stall / unavailable reporting ───────────────────
        if pending.is_empty() && arrivals_since_rebuild == 0 {
            // Idle episode over; counters reset so the next eviction's
            // progress does not read "3 of 700".
            total_seen = 0;
            observed_arrivals = 0;
            stall_reported = false;
            progress::emit_progress(reporter.as_ref(), &folder, 0, 0, 0, 0, false, &pending);
        } else {
            // What the *site* waits for, which is not what moss downloads:
            // `should_request` asks for all of `.moss/`'s materialized set
            //, so `pending` routinely holds files no page renders
            // from — and waiting on one of those put a dismissal-less
            // full-window screen over a served site.
            let blocking = progress::blocking_count(&pending);
            let stalled = blocking > 0 && now.duration_since(last_arrival) >= super::STALL_AFTER;
            if stalled && !stall_reported {
                stall_reported = true;
                // The operator wants the whole outstanding set, not the
                // render-relevant slice: moss really is still fetching them.
                progress::report_stall(pending.len());
            }
            progress::emit_progress(reporter.as_ref(), &folder, total_seen, observed_arrivals, pending.len(), blocking, stalled, &pending);
        }

        // ── (5) Dispatch — always through the host's rebuild path ──────────
        // Measured-from-finish pacing: the worker records each completed
        // admission's finish time and wall duration, and anything newer than
        // our own enqueue-time approximation supersedes it — so
        // `should_rebuild`'s self-tuning arm (`max(floor, last build
        // duration)`) runs on the real build cost, not the milliseconds an
        // enqueue takes. The enqueue stamp below remains the fallback until
        // the folder's first completed build.
        if let Some((finished_at, duration)) =
            worker.as_ref().and_then(|h| h.last_completed())
        {
            if last_rebuild_at.is_none_or(|at| finished_at > at) {
                last_rebuild_at = Some(finished_at);
                last_rebuild_duration = duration;
            }
        }
        let arrivals_due =
            progress::should_rebuild(first_arrival_at, last_rebuild_at, last_rebuild_duration, now);
        if dirty.is_some() || arrivals_due {
            // Re-check cancellation immediately before dispatching: the
            // walk above can be seconds long, and a rebuild slipping
            // through after folder close repoints a live preview at a
            // folder the user closed.
            if session.cancel.is_cancelled() {
                return true;
            }
            // Known interaction, accepted: an arrival-driven dispatch here
            // shares the worker slot with the failed-build backoff — a
            // steady drip of arrivals can re-enqueue sooner than the backoff
            // alone would. The slot coalesces the requests and
            // MIN_REBUILD_INTERVAL floors the rate, so the cost is a bounded
            // extra attempt, and an arrival IS new input — the very thing a
            // failed build most plausibly wanted.
            let dispatched_at = Instant::now();
            // `ui_bound` brackets only the dispatch, never the loop —
            // `wait_for_in_flight_work` reads it as "deploy should wait".
            let _ui = UiBoundGuard::begin(&session);
            // An arrival is a file materializing out of the cloud — a create
            // as far as the site is concerned, and it carries no classification
            // of its own, so it takes the dispatch back to `Full`.
            let trigger = match (dirty.take(), arrivals_due) {
                (Some(t), false) => t,
                _ => crate::build::BuildTrigger::Full,
            };
            while_servicing_queries(
                dispatch(worker::RebuildRequest { rename_pairs: Vec::new(), trigger, gate_paths: None }),
                &mut unavailable_rx,
                &folder,
                Some(&pending),
            )
            .await;
            arrivals_since_rebuild = 0;
            first_arrival_at = None;
            // The dispatch is an ENQUEUE, so this stamp measures the
            // enqueue — a floor-only placeholder that the worker's
            // completion signal supersedes at the top of section (5) as soon
            // as the admitted build finishes and reports its real duration.
            last_rebuild_at = Some(Instant::now());
            last_rebuild_duration = last_rebuild_at.unwrap().duration_since(dispatched_at);
            last_arrival = Instant::now();
            stall_reported = false;
        }

        // ── (6) Folder health surfacing ──────────────────────────────────
        // The sweep is the folder's one periodic task, so it is also where
        // the two health verdicts — its own `unavailable` and the combined
        // `degraded` (`folder_degraded`, below) — become one transitions-only
        // event the preview can render. Polling here (≤2s latency on a
        // diagnostics surface) heals for free: whatever a missed tick
        // skipped, the next tick's comparison emits. Sent through the
        // host's relay, which is a no-op when nobody is listening, so there is
        // no "is there a window" check to make here.
        let health = (session.is_unavailable(), folder_degraded(worker.as_deref(), &folder_str));
        if health != told_health {
            told_health = health;
            emit(crate::types::events::MossEvent::FolderHealthChanged {
                folder: folder_str.clone(),
                unavailable: health.0,
                degraded: health.1,
            });
        }

        // ── (7) Heartbeat — a dead loop must be diagnosable from the log ──
        if last_heartbeat.elapsed() >= HEARTBEAT_EVERY {
            last_heartbeat = Instant::now();
            log::info!(
                target: "moss::build::watch",
                "Sweep alive for '{}': {} walk pass(es), {} drift catch(es), {} consecutive pass(es) without a baseline, {} file(s) pending",
                folder_str,
                passes,
                drift_catches,
                passes_without_baseline,
                pending.len()
            );
        }

        false // keep looping
        });
        let outcome = FutureExt::catch_unwind(tick).await;
        match outcome {
            Ok(true) => return,
            Ok(false) => panic_warned = false,
            Err(_) => {
                if !panic_warned {
                    panic_warned = true;
                    log::error!(
                        target: "moss::build::watch",
                        "Sweep tick for '{}' panicked — tick skipped, sweep continues \
                         (further panics ride the heartbeat)",
                        folder_str
                    );
                }
            }
        }
    }
}

/// The `degraded` half of `FolderHealthChanged` — worker watchdog OR watcher
/// backoff (`WatcherHealth::is_degraded`). Extracted for a unit test that
/// doesn't have to drive the whole sweep loop.
pub(crate) fn folder_degraded(worker: Option<&worker::WorkerHandle>, folder: &str) -> bool {
    worker.is_some_and(|h| h.is_degraded()) || supervision::get(folder).is_some_and(|h| h.is_degraded())
}

/// Wait for one existing owner future while answering detail requests from
/// the last completed pending snapshot. Pinning preserves the original work
/// across a request; a closed owner channel simply disables the side route.
pub(super) async fn while_servicing_queries<F: std::future::Future>(
    future: F,
    unavailable_rx: &mut tokio::sync::mpsc::Receiver<super::UnavailableFilesQuery>,
    folder: &std::path::Path,
    pending: Option<&HashSet<PathBuf>>,
) -> F::Output {
    tokio::pin!(future);
    loop {
        tokio::select! {
            output = &mut future => return output,
            query = unavailable_rx.recv() => match query {
                Some(query) => answer_unavailable_query(query, folder, pending),
                None => return future.await,
            },
        }
    }
}

pub(super) fn answer_unavailable_query(
    query: super::UnavailableFilesQuery,
    folder: &std::path::Path,
    pending: Option<&HashSet<PathBuf>>,
) {
    if query.reply.is_closed() {
        return;
    }
    let Some(pending) = pending else {
        let _ = query.reply.send(Err("Folder sweep is still collecting its status".to_string()));
        return;
    };
    let paths = crate::build::cloud_readiness::download_failure_paths_for(folder, pending);
    let files: Vec<String> = paths
        .iter()
        .filter_map(|path| path.strip_prefix(folder).ok())
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect();
    let result = if files.len() == paths.len() {
        Ok(files)
    } else {
        Err("A failed file was outside the active folder".to_string())
    };
    let _ = query.reply.send(result);
}

/// A pending path leaves the unresolved set only after it can be opened.
/// Disappearance and an unreadable present file are different outcomes.
/// These probes only stat and open read-only; they never read bytes, truncate
/// or coordinate materialization. Provider refusals keep the path pending;
/// actual materialization belongs to the byte pool.
fn readable_arrival(path: &std::path::Path) -> bool {
    !crate::build::icloud::is_still_in_the_cloud(path) && std::fs::File::open(path).is_ok()
}

/// Navigation may name a file that the sweep already queued in its background
/// batch. Promote that same queued read; leave unresolved URLs without a
/// source untouched until discovery supplies one.
pub(super) fn promote_focused_source(session: &FolderSession, last_revision: &mut u64) {
    let Some(requirement) = session.preview_requirement() else { return };
    if requirement.revision == *last_revision { return; }
    *last_revision = requirement.revision;
    if let crate::system::folder_session::PreviewSource::File(source) = requirement.source {
        let path = session.folder.join(source);
        if crate::build::icloud::is_still_in_the_cloud(&path) {
            crate::build::cloud_readiness::request_download_foreground(&path);
        }
    }
}

/// Remove only positively observed arrivals and confirmed deletions. A
/// present path whose open fails remains unresolved.
pub(super) fn observe_pending(pending: &mut HashSet<PathBuf>) -> (usize, usize) {
    let mut arrived = 0;
    let mut deleted = 0;
    pending.retain(|path| {
        if readable_arrival(path) {
            arrived += 1;
            return false;
        }
        if pending_confirmed_absent(path) {
            deleted += 1;
            return false;
        }
        true
    });
    (arrived, deleted)
}

/// Metadata uncertainty keeps the path pending. The cloud helper also checks
/// the pre-Sonoma `.name.icloud` representation before confirming absence.
fn pending_confirmed_absent(path: &std::path::Path) -> bool {
    #[cfg(test)]
    if crate::build::cloud_readiness::storage::test_probe(
        path,
        crate::build::cloud_readiness::StorageOperation::EntryMetadata,
    ).is_err() {
        return false;
    }
    match std::fs::metadata(path) {
        Ok(_) => false,
        Err(error) => crate::build::icloud::is_definitely_absent(path, &error),
    }
}

fn pending_still_present(path: &std::path::Path) -> bool {
    !pending_confirmed_absent(path)
}

/// Which drifted paths are worth dispatching: those whose stamp has MOVED
/// since the dispatch that was supposed to clear them (or that no dispatch
/// has seen at all).
///
/// Per path, never per set. Suppression used to compare whole fingerprints,
/// so one unrelated path joining or leaving re-dispatched every path in the
/// set — including ones a rebuild had provably failed to clear. A set that
/// oscillates therefore never suppressed anything: on the vault that
/// motivated this the sweep alternated between 877 and 1102 drifted paths
/// and the guard's WARN never fired once, through minutes of continuous
/// rebuilding.
pub(crate) fn undispatched_paths(
    now: &std::collections::BTreeMap<String, drift::Drift>,
    dispatched: &std::collections::BTreeMap<String, drift::DriftStamp>,
) -> Vec<String> {
    now.iter()
        .filter(|(rel, d)| dispatched.get(*rel) != Some(&d.stamp))
        .map(|(rel, _)| rel.clone())
        .collect()
}

/// One more failed pass (root unreadable, or deadline blown); at the
/// threshold the folder goes user-visibly unavailable — ONE error, no
/// rebuild storm. A renamed watch root lands here too (FSEvents strands on
/// the old path; we detect unavailability rather than follow the rename).
/// The caller owns which streak this feeds — root-unreadable and
/// deadline-blown accrue separately, because section (1) resets the
/// unreadable streak on every readable tick and a chronically-stalling
/// filesystem keeps its root readable.
pub(crate) fn note_failed_pass(session: &FolderSession, folder: &str, streak: &mut u32, reason: &str) {
    *streak = streak.saturating_add(1);
    if *streak == UNAVAILABLE_AFTER && !session.set_unavailable(true) {
        log::error!(
            target: "moss::build::watch",
            "Project at '{}' is unavailable: {} for {} consecutive sweep passes. \
             Watching for it to come back; no rebuilds until it does.",
            folder,
            reason,
            UNAVAILABLE_AFTER
        );
    }
}

/// Decrements the session's `ui_bound` counter however the scope exits — a
/// leaked count makes the folder look permanently busy and stalls every
/// publish (`has_ui_bound` is what the close handler and
/// `wait_for_in_flight_work` consult).
struct UiBoundGuard<'a>(&'a FolderSession);

impl<'a> UiBoundGuard<'a> {
    fn begin(session: &'a FolderSession) -> Self {
        session.begin_ui_bound();
        Self(session)
    }
}

impl Drop for UiBoundGuard<'_> {
    fn drop(&mut self) {
        self.0.end_ui_bound();
    }
}
