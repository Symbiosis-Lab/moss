//! The preview's delta: the pages edited since the held bundle, indexed on
//! their own and merged by the search panel.
//!
//! Pagefind has no way to fold pages into a built bundle, so a search read in
//! the preview indexes only the pages that differ from the held bundle's, and
//! names the held fragments they replace or remove. The delta is cumulative
//! against one base, served from its own mount, never laid into staging, and
//! cleared by every whole index and by an undone edit. It scores with its own
//! statistics, which places an edited page a little differently than a whole
//! index would; a publish always ships a whole one.

use super::*;

/// The delta's holding directory and receipt, beside the held bundle's.
/// [`gc_holding`] removes the directory with every other one, so a whole
/// index leaves no delta behind.
pub(super) const DELTA_DIR: &str = "delta";
const DELTA_RECEIPT: &str = "delta.json";

/// The held bundle's pages, written beside its receipt by every whole index
/// and read only here.
const PAGES_FILE: &str = "pages.json";

/// The file in the delta's mount that tells the search panel what to merge:
/// whether there is a bundle, and the fragment ids of the held bundle's pages
/// the delta replaces or removes.
pub const DELTA_MANIFEST: &str = "moss-delta.json";

/// A delta past this share of the held bundle's pages takes a whole index,
/// which becomes the new base.
const DELTA_MAX_SHARE: usize = 4;

/// What the last delta published, against which held bundle.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct DeltaReceipt {
    /// Hex [`PageSetFp`] of the page set the held bundle and this delta cover.
    pub(super) fp: String,
    /// Hex fingerprint of the held bundle this delta is a delta of.
    pub(super) base: String,
    /// Paths within the delta's mount, the manifest among them.
    pub(super) files: Vec<String>,
}

/// The pages of the held bundle `fp`: where each was read from, the hex
/// content key of its bytes and its URL. The URL → fragment id map is filled
/// by the first delta against this bundle, since only a delta needs it and it
/// costs a read of every fragment.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct HeldPages {
    fp: String,
    pages: Vec<(String, String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fragments: Option<HashMap<String, String>>,
}

#[derive(Serialize)]
struct DeltaManifest<'a> {
    bundle: bool,
    supersede: &'a [String],
}

fn page_key(key: u128) -> String {
    format!("{key:032x}")
}

/// Write the pages a whole index just published as bundle `fp`.
pub(super) fn record_pages(index_dir: &Path, fp: &str, indexed: &[search::IndexedPage]) {
    let pages = indexed.iter().map(|p| (p.rel_path.clone(), page_key(p.key), p.url.clone())).collect();
    write_pages(index_dir, &HeldPages { fp: fp.to_string(), pages, fragments: None });
}

fn write_pages(index_dir: &Path, held: &HeldPages) {
    if let Ok(json) = serde_json::to_vec(held) {
        let _ = crate::build::io_utils::write_output(&index_dir.join(PAGES_FILE), &json);
    }
}

/// The held bundle's pages with each one's fragment id by its path, mapping
/// the fragments on the first call against this bundle. `None` when the file
/// is not this bundle's or a page has no fragment.
pub(super) fn held_fragments(_holding: &Holding, index_dir: &Path, base: &BundleReceipt) -> Option<Vec<(String, String, String)>> {
    let mut held: HeldPages = serde_json::from_slice(&std::fs::read(index_dir.join(PAGES_FILE)).ok()?).ok()?;
    if held.fp != base.fp || held.pages.is_empty() {
        return None;
    }
    if held.fragments.is_none() {
        let dir = holding_dir(index_dir, base.fp());
        let files: Vec<search::SearchIndexFile> = base
            .files
            .iter()
            .filter(|(rel, _)| rel.starts_with("fragment/"))
            .filter_map(|(rel, _)| Some(search::SearchIndexFile { rel_path: rel.clone(), bytes: std::fs::read(dir.join(rel)).ok()? }))
            .collect();
        held.fragments = Some(search::fragment_ids(&files));
        write_pages(index_dir, &held);
    }
    let fragments = held.fragments.as_ref()?;
    held.pages.iter().map(|(rel, key, url)| Some((rel.clone(), key.clone(), fragments.get(url)?.clone()))).collect()
}

pub(super) fn read_delta(index_dir: &Path) -> Option<DeltaReceipt> {
    serde_json::from_slice(&std::fs::read(index_dir.join(DELTA_RECEIPT)).ok()?).ok()
}

pub(super) fn clear_delta(_holding: &Holding, index_dir: &Path) {
    // allow:unlink the search index under .moss/build.nosync/index, not staging
    let _ = std::fs::remove_file(index_dir.join(DELTA_RECEIPT));
    // allow:unlink the search index under .moss/build.nosync/index, not staging
    let _ = crate::build::io_utils::remove_output_dir_all(&index_dir.join(DELTA_DIR));
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum DeltaOutcome {
    /// The delta covers the page set, or a transient failure left the last one.
    Done,
    /// The preview needs a whole index instead.
    Whole,
}

/// Index the pages of `generation` that `base` does not hold as they are now,
/// and publish them with the fragment ids of the base pages they replace or
/// remove.
pub(super) fn index_delta(mp: &MossPaths, generation: &Path, want: PageSet, base: &BundleReceipt) -> DeltaOutcome {
    let index_dir = mp.index_dir();
    let base_fp = base.fp.clone();
    let held = with_holding(&index_dir, |h| {
        let covered = read_delta(&index_dir).is_some_and(|delta| {
            let dir = index_dir.join(DELTA_DIR);
            delta.fp == want.fp.hex() && delta.base == base_fp && delta.files.iter().all(|rel| dir.join(rel).is_file())
        });
        if covered { Err(DeltaOutcome::Done) } else { held_fragments(h, &index_dir, base).ok_or(DeltaOutcome::Whole) }
    });
    let held = match held {
        Ok(held) => held,
        Err(outcome) => return outcome,
    };

    let _phase = crate::build::phase::PhaseTrace::start("search_delta");
    let t_read = std::time::Instant::now();
    let read = search::read_pages(generation);
    if resolve_current(mp) != generation {
        log::info!(target: "search", "search delta not generated: a promote moved current during the read");
        return DeltaOutcome::Done;
    }
    if read.unreadable > 0 || read.pages.len() + read.left_out < want.pages {
        return DeltaOutcome::Whole;
    }
    let keys: HashMap<&str, &str> = held.iter().map(|(rel, key, _)| (rel.as_str(), key.as_str())).collect();
    let (unchanged, changed): (Vec<_>, Vec<_>) =
        read.pages.into_iter().partition(|page| keys.get(page.rel_path.as_str()) == Some(&page_key(page.key).as_str()));
    let unchanged: HashSet<&str> = unchanged.iter().map(|page| page.rel_path.as_str()).collect();
    let supersede: Vec<String> =
        held.iter().filter(|(rel, ..)| !unchanged.contains(rel.as_str())).map(|(.., fragment)| fragment.clone()).collect();
    if changed.len().max(supersede.len()) * DELTA_MAX_SHARE > held.len() {
        return DeltaOutcome::Whole;
    }

    let changed_count = changed.len();
    let produced = if changed.is_empty() {
        search::SearchIndex::default()
    } else {
        match search::index_pages(search::PageRead { pages: changed, unreadable: 0, left_out: 0 }, t_read, search::Scope::Delta) {
            Ok(produced) if produced.skipped == 0 && produced.pages == changed_count => produced,
            _ => return DeltaOutcome::Whole,
        }
    };
    crate::build::phase::record_count("search_delta_pages", changed_count);

    with_holding(&index_dir, |h| {
        if read_receipt(&index_dir).map(|r| r.fp) != Some(base_fp.clone()) {
            return DeltaOutcome::Done;
        }
        if let Err(e) = publish_delta(h, &index_dir, want, &base_fp, &produced, &supersede) {
            log::info!(target: "search", "search delta not generated: {}", e);
        }
        DeltaOutcome::Done
    })
}

fn publish_delta(
    holding: &Holding,
    index_dir: &Path,
    want: PageSet,
    base: &str,
    produced: &search::SearchIndex,
    supersede: &[String],
) -> Result<(), String> {
    clear_delta(holding, index_dir);
    let dir = index_dir.join(DELTA_DIR);
    let mut files = Vec::with_capacity(produced.files.len() + 1);
    for file in &produced.files {
        ServedPath::for_search_asset(&file.rel_path).map_err(|e| format!("bad search bundle path {:?}: {:?}", file.rel_path, e))?;
        crate::build::io_utils::write_output(&dir.join(&file.rel_path), &file.bytes)
            .map_err(|e| format!("failed to write search delta file {:?}: {}", file.rel_path, e))?;
        files.push(file.rel_path.clone());
    }
    let manifest = serde_json::to_vec(&DeltaManifest { bundle: !produced.files.is_empty(), supersede })
        .map_err(|e| format!("failed to serialize search delta manifest: {}", e))?;
    crate::build::io_utils::write_output(&dir.join(DELTA_MANIFEST), &manifest)
        .map_err(|e| format!("failed to write search delta manifest: {}", e))?;
    files.push(DELTA_MANIFEST.to_string());
    let receipt = DeltaReceipt { fp: want.fp.hex(), base: base.to_string(), files };
    let json = serde_json::to_vec(&receipt).map_err(|e| format!("failed to serialize search delta receipt: {}", e))?;
    crate::build::io_utils::write_output(&index_dir.join(DELTA_RECEIPT), &json)
        .map_err(|e| format!("failed to write search delta receipt: {}", e))
}

/// The file at `rel` (a path under the delta's mount) in the delta against
/// the held bundle, if there is one.
pub fn held_delta_file(mp: &MossPaths, rel: &str) -> Option<PathBuf> {
    let index_dir = mp.index_dir();
    let delta = read_delta(&index_dir)?;
    let base = read_receipt(&index_dir)?;
    (delta.base == base.fp && delta.files.iter().any(|named| named == rel)).then(|| index_dir.join(DELTA_DIR).join(rel))
}
