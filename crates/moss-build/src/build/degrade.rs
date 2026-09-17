//! Post-seal, pre-publish HTML degradation pass (moss#867).
//!
//! Glues the pure [`crate::build::markdown::html_post::degrade_failed_variants`]
//! filter to disk: for every HTML page the sealed manifest tracked as a
//! blocking artifact, read the staging bytes, run the filter, and if
//! anything changed, write the new bytes back to `stage_dir` and record the
//! new hash for [`SealedManifest::apply_post_seal_rewrites`].
//!
//! [`repair_staged_html`] is the ordered whole that pass is the last step of:
//! the removal passes live in `ship`, the repair lives here, and the sequence
//! is owned here because `degrade` already depends on `ship` and not the
//! reverse.

use crate::build::manifest::SealedManifest;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Everything that can remove a variant, then the HTML repair that must follow
/// it. The whole post-seal tail of `advertise_sealed`, in one synchronous call
/// so that dropping a pass from it is something a test can see.
///
/// `failed` is every variant URL the AssetRegistry already knows will not
/// exist (a terminally-failed encode, moss#867); the caller owns registry
/// access, so this function needs no `BuildServices`. Returned is whether the
/// generation may ship at all.
///
/// The four sources that can leave a `<source>` pointing at nothing: a failed
/// encode (moss#867), the orphan prune (moss#976 B2), the presence pass, and a
/// reference with no manifest entry at all. This is the only scope that sees
/// all four (ADR-013 amendment 2026-09-09) — and the only one that can hand
/// them ONE reference scan, which their disjointness depends on: the prune acts
/// on what that scan did NOT see, the fourth source on what it did. Hoisting
/// the scan out of the prune also keeps the fourth source alive under
/// `[build].prune_orphaned_images = false`, an opt-out about upload bytes.
///
/// The order is load-bearing. Every removal pass runs before
/// [`apply_to_staging`], which repairs the HTML LAST, after everything that can
/// remove a variant: until 2026-09-09 it ran first, so every removal shipped a
/// live 404 that `<picture>` renders blank instead of falling back. The caller
/// must in turn run this before it persists or materializes the generation,
/// which would otherwise advertise a manifest entry with no file behind it
/// (deploy refuses the whole upload over one).
///
/// "Remove" here means remove from the MANIFEST. The preview server is reading
/// `stage_dir` while this runs — the seal tail is detached, and nothing points
/// the server away from staging until the next build starts — so the only write
/// this whole sequence makes into it is [`apply_to_staging`]'s, which goes
/// through `io_utils::write_output` and is therefore a rename, never a window
/// where the page is absent. The staged bytes of an unshipped variant are
/// unlinked by `build::pipeline`'s pre-render sweep instead.
///
/// The ship verdict is decided after the presence pass and BEFORE the repair:
/// a generation withheld because its tree could not be read must not also
/// rewrite the HTML the preview is serving, from a strip set that unreadable
/// tree produced.
pub(crate) fn repair_staged_html(
    mp: &crate::moss_paths::MossPaths,
    stage_dir: &Path,
    sealed: &mut SealedManifest,
    failed: HashSet<String>,
) -> crate::build::ship::ShipVerdict {
    let mut unshippable = failed;
    let scan = crate::build::media::orphan_prune::extract_referenced_tails(stage_dir);
    unshippable.extend(crate::build::ship::prune_orphaned_webp_before_ship(mp, sealed, &scan));
    let entries = sealed.files().len();
    let lost = crate::build::ship::drop_absent_outputs(stage_dir, sealed);
    let verdict = crate::build::ship::ShipVerdict::after_presence_pass(sealed, entries, lost.len());
    unshippable.extend(lost);
    unshippable.extend(crate::build::ship::unregistered_referenced_variants(
        &scan, sealed, stage_dir,
    ));
    if verdict.repairs_staging() {
        apply_to_staging(stage_dir, sealed, &unshippable);
    }
    verdict
}

/// Rewrite every sealed HTML page in `stage_dir` to drop references to image
/// variants that will not exist on the published site, then fold the
/// resulting hash changes into `sealed` (updating `generation_id`).
///
/// `failed` is every site-root-relative URL that must not survive in HTML,
/// whatever removed it. This function does not care which — a `<picture>`
/// cannot fall back from a chosen-source 404 regardless of who deleted the
/// file, so every source feeds one strip set (ADR-013 amendment 2026-09-09).
/// Taking a set rather than the `AssetRegistry` is what lets the removal
/// passes downstream of the encoder participate at all.
///
/// No-op, with no filesystem access, when `failed` is empty (the common case).
/// [`repair_staged_html`] is what assembles the set and owns the ordering this
/// pass depends on.
pub fn apply_to_staging(
    stage_dir: &Path,
    sealed: &mut SealedManifest,
    failed: &std::collections::HashSet<String>,
) {
    if failed.is_empty() {
        return;
    }
    let mut rewrites: HashMap<String, String> = HashMap::new();

    for path in sealed.blocking_keys() {
        if !path.ends_with(".html") {
            continue;
        }
        let full = stage_dir.join(path);
        let bytes = match std::fs::read(&full) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("degrade_failed_variants: failed to read {}: {}", full.display(), e);
                continue;
            }
        };
        let html = match std::str::from_utf8(&bytes) {
            Ok(s) => s,
            Err(_) => continue, // never expected for moss-generated HTML; skip defensively
        };
        let rewritten =
            match crate::build::markdown::html_post::degrade_failed_variants(html, path, failed) {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("degrade_failed_variants: rewrite failed for {}: {}", path, e);
                    continue;
                }
            };
        if let std::borrow::Cow::Owned(new_html) = rewritten {
            if let Err(e) = crate::build::io_utils::write_output(&full, new_html.as_bytes()) {
                log::warn!("degrade_failed_variants: failed to write {}: {}", full.display(), e);
                continue;
            }
            // The manifest records the bytes the SITE will serve, not the
            // staged ones: staging keeps the preview annotations that
            // `ship::apply_transform` strips on the way into the generation.
            // Hashing the staged bytes here made the manifest describe a file
            // that would never exist, and deploy's integrity check then
            // rejected the whole upload — every page this pass rewrote, on any
            // vault holding a CMYK image (its variants settle as `Failed`, so
            // this pass runs at all). Same computation as `enhance.rs`'s
            // registration site, and for the same reason.
            let hash = crate::build::assets::paths::compute_binary_hash(
                &crate::build::ship::apply_transform(
                    crate::build::ship::transform_for(path),
                    new_html.as_bytes(),
                ),
            );
            rewrites.insert(path.clone(), hash);
        }
    }

    if !rewrites.is_empty() {
        log::info!(
            "degrade_failed_variants: rewrote {} page(s) to drop failed image variant(s)",
            rewrites.len()
        );
        sealed.apply_post_seal_rewrites(rewrites);
    }
}

#[cfg(test)]
#[path = "degrade_tests.rs"]
mod tests;
