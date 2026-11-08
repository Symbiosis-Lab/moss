//! Keeps old addresses answering from what the seal serves.
//!
//! Three kinds of address need the final list of outputs, or their bytes,
//! which only the seal has: the redirect table is planned before the image
//! pipeline has said what it produces.
//!
//! - A file redirect to a folder file (`"/old.png" = "/new.png"`) is a copy
//!   of what the build serves at the new address, so a picture's old address
//!   gives the processed picture, not the raw file in the folder.
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

/// What the plan leaves for the seal: the file redirects to folder files, and
/// the folder moves.
#[derive(Debug, Clone, Default)]
pub(crate) struct LateRedirects {
    pub copies: Vec<KeptCopy>,
    pub folder_moves: Vec<FolderMove>,
}

/// A declared file redirect: `from` and `to` are output paths, and `raw` is
/// the target's path in the site folder, which differs from `to` when a
/// folder name has capitals or spaces.
#[derive(Debug, Clone)]
pub(crate) struct KeptCopy {
    pub from: String,
    pub to: String,
    pub raw: String,
}

/// Where a kept address comes from: a declared line, reported when it fails
/// and left out of the report of what moss kept on its own, or one of moss's.
enum Origin<'a> {
    Declared { raw: &'a str },
    Kept,
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

/// The QR codes a folder move keeps, as [`moved_folder_files`] keeps its
/// files: the code of every page under the old folder. The folder's own page
/// is a stub, whose code the redirect table keeps.
pub(crate) fn moved_folder_qr_codes(moves: &[FolderMove], files: &HashMap<String, String>) -> Vec<(String, String)> {
    let under: Vec<FolderMove> = moves.iter().map(|m| FolderMove { from: format!("qr/{}", m.from), to: format!("qr/{}", m.to), old_served: false }).collect();
    moved_folder_files(&under, files)
}

/// Write each kept address into `stage_dir` and `sealed`. An old address two
/// sources reach takes the first: a declared file redirect, then a dropped
/// derivative of a served picture, then a file or QR code under a folder move. Returns
/// the `(old, served)` pairs written that no line names on its own, for the
/// report. An address the build already serves is never overwritten.
pub(crate) fn keep(mp: &MossPaths, stage_dir: &Path, sealed: &mut SealedManifest, late: &LateRedirects) -> Vec<(String, String)> {
    let files = sealed.files().into_owned();
    let moves = &late.folder_moves;
    let folder_files = moved_folder_files(moves, &files);
    let folder_qr = moved_folder_qr_codes(moves, &files);
    // A folder line that keeps nothing is reported here, the first point that can tell.
    for m in moves {
        if !files.keys().any(|k| k.starts_with(&m.to) && is_public_address(k)) {
            log_warn_problem!("redirect '/{}' skipped: '/{}' is not an address this site serves, and no file is served under it", m.from, m.to);
        } else if m.old_served && !folder_files.iter().any(|(old, served)| old.starts_with(&m.from) && served.starts_with(&m.to)) {
            log_warn_problem!("redirect '/{}' skipped: the site already serves that address and every one under it the line would keep", m.from);
        }
    }
    let derivatives = published_record::load_address_baseline(mp, backfill::publish_target(mp).as_deref())
        .map(|baseline| kept_derivatives(&baseline, &files))
        .unwrap_or_default();
    let mut planned: BTreeMap<&str, (&str, Origin)> = BTreeMap::new();
    let copies = late.copies.iter().filter(|c| !files.contains_key(&c.from));
    let copies = copies.map(|c| (c.from.as_str(), c.to.as_str(), Origin::Declared { raw: &c.raw }));
    let own = derivatives.iter().chain(&folder_files).chain(&folder_qr).map(|(from, to)| (from.as_str(), to.as_str(), Origin::Kept));
    for (from, to, origin) in copies.chain(own) {
        planned.entry(from).or_insert((to, origin));
    }
    let mut rewrites: HashMap<String, String> = HashMap::new();
    let mut kept: Vec<(String, String)> = Vec::new();
    for (from, (to, origin)) in planned {
        let raw = match origin {
            Origin::Declared { raw } => Some(raw),
            Origin::Kept => None,
        };
        match (write_kept(mp, stage_dir, from, to, raw), raw) {
            (Ok(hash), raw) => {
                rewrites.insert(from.to_string(), hash);
                if raw.is_none() {
                    kept.push((from.to_string(), to.to_string()));
                }
            }
            (Err(e), Some(_)) => log_warn_problem!("redirect '/{from}': its target could not be read, so no copy was written ({e})"),
            (Err(e), None) => log::info!("kept address '{from}' not written: {e}"),
        }
    }
    if !rewrites.is_empty() {
        let keys: Vec<String> = rewrites.keys().cloned().collect();
        sealed.apply_post_seal_rewrites(rewrites);
        sealed.stamp_ship_fingerprints(stage_dir, keys.iter().map(String::as_str));
    }
    kept
}

/// One old address: a forwarding page for an `.html` output, else a copy of
/// the served file (the site folder's own when the stage has none, as for a
/// picture copied in as it is, found at `to` or else at `raw`). Returns the
/// hash of what was written.
fn write_kept(mp: &MossPaths, stage_dir: &Path, from: &str, to: &str, raw: Option<&str>) -> Result<String, String> {
    let dst = stage_dir.join(from);
    if from.ends_with(".html") {
        let html = super::redirects::generate_redirect_html(&served_address(to));
        crate::build::io_utils::write_output(&dst, html.as_bytes()).map_err(|e| e.to_string())?;
        return Ok(compute_binary_hash(html.as_bytes()));
    }
    let in_folder = |rel: &str| Some(mp.project_root().join(rel)).filter(|p| p.is_file());
    let staged = Some(stage_dir.join(to)).filter(|p| p.is_file());
    let source = staged.or_else(|| in_folder(to)).or_else(|| raw.and_then(in_folder)).unwrap_or_else(|| mp.project_root().join(to));
    crate::build::io_utils::copy_output(&source, &dst).map_err(|e| e.to_string())?;
    compute_binary_hash_file(&dst)
}

#[cfg(test)]
#[path = "kept_addresses_tests.rs"]
mod tests;
