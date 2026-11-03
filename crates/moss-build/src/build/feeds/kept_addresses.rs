//! Keeps old addresses answering from what the seal serves.
//!
//! Two kinds of address need the final list of outputs, which only the seal
//! has: the redirect table is planned before the image pipeline has said what
//! it produces.
//!
//! - A picture a page embeds gets `.webp` derivatives, and a publish serves
//!   them. When no page embeds the picture any more the build drops them,
//!   though the picture itself is still served. Links to the derivative are
//!   stored outside the site, so its address becomes a copy of the picture.
//! - A folder redirect (`"/old/" = "/new/"` under `[redirects]`) covers every
//!   address under the old folder: each output served under the new folder
//!   gets a forwarding page (an `.html` file) or a copy of its served bytes at
//!   the same path under the old one.
//!
//! The next build's stale-page sweep removes these forwarding pages from the
//! stage before this pass writes them again, so a preview can miss them for
//! that moment; a publish always ships the sealed set.

use crate::build::assets::paths::{compute_binary_hash, compute_binary_hash_file};
use crate::build::cli_output::log_warn_problem;
use crate::build::feeds::redirect_table::FolderMove;
use crate::build::manifest::{backfill, change_set, published_record, SealedManifest};
use crate::build::served_path::{is_public_address, served_address};
use crate::moss_paths::MossPaths;
use std::collections::{BTreeMap, HashMap};
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

/// `(old, served)` output paths for every public output under a moved
/// folder's new address whose counterpart under the old one `files` does not
/// serve. The most specific move wins an address two of them reach, and an
/// output already under a move's old folder is not aliased again (a folder
/// moved up into its parent would otherwise nest copies of itself).
pub(crate) fn moved_folder_files(moves: &[FolderMove], files: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut by_specificity: Vec<&FolderMove> = moves.iter().collect();
    by_specificity.sort_by_key(|m| std::cmp::Reverse(m.from.len()));
    let mut kept: BTreeMap<String, String> = BTreeMap::new();
    for m in by_specificity {
        let nests = m.from.starts_with(&m.to);
        for served in files.keys().filter(|k| k.starts_with(&m.to) && !(nests && k.starts_with(&m.from)) && is_public_address(k)) {
            let old = format!("{}{}", m.from, &served[m.to.len()..]);
            if !files.contains_key(&old) {
                kept.entry(old).or_insert_with(|| served.clone());
            }
        }
    }
    kept.into_iter().collect()
}

/// Write each kept address into `stage_dir` and `sealed`: the dropped
/// derivatives of a served picture, and the files under each of `moves`.
/// Returns the `(old, served)` pairs written, for the report. An address the
/// build already serves is never overwritten.
pub(crate) fn keep(mp: &MossPaths, stage_dir: &Path, sealed: &mut SealedManifest, moves: &[FolderMove]) -> Vec<(String, String)> {
    let files = sealed.files().into_owned();
    let folder_files = moved_folder_files(moves, &files);
    // A folder line that keeps nothing is reported here, the first point that can tell.
    for m in moves {
        if !files.keys().any(|k| k.starts_with(&m.to) && is_public_address(k)) {
            log_warn_problem!("redirect '/{}' skipped: '/{}' is not an address this site serves, and no file is served under it", m.from, m.to);
        } else if m.old_served && !folder_files.iter().any(|(old, served)| old.starts_with(&m.from) && served.starts_with(&m.to)) {
            log_warn_problem!("redirect '/{}' skipped: the site already serves that address and every one under it the line would keep", m.from);
        }
    }
    let mut kept = published_record::load_address_baseline(mp, backfill::publish_target(mp).as_deref())
        .map(|baseline| kept_derivatives(&baseline, &files))
        .unwrap_or_default();
    kept.extend(folder_files);
    let mut rewrites: HashMap<String, String> = HashMap::new();
    kept.retain(|(from, to)| match write_kept(mp, stage_dir, from, to) {
        Ok(hash) => rewrites.insert(from.clone(), hash).is_none(),
        Err(e) => {
            log::info!("kept address '{from}' not written: {e}");
            false
        }
    });
    if !rewrites.is_empty() {
        let keys: Vec<String> = rewrites.keys().cloned().collect();
        sealed.apply_post_seal_rewrites(rewrites);
        sealed.stamp_ship_fingerprints(stage_dir, keys.iter().map(String::as_str));
    }
    kept
}

/// One old address: a forwarding page for an `.html` output, else a copy of
/// the served file (the site folder's own when the stage has none, as for a
/// picture copied in as it is). Returns the hash of what was written.
fn write_kept(mp: &MossPaths, stage_dir: &Path, from: &str, to: &str) -> Result<String, String> {
    let dst = stage_dir.join(from);
    if from.ends_with(".html") {
        let html = super::redirects::generate_redirect_html(&served_address(to));
        crate::build::io_utils::write_output(&dst, html.as_bytes()).map_err(|e| e.to_string())?;
        return Ok(compute_binary_hash(html.as_bytes()));
    }
    let staged = stage_dir.join(to);
    let source = if staged.is_file() { staged } else { mp.project_root().join(to) };
    crate::build::io_utils::copy_output(&source, &dst).map_err(|e| e.to_string())?;
    compute_binary_hash_file(&dst)
}

#[cfg(test)]
#[path = "kept_addresses_tests.rs"]
mod tests;
