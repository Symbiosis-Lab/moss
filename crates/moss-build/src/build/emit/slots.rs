//! The enhance-hook pass over the stage, plus the manifest re-registration it
//! forces.
//!
//! Slot injection rewrites `<!-- slot:... -->` markers into real content, so
//! every page it touches has different bytes than the render phase registered.
//! This module runs that pass and hands its receipts to the manifest — the
//! final hash, and the CAS object `ship_phase` reads the page from — which is
//! why it owns both halves: a caller that ran one without the other would seal
//! a manifest describing bytes that are no longer on disk.
//!
//! It also runs the `MOSS_INCREMENTAL_VERIFY=1` carry check
//! ([`render::incremental::carry_verify`]) — the check is defined as "final
//! bytes vs final bytes", and this is the first point where the stage holds
//! this build's final bytes.
//!
//! Lifted out of `build::pipeline` where it was a private helper.

use crate::build::outcome::BuildStopped;
use crate::build::manifest::PendingManifest;
use crate::build::render::incremental::CarryVerification;
use crate::build::enhance::ResolvedSlots;
use crate::moss_paths::MossPaths;
use std::path::Path;

#[cfg(test)]
thread_local! {
    /// Pages the latest slot pass on this thread read and hashed.
    pub(crate) static LAST_SCANNED: std::cell::Cell<usize> = const { std::cell::Cell::new(usize::MAX) };
}

/// Every registered page in the stage is one slot injection would leave as it
/// is. The pass fills only registered pages, so a page registered after it,
/// over bytes already staged, would ship with its slots empty.
#[cfg(debug_assertions)]
pub(crate) fn assert_every_page_filled(stage_dir: &Path, slots: &ResolvedSlots, pending: &PendingManifest) {
    let unfilled: Vec<&String> = pending
        .files()
        .keys()
        .filter(|page| crate::build::enhance::takes_slots(page))
        .filter(|page| {
            let Ok(html) = std::fs::read_to_string(stage_dir.join(page.as_str())) else { return false };
            crate::build::enhance::inject_slots(&html, slots, page) != html
        })
        .collect();
    assert!(unfilled.is_empty(), "registered after the slot pass, so never filled: {unfilled:?}");
}

#[allow(clippy::too_many_arguments)]
pub fn apply_to_stage_and_manifest(
    paths: &MossPaths,
    stage_dir: &Path,
    slots: &ResolvedSlots,
    pending: &mut PendingManifest,
    // Shadow-verification snapshots. `Some` only under
    // `MOSS_INCREMENTAL_VERIFY=1`; consumed at the END of this function because
    // that is the first moment the stage holds post-injection bytes.
    carry_verification: Option<CarryVerification>,
) -> Result<(), BuildStopped> {
    // A carried page holds the previous pass's output only if that pass ran with
    // these same slots and shipping transform; otherwise every page is re-read.
    let slots_hash = crate::build::enhance::resolved_slots_hash(slots);
    let mount = pending.mount();
    let key = format!("{slots_hash}:{}", crate::build::ship::shipped_key(&mount));
    let mut record = PassRecord::load(paths);
    let same_pass = record.key == key;
    // Until this pass ends, the stage holds no one render's pages.
    if record.render.take().is_some() {
        record.save(paths);
    }
    let object_store = crate::build::cache::ObjectStore::for_slot_pass(paths);
    let transform_cache = crate::build::cache::TransformCache::for_slot_pass(paths, &object_store);
    // Emit the feature stylesheets the resolved slots link to, BEFORE the
    // injection that writes those <link> tags into pages. Slot resolution can
    // name a content-hashed file but has no manifest to write one; this is the
    // first point that has both. See `build::emit::feature_styles`.
    crate::build::emit::feature_styles::emit(slots.feature_styles(), stage_dir, pending)?;

    // The record of which blob each staged page was linked from, so a page
    // whose stage file already holds its final bytes is not written again.
    let mut staged = crate::build::lifecycle::cas_heal::StagedLinks::load(paths, "pages", stage_dir);
    let carried = if same_pass { pending.carried_pages() } else { Default::default() };
    let registered = pending.files().keys().filter(|page| crate::build::enhance::takes_slots(page)).cloned().collect();
    let pass = crate::build::enhance::inject_slots_into_directory_cached(
        paths.project_root(),
        stage_dir,
        slots,
        &slots_hash,
        &mount,
        &object_store,
        &transform_cache,
        pending.take_unwritten_pages(),
        registered,
        &carried,
        &mut staged,
    )
    // `with_context`, not `format!`: re-stringifying would discard the deferred
    // verdict. `BuildStopped` has no `Display`, so that mistake cannot compile.
    .map_err(|e| e.with_context("Slot injection failed on site-stage"))?;
    // Before `staged.save()`: a crash between the two must leave links that no
    // longer vouch, never a record claiming slots the staged pages were not filled with.
    if !same_pass {
        record.key = key;
        record.save(paths);
    }
    staged.save();
    if let Err(e) = object_store.save_verified_keeping_unchecked() {
        log::info!("[slots] could not save the verified-blob record: {e}");
    }
    crate::build::cache::page_sources::save(paths, pass.sources, carried.keys().map(String::as_str));
    #[cfg(test)]
    LAST_SCANNED.with(|n| n.set(pass.scanned));

    // Takes what the injection pass REPORTS, never what a read of the stage says
    // is there. The hash and the CAS object arrive in the receipt because the
    // bytes existed in memory when the pass held them and nowhere afterwards.
    //
    // The pass reports on every `.html` in the stage, including pages an earlier
    // build left there, so a receipt is not proof that this build made the page.
    // `attach_final_bytes` gives the final bytes only to pages the render phase
    // already registered, and drops the rest — see it for what a stale page
    // would otherwise do. Nothing that used to be registered here is lost: a page
    // keeps its markers only until the first pass that fills them, so the pages
    // the pass rewrites are the ones this build just rendered.
    for receipt in pass.receipts {
        // A stage path no page of this build could have (`_moss/...` bundles a
        // search settle laid down last time) is simply not one.
        let Ok(sp) = crate::build::served_path::ServedPath::from_output(&receipt.page_path) else {
            continue;
        };
        pending.attach_final_bytes(&sp, &receipt.manifest_hash, receipt.content_oid);
    }
    // The build that runs again finds the page missing and renders it.
    if let Some(page) = pending.rereads_left().next() {
        return Err(BuildStopped::discarded(format!("{page} left the stage before the slot pass read it")));
    }
    record.render = pending.render_id().map(str::to_string);
    record.save(paths);

    // Comparing any earlier compares the render phase's PRE-injection bytes
    // against the previous build's POST-injection bytes, which reports a
    // divergence on literally every page — which is what the first cut did.
    if let Some(verification) = carry_verification.filter(|v| !v.is_empty()) {
        verification.compare_final_bytes(stage_dir);
    }

    Ok(())
}

/// What the last slot pass left on the stage, per machine.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct PassRecord {
    /// The slots and shipping transform the pass filled pages with.
    key: String,
    /// The render whose final bytes every stage page holds: set when a pass
    /// ends, cleared before the next one first writes to the stage.
    render: Option<String>,
}

impl PassRecord {
    fn path(paths: &MossPaths) -> std::path::PathBuf {
        paths.cache_dir().join("slot-pass.json")
    }

    fn load(paths: &MossPaths) -> Self {
        std::fs::read(Self::path(paths))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn save(&self, paths: &MossPaths) {
        if let Ok(json) = serde_json::to_vec(self) {
            let _ = std::fs::write(Self::path(paths), json); // allow:raw_write per-machine cache record, never staging
        }
    }
}

/// The render the last slot pass wrote the stage for, if it ended and no pass
/// has started since.
pub(crate) fn staged_render(paths: &MossPaths) -> Option<String> {
    PassRecord::load(paths).render
}

/// Write rendered pages to the stage as rendered, slot markers and all: what
/// an entry point with no slot pass after it leaves.
pub(crate) fn write_as_rendered(
    stage_dir: &Path,
    pages: std::collections::BTreeMap<String, String>,
) -> Result<(), BuildStopped> {
    for (page_path, html) in pages {
        let path = stage_dir.join(&page_path);
        crate::build::io_utils::write_output_if_changed(&path, html.as_bytes())
            .map_err(|e| format!("Failed to write HTML file {}: {}", path.display(), e))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "slots_tests.rs"]
mod tests;
