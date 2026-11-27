//! Mass-removal deploy guard: pure decision logic, no I/O.

/// Refuse a publish whose removals would wipe most of the live site: the
/// signature of an empty or stale build manifest, not an intended deletion.
///
/// `live_total` is the number of files the destination serves now. Paths
/// outside the public addresses (`_moss/` derived assets such as OG cards)
/// are content-addressed and rotate en masse on site-wide changes, so they
/// never count as evidence of a wipe.
///
/// Blocks when the new build is under half the live site AND either at least
/// `MIN_ABS` content files go, or the collapse is extreme (under a quarter of
/// the live site), which keeps small sites protected. Site-wide churn and
/// mass slug moves keep the build near live size and pass.
///
/// `MOSS_DEPLOY_ALLOW_MASS_REMOVE` (passed as `allow_override`) bypasses the
/// guard for a genuinely intended large purge.
pub(super) fn check_mass_removal(
    manifest_count: usize,
    remove: &[String],
    live_total: usize,
    allow_override: bool,
) -> Result<(), String> {
    /// Below this many removed content files, only an extreme collapse blocks.
    const MIN_ABS: usize = 10;

    let content_remove = remove.iter().filter(|p| crate::build::served_path::is_public_address(p)).count();
    if content_remove == 0 || allow_override {
        return Ok(());
    }
    // The live site holds at least what it is asked to remove; a smaller
    // total would disarm the `total / 2` check.
    let total = live_total.max(content_remove);
    let suspicious = content_remove >= MIN_ABS || manifest_count * 4 < total;
    if suspicious && manifest_count < total / 2 {
        return Err(wipe_refusal(manifest_count, total, content_remove));
    }
    Ok(())
}

/// Whether a publish may remove most of the live site: `--accept-removals`, or
/// `MOSS_DEPLOY_ALLOW_MASS_REMOVE` for a route without the flag. Every route
/// asks here, so the refusal's advice works on each.
pub(crate) fn removal_accepted(accept_removals: bool) -> bool {
    accept_removals || std::env::var("MOSS_DEPLOY_ALLOW_MASS_REMOVE").is_ok()
}

/// The refusal of a publish that keeps `files` of the live site's `total` and
/// removes `removed`, whether moss or the server found it.
pub(crate) fn wipe_refusal(files: usize, total: usize, removed: usize) -> String {
    format!(
        "Deploy aborted: the new build contains only {files} files while the \
         live site has {total} — publishing it would remove {removed} of them. This \
         almost always means the build is empty or stale (e.g. a missing \
         hashes.json), not an intentional deletion. Rebuild the folder and \
         retry. As a last resort, if you genuinely intend to remove most \
         of the site, set MOSS_DEPLOY_ALLOW_MASS_REMOVE=1 and re-run."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page_paths(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("posts/page-{i}.html")).collect()
    }

    fn og_paths(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("_moss/og/{i:040x}.png")).collect()
    }

    #[test]
    fn mass_removal_blocks_empty_manifest_against_populated_site() {
        let msg = check_mass_removal(0, &page_paths(687), 687, false).unwrap_err();
        assert!(msg.contains("687"), "message must report the live count: {msg}");
    }

    #[test]
    fn mass_removal_allows_normal_content_update() {
        assert!(check_mass_removal(700, &page_paths(1), 701, false).is_ok());
    }

    #[test]
    fn mass_removal_allows_pure_additions() {
        assert!(check_mass_removal(700, &[], 0, false).is_ok());
    }

    #[test]
    fn mass_removal_does_not_trip_tiny_sites() {
        // Restructured wholesale, but the build stays near live size.
        assert!(check_mass_removal(14, &page_paths(6), 20, false).is_ok());
    }

    #[test]
    fn mass_removal_blocks_small_site_wipe_despite_few_removals() {
        // Under MIN_ABS content removals, but the build is under a quarter of
        // the live site: both an empty and a chrome-only build block.
        let remove = page_paths(9);
        assert!(check_mass_removal(0, &remove, 167, false).is_err());
        assert!(check_mass_removal(11, &remove, 167, false).is_err());
    }

    #[test]
    fn mass_removal_min_abs_boundary_is_pinned() {
        assert!(check_mass_removal(0, &page_paths(10), 366, false).is_err());
    }

    #[test]
    fn mass_removal_distrusts_a_total_below_the_removals() {
        assert!(check_mass_removal(0, &page_paths(366), 0, false).is_err());
        assert!(check_mass_removal(700, &page_paths(1), 0, false).is_ok());
    }

    #[test]
    fn mass_removal_blocks_majority_removal() {
        assert!(check_mass_removal(40, &page_paths(60), 100, false).is_err());
    }

    #[test]
    fn mass_removal_override_bypasses_block() {
        assert!(check_mass_removal(0, &page_paths(687), 687, true).is_ok());
    }

    #[test]
    fn mass_removal_allows_og_card_rotation() {
        // A site rename rotates every OG card's filename.
        assert!(check_mass_removal(366, &og_paths(158), 366, false).is_ok());
    }

    #[test]
    fn mass_removal_blocks_stale_manifest() {
        let msg = check_mass_removal(11, &page_paths(355), 366, false).unwrap_err();
        assert!(msg.contains("only 11 files while the live site has 366"), "{msg}");
        assert!(msg.contains("Rebuild the folder and retry"), "keep the remedy: {msg}");
        assert!(msg.contains("MOSS_DEPLOY_ALLOW_MASS_REMOVE"), "keep the override: {msg}");
    }

    #[test]
    fn mass_removal_allows_mass_slug_move() {
        assert!(check_mass_removal(366, &page_paths(40), 366, false).is_ok());
    }

    #[test]
    fn mass_removal_collapse_threshold_is_half_the_live_site() {
        // Pruning old posts publishes without ceremony; a build of exactly
        // half the live site is not a collapse, one file fewer is.
        assert!(check_mass_removal(351, &page_paths(15), 366, false).is_ok());
        let remove = page_paths(184);
        assert!(check_mass_removal(183, &remove, 366, false).is_ok());
        assert!(check_mass_removal(182, &remove, 366, false).is_err());
    }

    #[test]
    fn mass_removal_filters_moss_paths_mixed_with_content() {
        let mut remove = og_paths(158);
        remove.extend(page_paths(5));
        assert!(check_mass_removal(366, &remove, 366, false).is_ok());

        let mut remove = og_paths(158);
        remove.extend(page_paths(355));
        assert!(check_mass_removal(11, &remove, 366, false).is_err());
    }
}
