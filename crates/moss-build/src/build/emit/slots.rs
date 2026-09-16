//! The enhance-hook pass over the stage, plus the manifest re-registration it
//! forces.
//!
//! Slot injection rewrites `<!-- slot:... -->` markers into real content, so
//! every page it touches has different bytes than the render phase registered.
//! This module runs that pass and re-registers the rewritten pages, which is
//! why it owns both halves: a caller that ran one without the other would seal
//! a manifest describing bytes that are no longer on disk.
//!
//! It also runs the `MOSS_INCREMENTAL_VERIFY=1` carry check
//! ([`render::incremental::carry_verify`]) — the check is defined as "final
//! bytes vs final bytes", and this is the first point where the stage holds
//! this build's final bytes.
//!
//! Lifted out of `build::pipeline` (moss#968) where it was a private helper.

use crate::build::outcome::BuildStopped;
use crate::build::manifest::PendingManifest;
use crate::build::render::incremental::CarryVerification;
use crate::build::enhance::ResolvedSlots;
use crate::types::content::SiteResult;
use crate::moss_paths::MossPaths;
use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub fn apply_to_stage_and_manifest(
    paths: &MossPaths,
    stage_dir: &Path,
    slots: &ResolvedSlots,
    pending: &mut PendingManifest,
    site_result: &mut SiteResult,
    // Shadow-verification snapshots (moss#968 §10 gate 4). `Some` only under
    // `MOSS_INCREMENTAL_VERIFY=1`; consumed at the END of this function because
    // that is the first moment the stage holds post-injection bytes.
    carry_verification: Option<CarryVerification>,
) -> Result<(), BuildStopped> {
    // Construction is free of I/O — ObjectStore/TransformCache are just path
    // handles — so building fresh ones here (rather than threading them
    // through from further up) matches the established per-call-site idiom.
    let object_store = crate::build::cache::ObjectStore::new(paths.cache_objects());
    let transform_cache = crate::build::cache::TransformCache::new(
        paths.cache_transforms(),
        crate::build::cache::ObjectStore::new(paths.cache_objects()),
    );
    // Emit the feature stylesheets the resolved slots link to, BEFORE the
    // injection that writes those <link> tags into pages. Slot resolution can
    // name a content-hashed file but has no manifest to write one; this is the
    // first point that has both. See `build::emit::feature_styles`.
    crate::build::emit::feature_styles::emit(slots.feature_styles(), stage_dir, pending)?;

    let changed = crate::build::enhance::inject_slots_into_directory_cached(
        paths.project_root(),
        stage_dir,
        slots,
        &object_store,
        &transform_cache,
    )
    // `with_context`, not `format!`: re-stringifying would discard the deferred
    // verdict. `BuildStopped` has no `Display`, so that mistake cannot compile.
    .map_err(|e| e.with_context("Slot injection failed on site-stage"))?;

    // Registers what the injection pass REPORTS it wrote, never what a read of
    // the stage says is there. The hash arrives in the receipt because the
    // injected bytes existed in memory at write time and nowhere afterwards.
    for (rel_path_str, hash) in changed {
        let sp = crate::build::served_path::ServedPath::from_source(&rel_path_str)
            .map_err(|e| format!("slot-inject registration: invalid path {rel_path_str}: {e}"))?;

        pending.register_hashed(&sp, &hash, crate::build::manifest::HashBucket::Files);
        let Some(entry) = pending.files().get(sp.as_str()).cloned() else {
            continue;
        };

        site_result.hashes.insert_file_hash(&sp, entry);
    }

    // Comparing any earlier compares the render phase's PRE-injection bytes
    // against the previous build's POST-injection bytes, which reports a
    // divergence on literally every page — which is what the first cut did.
    if let Some(verification) = carry_verification.filter(|v| !v.is_empty()) {
        verification.compare_final_bytes(stage_dir);
    }

    Ok(())
}
