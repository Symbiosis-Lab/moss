//! Keeps an image derivative's old address answering once the build stops
//! producing it.
//!
//! A picture a page embeds gets `.webp` derivatives, and a publish serves them.
//! When no page embeds the picture any more the build drops them, though the
//! picture itself is still served. Links to the derivative are stored outside
//! the site, so its address becomes a copy of the picture and a row of
//! `_moss/redirects.json`, written the way `redirects` writes a file redirect.
//!
//! This runs after the seal's orphan prune, the only point that knows which
//! derivatives the final build drops: the redirect table is planned before the
//! image pipeline has said what it produces.

use crate::build::assets::paths::{compute_binary_hash, compute_binary_hash_file};
use crate::build::manifest::{backfill, change_set, published_record, SealedManifest};
use crate::build::feeds::redirect_table::{Row, TableFile};
use crate::build::served_path::{is_public_address, served_address, ServedPath};
use crate::moss_paths::MossPaths;
use std::collections::HashMap;
use std::path::Path;

/// The image outputs a `.webp` could be cut from: `photo.webp` and
/// `photo.w800.webp` both come from `photo.png` / `.jpg` / `.jpeg` (any case);
/// a rung may also come from the webp source `photo.webp`. Empty for a
/// non-`.webp`.
pub(crate) fn webp_sources(path: &str) -> Vec<String> {
    let Some(stem) = path.strip_suffix(".webp") else { return Vec::new() };
    let (base, rung) = match stem.rsplit_once(".w") {
        Some((base, w)) if !w.is_empty() && w.bytes().all(|b| b.is_ascii_digit()) => (base, true),
        _ => (stem, false),
    };
    let mut out: Vec<String> = ["png", "jpg", "jpeg", "PNG", "JPG", "JPEG"].iter().map(|e| format!("{base}.{e}")).collect();
    if rung {
        out.push(format!("{base}.webp"));
    }
    out
}

/// Derivatives the last publish served that `current` no longer has while the
/// folder picture they were cut from (recorded in `asset_source_to_output`) is
/// still served: `(derivative, picture)` output paths, sorted.
pub(crate) fn kept_derivatives(
    baseline: &change_set::PublishedSnapshot,
    current: &HashMap<String, String>,
) -> Vec<(String, String)> {
    let Some(assets) = baseline.asset_source_to_output.as_ref() else { return Vec::new() };
    let pictures: std::collections::HashSet<&str> = assets.values().map(String::as_str).collect();
    let mut kept: Vec<(String, String)> = baseline
        .files
        .keys()
        .filter(|out| !current.contains_key(*out) && is_public_address(out))
        .filter_map(|out| {
            let picture = webp_sources(out).into_iter().find(|c| pictures.contains(c.as_str()) && current.contains_key(c))?;
            Some((out.clone(), picture))
        })
        .collect();
    kept.sort();
    kept
}

/// Write a redirect for each derivative the last publish served and this
/// build dropped, into `stage_dir` and `sealed`. Returns `(derivative,
/// picture)` output paths for the report. An address the build already serves
/// (an author's own `[redirects]` entry included) is not vanished, so it is
/// never overwritten here.
pub(crate) fn keep(mp: &MossPaths, stage_dir: &Path, sealed: &mut SealedManifest) -> Vec<(String, String)> {
    let Some(baseline) = published_record::load_address_baseline(mp, backfill::publish_target(mp).as_deref()) else {
        return Vec::new();
    };
    let kept = kept_derivatives(&baseline, &sealed.files());
    let mut written: Vec<(String, String)> = Vec::new();
    let mut rewrites: HashMap<String, String> = HashMap::new();
    for (from, picture) in kept {
        let staged = stage_dir.join(&picture);
        let source = if staged.is_file() { staged } else { mp.project_root().join(&picture) };
        let dst = stage_dir.join(&from);
        let copied = crate::build::io_utils::copy_output(&source, &dst).map_err(|e| e.to_string());
        match copied.and_then(|_| compute_binary_hash_file(&dst)) {
            Ok(hash) => {
                rewrites.insert(from.clone(), hash);
                written.push((from, picture));
            }
            Err(e) => log::info!("derivative redirect '{from}' not written: {e}"),
        }
    }
    if written.is_empty() {
        return written;
    }
    let table = ServedPath::for_redirects_manifest();
    if let Some(bytes) = with_rows(&stage_dir.join(table.as_str()), &written) {
        if crate::build::io_utils::write_output(&stage_dir.join(table.as_str()), &bytes).is_ok() {
            rewrites.insert(table.as_str().to_string(), compute_binary_hash(&bytes));
        }
    }
    let keys: Vec<String> = rewrites.keys().cloned().collect();
    sealed.apply_post_seal_rewrites(rewrites);
    sealed.stamp_ship_fingerprints(stage_dir, keys.iter().map(String::as_str));
    written
}

/// The staged table with a 301 row added per kept derivative, or `None` when
/// the site ships no table (no deployed address) or it cannot be read.
fn with_rows(path: &Path, kept: &[(String, String)]) -> Option<Vec<u8>> {
    let mut table: TableFile = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    for (from, picture) in kept {
        let from = served_address(from);
        table.redirects.retain(|r| r.from != from);
        table.redirects.push(Row { from, to: served_address(picture), status: 301 });
    }
    table.redirects.sort_by(|a, b| a.from.cmp(&b.from));
    serde_json::to_vec(&table).ok()
}

#[cfg(test)]
#[path = "derivative_redirects_tests.rs"]
mod tests;
