//! Mass-removal deploy guard — pure decision logic, no I/O.
//!
//! Refuses a deploy whose removal set would wipe most/all of the live site:
//! the signature of an empty or stale build manifest (2026-06-14 incident: a
//! deleted hashes.json produced an empty manifest → server diff said "remove
//! everything" → 404), not a real intent to delete the site. The guard's
//! decision table lives (and is tested) in one place; `deploy.rs` calls the
//! diff-taking wrapper so the field mapping is covered by unit tests too.

/// Guard against a deploy whose removal set would wipe most/all of the live
/// site — the signature of an empty or stale build manifest, not an
/// intentional deletion. Pure (no I/O) so the decision is unit-testable.
///
/// `remove` is the server's removal list from the sync diff. Paths under
/// `_moss/` (internal derived assets: OG cards, hashed js/css) are
/// content-addressed and rotate en masse on site-wide changes — a home-page
/// rename changes the site name, which rotates EVERY OG-card filename — so
/// they are never evidence of a wipe. New setas already exclude them from
/// `remove` at the response boundary; we filter again here as defense in
/// depth against older servers that send the list unfiltered (2026-07-22
/// incident: 158 rotated OG cards counted as removals → "remove 158 of 169
/// (93%)" blocked a healthy publish of a 366-file live site, true fraction
/// 43%). The filter only affects this guard's signal — actual file removal
/// happens implicitly via the server's generation swap at commit, so nothing
/// about what gets deleted changes.
///
/// Decision, after filtering (`content_remove` = non-`_moss/` removals):
///   - `server_total` present AND consistent (a total smaller than the
///     removal list contradicts itself — removals come from the server's own
///     manifest — so an inconsistent total is distrusted and falls through
///     to the derived branch, which fails safe): block on the
///     manifest-collapse signature — the new build is less than half the
///     live site AND (`content_remove >= MIN_ABS` OR the collapse is extreme,
///     manifest under a quarter of the live site). The extreme clause keeps
///     small sites (< MIN_ABS content pages) as protected as the pre-rework
///     guard, whose raw removal count was inflated past MIN_ABS by `_moss/`
///     rotations. Site-wide churn (rename: manifest ≈ live) and mass
///     slug-reorgs (manifest still full-size) pass; the real catastrophe
///     (empty/stale manifest) is caught.
///   - `server_total` absent (older setas): derive the live count from the
///     diff over content-filtered counts:
///       server set S, new manifest M; `need` ⊇ M\S (missing OR
///       hash-differs!), `remove` = S\M → live ≈ (|M| − |need|) + |remove|.
///     KNOWN LIMITATIONS of this branch: `need` includes changed-in-place
///     files, so the derived denominator deflates and a mass slug-reorg
///     still false-positives; and a small-site wipe (< MIN_ABS content
///     removals) passes undetected. Both go away once every seta ships
///     `server_total`.
///
/// `MOSS_DEPLOY_ALLOW_MASS_REMOVE` (passed as `allow_override`) bypasses the
/// guard for a genuinely intended large purge.
pub(super) fn check_mass_removal(
    manifest_count: usize,
    need_count: usize,
    remove: &[String],
    server_total: Option<usize>,
    allow_override: bool,
) -> Result<(), String> {
    /// Below this many removed content files, never block (small sites
    /// legitimately churn wholesale).
    const MASS_REMOVE_MIN_ABS: usize = 10;
    /// Fallback path only: block when content removals are at least this
    /// fraction of the derived live count.
    const MASS_REMOVE_FRACTION: f64 = 0.5;

    let content_remove = remove.iter().filter(|p| !p.starts_with("_moss/")).count();

    if content_remove == 0 || allow_override {
        return Ok(());
    }

    // Distrust a self-contradictory total (fewer live files than the server
    // itself asks to remove — includes the degenerate Some(0), where
    // `total / 2 == 0` would disarm the collapse check entirely). The
    // derived branch below fails safe on the wipe shapes.
    let trusted_total = server_total.filter(|&t| t >= content_remove);

    match trusted_total {
        Some(total) => {
            // Manifest-collapse signature: the new build is drastically
            // smaller than the live site. Below MIN_ABS content removals,
            // only an EXTREME collapse (under a quarter of the live site)
            // blocks — a tiny site legitimately churning wholesale keeps its
            // manifest near live-size, while a small-site wipe does not.
            let suspicious =
                content_remove >= MASS_REMOVE_MIN_ABS || manifest_count * 4 < total;
            if suspicious && manifest_count < total / 2 {
                return Err(format!(
                    "Deploy aborted: the new build contains only {} files while the \
                     live site has {} — publishing it would remove {} of them. This \
                     almost always means the build is empty or stale (e.g. a missing \
                     hashes.json), not an intentional deletion. Rebuild the folder and \
                     retry. As a last resort, if you genuinely intend to remove most \
                     of the site, set MOSS_DEPLOY_ALLOW_MASS_REMOVE=1 and re-run.",
                    manifest_count, total, content_remove
                ));
            }
        }
        None => {
            // Old server: no server_total on the wire. Derive the live count
            // from the diff (see doc comment — deflates on in-place changes,
            // so this still false-positives on mass slug-reorgs).
            if content_remove < MASS_REMOVE_MIN_ABS {
                return Ok(());
            }
            let live = manifest_count.saturating_sub(need_count) + content_remove;
            let fraction = if live > 0 {
                content_remove as f64 / live as f64
            } else {
                1.0
            };
            if fraction >= MASS_REMOVE_FRACTION {
                return Err(format!(
                    "Deploy aborted: publishing would remove {} of roughly {} files on \
                     the live site ({:.0}%). A near-total removal almost always means \
                     the build is empty or stale (e.g. a missing hashes.json), not an \
                     intentional deletion. Rebuild the folder and retry. As a last \
                     resort, if the removal is genuinely intended, set \
                     MOSS_DEPLOY_ALLOW_MASS_REMOVE=1 and re-run.",
                    content_remove,
                    live,
                    fraction * 100.0
                ));
            }
        }
    }
    Ok(())
}

/// Diff-taking wrapper — the one `deploy.rs` calls. Owns the field mapping
/// from the wire type so the wiring itself is unit-tested: a regression that
/// stops threading `server_total` (or threads the wrong count) fails the
/// wrapper tests, not just code review. See the "mechanism-not-wiring"
/// lesson from the 2026-07 publish-reliability batch.
pub fn check_mass_removal_for_diff(
    manifest_count: usize,
    diff: &crate::seta::sites::SyncManifestResponse,
    allow_override: bool,
) -> Result<(), String> {
    check_mass_removal(
        manifest_count,
        diff.need.len(),
        &diff.remove,
        diff.server_total.map(|t| t as usize),
        allow_override,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// n distinct user-content page paths.
    fn page_paths(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("posts/page-{i}.html")).collect()
    }

    /// n distinct `_moss/` internal derived-asset paths (OG cards).
    fn og_paths(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("_moss/og/{i:040x}.png")).collect()
    }

    #[test]
    fn mass_removal_blocks_empty_manifest_against_populated_site() {
        // The 2026-06-14 incident exactly: empty manifest, server has 687 →
        // remove all. Blocked with or without server_total.
        let remove = page_paths(687);
        let r = check_mass_removal(0, 0, &remove, Some(687), false);
        assert!(r.is_err(), "empty manifest wiping a populated site must be blocked");
        let msg = r.unwrap_err();
        assert!(msg.contains("687"), "message must report true live count: {msg}");

        let r = check_mass_removal(0, 0, &remove, None, false);
        assert!(r.is_err(), "must also block against an old server (no server_total)");
    }

    #[test]
    fn mass_removal_allows_normal_content_update() {
        // 700-file site: 2 changed, 1 deleted.
        let remove = page_paths(1);
        assert!(check_mass_removal(700, 2, &remove, Some(701), false).is_ok());
        assert!(check_mass_removal(700, 2, &remove, None, false).is_ok());
    }

    #[test]
    fn mass_removal_does_not_trip_tiny_sites() {
        // A tiny site legitimately restructured wholesale: 6 pages dropped,
        // but the new build is still near live size (14 of 20) → allowed.
        let remove = page_paths(6);
        assert!(check_mass_removal(14, 14, &remove, Some(20), false).is_ok());
        // Fallback path (old server): below MIN_ABS never blocks. This pins
        // the documented residual too — a small-site wipe against an OLD
        // server (< MIN_ABS content removals) passes undetected; the
        // server_total path below covers it on current servers.
        assert!(check_mass_removal(0, 0, &page_paths(9), None, false).is_ok());
    }

    #[test]
    fn mass_removal_blocks_small_site_wipe_despite_few_removals() {
        // 9-content-page site (live 167 incl. 158 og cards) hit by an
        // empty/stale build: only 9 content removals — under MIN_ABS — but
        // the collapse is extreme (manifest < live/4). The pre-rework guard
        // blocked this (raw remove = 167); the extreme clause keeps that
        // protection. Both the empty-manifest and chrome-only (11-file)
        // build shapes must block.
        let remove = page_paths(9);
        assert!(check_mass_removal(0, 0, &remove, Some(167), false).is_err());
        assert!(check_mass_removal(11, 0, &remove, Some(167), false).is_err());
    }

    #[test]
    fn mass_removal_min_abs_boundary_is_pinned() {
        // Exactly MIN_ABS (10) content removals with the collapse signature
        // must block on both paths — pins `<` vs `<=` on the constant.
        let remove = page_paths(10);
        assert!(check_mass_removal(0, 0, &remove, Some(366), false).is_err());
        assert!(check_mass_removal(0, 0, &remove, None, false).is_err());
    }

    #[test]
    fn mass_removal_distrusts_inconsistent_server_total() {
        // A server_total smaller than the removal list contradicts itself
        // (removals come from the server's own manifest). Some(0) would make
        // `total / 2 == 0` and disarm the collapse check — the guard must
        // fall through to the derived estimate and still block the
        // 2026-06-14 wipe shape.
        let remove = page_paths(366);
        assert!(check_mass_removal(0, 0, &remove, Some(0), false).is_err());
        // Benign inconsistency on a routine update stays permissive: the
        // fallback's MIN_ABS floor applies.
        assert!(check_mass_removal(700, 2, &page_paths(1), Some(0), false).is_ok());
    }

    #[test]
    fn wrapper_threads_the_diff_fields_through() {
        // Wiring test for check_mass_removal_for_diff: the collapse verdict
        // and its message prove server_total flowed from the wire type (the
        // Some-path message cites the true total; the fallback path says
        // "roughly"). Threading None — or the wrong count — flips these.
        let diff = crate::seta::sites::SyncManifestResponse {
            need: vec![],
            remove: page_paths(355),
            server_total: Some(366),
        };
        let msg = check_mass_removal_for_diff(11, &diff, false).unwrap_err();
        assert!(
            msg.contains("live site has 366"),
            "server_total must reach the guard: {msg}"
        );

        let old_server = crate::seta::sites::SyncManifestResponse {
            need: vec![],
            remove: page_paths(355),
            server_total: None,
        };
        let msg = check_mass_removal_for_diff(11, &old_server, false).unwrap_err();
        assert!(msg.contains("roughly"), "None must select the fallback: {msg}");

        // allow_override threads through the wrapper too.
        assert!(check_mass_removal_for_diff(11, &diff, true).is_ok());
    }

    #[test]
    fn mass_removal_blocks_majority_removal() {
        // Live site of 100 collapsing to a 40-file build, dropping 60 pages:
        // blocked on both paths (40 < 100/2; fallback 60/100 ≥ 50%).
        let remove = page_paths(60);
        assert!(check_mass_removal(40, 0, &remove, Some(100), false).is_err());
        assert!(check_mass_removal(40, 0, &remove, None, false).is_err());
    }

    #[test]
    fn mass_removal_override_bypasses_block() {
        // MOSS_DEPLOY_ALLOW_MASS_REMOVE=1 (threaded in as allow_override) →
        // an intentional purge is permitted on both paths.
        let remove = page_paths(687);
        assert!(check_mass_removal(0, 0, &remove, Some(687), true).is_ok());
        assert!(check_mass_removal(0, 0, &remove, None, true).is_ok());
    }

    #[test]
    fn mass_removal_allows_pure_additions() {
        // First publish / all-new: remove 0 → never blocked regardless of size.
        assert!(check_mass_removal(700, 700, &[], Some(0), false).is_ok());
        assert!(check_mass_removal(700, 700, &[], None, false).is_ok());
    }

    #[test]
    fn mass_removal_allows_og_card_rotation() {
        // The 2026-07-22 incident: home page renamed → site name changed →
        // all 158 content-addressed OG cards rotated filenames. Server diff:
        // manifest 366, need 355 (changed in place), remove 158 all _moss/og/.
        // Old guard derived live = 366 − 355 + 158 = 169 → "remove 158 of 169
        // (93%)" and blocked a healthy publish. The _moss/ filter zeroes the
        // removal signal → Ok, even against an old server that sends the
        // unfiltered remove list.
        let remove = og_paths(158);
        assert!(check_mass_removal(366, 355, &remove, Some(366), false).is_ok());
        assert!(check_mass_removal(366, 355, &remove, None, false).is_ok());
    }

    #[test]
    fn mass_removal_blocks_stale_manifest_with_server_total() {
        // Manifest collapse: an 11-file build against a 366-file live site.
        let remove = page_paths(355);
        let r = check_mass_removal(11, 0, &remove, Some(366), false);
        assert!(r.is_err(), "collapsed build must be blocked");
        let msg = r.unwrap_err();
        assert!(
            msg.contains("only 11 files while the live site has 366"),
            "message must report TRUE numbers from server_total: {msg}"
        );
        assert!(msg.contains("Rebuild the folder and retry"), "keep the remedy: {msg}");
        assert!(msg.contains("MOSS_DEPLOY_ALLOW_MASS_REMOVE"), "keep the override: {msg}");

        // Same collapse against an old server: fallback formula still catches it.
        // live = 11 − 0 + 355 = 366; 355/366 ≈ 97% ≥ 50% → blocked.
        assert!(check_mass_removal(11, 0, &remove, None, false).is_err());
    }

    #[test]
    fn mass_removal_allows_mass_slug_reorg_with_server_total() {
        // Mass slug-reorg: 40 pages moved to new paths (old paths removed, new
        // ones in `need`), but the build is still full-size (366 ≥ 366/2) → Ok.
        let remove = page_paths(40);
        assert!(check_mass_removal(366, 355, &remove, Some(366), false).is_ok());
    }

    #[test]
    fn mass_removal_collapse_threshold_is_half_the_live_site() {
        // Pin the `total / 2` boundary so the threshold can't drift STRICTER
        // unnoticed (a `< total` mutation would block any deploy deleting
        // >= MIN_ABS pages and no other test fails). Deleting a batch of old
        // posts is normal: a 366-file live site pruned to a 351-file build
        // (15 pages removed) must publish without ceremony...
        let remove = page_paths(15);
        assert!(check_mass_removal(351, 0, &remove, Some(366), false).is_ok());

        // ...and the exact boundary: manifest_count == total/2 (183 of 366)
        // is NOT a collapse; one file fewer is. If either assertion fails,
        // the threshold moved — that is a product decision, not a refactor.
        let remove = page_paths(184);
        assert!(check_mass_removal(183, 0, &remove, Some(366), false).is_ok());
        assert!(check_mass_removal(182, 0, &remove, Some(366), false).is_err());
    }

    #[test]
    fn mass_removal_fallback_still_blocks_mass_slug_reorg() {
        // DOCUMENTED LIMITATION (pinned): against an old server (no
        // server_total) the derived denominator deflates on a slug-reorg —
        // live = 366 − 355 + 40 = 51; 40/51 ≈ 78% ≥ 50% → false block. The
        // user unblocks via the env override; the limitation disappears once
        // seta ships server_total. If this assertion starts failing because
        // the fallback got smarter, delete it and celebrate.
        let remove = page_paths(40);
        assert!(check_mass_removal(366, 355, &remove, None, false).is_err());
    }

    #[test]
    fn mass_removal_filters_moss_paths_mixed_with_content() {
        // Defense in depth: an old server sends remove unfiltered. 158 OG
        // rotations + 5 real page deletions → content_remove = 5 < MIN_ABS → Ok.
        let mut remove = og_paths(158);
        remove.extend(page_paths(5));
        assert!(check_mass_removal(366, 355, &remove, None, false).is_ok());

        // But mixed removals with a genuine collapse still block: 158 OG +
        // 355 pages against an 11-file build.
        let mut remove = og_paths(158);
        remove.extend(page_paths(355));
        assert!(check_mass_removal(11, 0, &remove, Some(366), false).is_err());
    }
}
