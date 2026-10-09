//! URL redirect management: the rename history, and the one writer of every
//! redirect stub and copy.
//!
//! When a user renames a page (changing its URL slug), old URLs should redirect
//! to new ones via HTML meta-refresh files. This module detects renames,
//! manages the persistent redirect map, and writes the build's redirects from
//! the merged table (`redirect_table`: renames, `[redirects]`, the generator's
//! former paths).
//!
//! ## Workflow
//!
//! 1. Compare previous and current `ArticleMap` to detect renames via stable UIDs
//! 2. Merge new renames into the persistent redirect map (with chain resolution)
//! 3. Merge in the other two sources, plan each entry's static form, and write
//!    the stubs and copies plus `_moss/redirects.json` — skipping the stub
//!    writes when the stub set is identical to what the last build of this
//!    folder in this process already wrote

use crate::build::assets::paths::compute_binary_hash;
use crate::build::manifest::live_baseline::{self, Baseline, LiveBaseline, Unreadable};
use crate::build::scan::article_map::ArticleMap;
#[cfg(test)]
use crate::types::content::file_entry;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// Detect URL renames by comparing article UIDs between what is live and what
/// this build produced.
///
/// For each UID present in both at different pretty-URLs, records
/// `old_url → new_url`. Articles without a UID are skipped.
///
/// The baseline is a [`LiveBaseline`] rather than a whole `ArticleMap` because
/// uid and URL are all this ever read of it — see
/// [`crate::build::manifest::live_baseline`] for what the other 300× was.
pub fn detect_renames(
    previous: &LiveBaseline,
    current_map: &ArticleMap,
) -> HashMap<String, String> {
    let prev_uid_to_url = previous.urls_by_uid();

    let curr_uid_to_url: HashMap<&str, &str> = current_map
        .articles
        .iter()
        .filter_map(|(url, info)| info.uid.as_deref().map(|uid| (uid, url.as_str())))
        .collect();

    let mut renames = HashMap::new();

    for (uid, old_url) in &prev_uid_to_url {
        if let Some(new_url) = curr_uid_to_url.get(uid) {
            if old_url != new_url {
                renames.insert(old_url.to_string(), new_url.to_string());
            }
        }
    }

    renames
}

/// Every URL a real page — article or folder index — will serve in this
/// build, in the pretty-URL format this module trades in ("foo/bar/", no
/// leading slash).
///
/// The one set two different stub sources must both check before emitting: a
/// detected rename here, or a term namespace-move stub from
/// `terms::kind_move_stubs`. Skipping the check silently buries a real page,
/// because the manifest is last-write-wins and a stub emitted after the
/// page's own write simply replaces it with a meta-refresh. `pages` matters
/// here as much as `articles`: a namespace ROOT stub (e.g. `authors/`) lands
/// squarely on the URL a hand-authored folder index uses.
pub fn current_build_urls(current_article_map: &ArticleMap) -> HashSet<String> {
    current_article_map
        .articles
        .keys()
        .chain(current_article_map.pages.keys())
        .cloned()
        .collect()
}

/// Merge new renames into an existing redirect map.
///
/// - Starts with existing redirects
/// - Adds new renames (new entries override existing ones for the same key)
/// - Resolves chains: if A→B and B→C, collapses to A→C (iterates until stable)
/// - Removes stale entries: if an old-path now exists in `current_build_urls`
/// - Removes self-loops: if A→A after chain resolution
pub fn merge_redirects(
    existing: &BTreeMap<String, String>,
    new_renames: &HashMap<String, String>,
    current_build_urls: &HashSet<String>,
) -> BTreeMap<String, String> {
    // Start with existing, override with new
    let mut merged: BTreeMap<String, String> = existing.clone();
    for (k, v) in new_renames {
        merged.insert(k.clone(), v.clone());
    }

    // Chain resolution: for each entry, follow the chain to its final target.
    // Uses cycle detection (visited set) to handle A→B→A gracefully.
    let keys: Vec<String> = merged.keys().cloned().collect();
    for key in &keys {
        let mut target = match merged.get(key) {
            Some(t) => t.clone(),
            None => continue,
        };
        let mut visited = HashSet::new();
        visited.insert(key.clone());
        while let Some(next) = merged.get(&target) {
            if visited.contains(&target) {
                // Cycle detected — collapse to self-loop (will be removed below)
                target = key.clone();
                break;
            }
            visited.insert(target.clone());
            target = next.clone();
        }
        merged.insert(key.clone(), target);
    }

    // Remove stale: old-path now exists as a real page in the current build
    merged.retain(|old_path, _| !current_build_urls.contains(old_path));

    // Remove self-loops
    merged.retain(|k, v| k != v);

    merged
}

/// Generate an HTML redirect page for the given target URL.
///
/// Uses `<meta http-equiv="refresh">` for immediate redirect with a
/// `<link rel="canonical">` for SEO and a fallback anchor link.
pub fn generate_redirect_html(target_url: &str) -> String {
    // Ensure absolute path (pretty URLs from article-map don't have leading /);
    // an external https URL is already absolute.
    let absolute = if target_url.starts_with('/') || target_url.starts_with("https://") {
        target_url.to_string()
    } else {
        format!("/{target_url}") // allow:served-path-url-construct (user-authored redirect target URL from frontmatter, not a framework asset)
    };
    // The target is author text (a `[redirects]` line, a `#fragment`): escaped
    // here, at the one place a stub is written, so no source can break out of
    // the attribute or the link text.
    let absolute = crate::build::page::meta::escape_html_attr(&absolute);
    format!(
        r#"<!DOCTYPE html>
<html><head>
<meta charset="utf-8">
<meta http-equiv="refresh" content="0; url={absolute}">
<link rel="canonical" href="{absolute}">
<title>Redirecting...</title>
</head><body>
<p>This page has moved to <a href="{absolute}">{absolute}</a>.</p>
</body></html>"#
    )
}

/// Convert a pretty URL to a filesystem path for the redirect stub.
///
/// All article pretty URLs map to `{path}/index.html` since moss generates
/// directory-style output (`slug/index.html`).
///
/// - `"foo/bar/"` → `"foo/bar/index.html"`
/// - `"foo/bar"` → `"foo/bar/index.html"`
/// - `""` → `"index.html"` (root)
pub fn pretty_url_to_fs_path(pretty_url: &str) -> String {
    let trimmed = pretty_url.trim_end_matches('/');
    if trimmed.is_empty() {
        "index.html".to_string()
    } else {
        format!("{}/index.html", trimmed)
    }
}

/// What the table needs beyond the persisted history and the article map.
#[derive(Default)]
pub struct TableInputs {
    /// Term-kind namespace moves, `(old, new)`: recomputed every build, stored nowhere.
    pub kind_moves: Vec<(String, String)>,
    /// The site resolved a real (non-loopback) URL: only then does the build
    /// serve the generator's former paths and ship `_moss/redirects.json`.
    pub has_deployed_url: bool,
    /// The `[redirects]` entries of the config the build already parsed.
    pub declared: Vec<(String, String)>,
}

/// Build-time entry point: detect renames, write every redirect into `pending`,
/// and persist `redirects.json`.
///
/// Call this in `generate_blocking_content` AFTER `build_article_map` so the in-memory
/// `current_article_map` is available. The baseline for rename detection is the
/// record of what is live (`manifest::live_baseline`), written by
/// `deploy::landed` when a publish lands — so it reflects what URLs are serving
/// at the point a new build starts. Returns what to tell the user about that
/// record; an unreadable one detects no renames rather than claiming none.
///
/// Stubs and copies are emitted into `pending` via `BuildContext::for_render`,
/// so `seal()` covers them and the generation-id is stable for all of them.
pub fn emit_redirect_table(
    paths: &crate::moss_paths::MossPaths,
    current_article_map: &ArticleMap,
    inputs: &TableInputs,
    output_dir: &Path,
    pending: &mut crate::build::manifest::PendingManifest,
) -> Result<BaselineHealth, String> {
    use super::redirect_table::{self as table, Candidate, Form, Probe};
    use crate::build::context::BuildContext;
    use crate::build::manifest::HashBucket;

    let data_dir = paths.data_dir();
    let baseline = live_baseline::load(paths);

    // `Err` here means the file is present but could not be read — evicted by
    // the sync client, half-synced, bad permissions. The merge below would then
    // start from an empty history and the save would write that back, erasing
    // every redirect the site has ever accumulated. So an unreadable file
    // suppresses the SAVE, not the build: the stubs already on disk keep
    // serving, and the next build (after the download lands) writes the real
    // merge. Only a file that is provably absent is legitimately "no redirects
    // yet".
    let existing: Option<BTreeMap<String, String>> = load_redirects(&data_dir);
    // An unreadable baseline detects NO new renames — not "no renames". The
    // difference is the one the uid-remint bug is about: a build that decides against a
    // record it could not read publishes that decision. Every stub already in
    // `redirects.json` keeps serving (they are merged below and re-emitted),
    // so the site loses nothing it has already earned; what it cannot do is
    // notice a move made since the last publish, and the advisory says so
    // instead of telling the user their site is complete.
    let mut new_renames = match &baseline {
        Baseline::Present(projection) => detect_renames(projection, current_article_map),
        Baseline::Absent | Baseline::Unreadable(_) => HashMap::new(),
    };
    // Folder homes have no article UID entry. Their unchanged source path
    // still identifies an output moved by slug rules or a `url:` edit.
    // The same published mapping preserves QR files for all unchanged page
    // sources. Copies go through the table's normal output/collision checks.
    use crate::build::manifest::{backfill, published_record};
    use crate::build::media::qr::qr_key_for_url_path;
    use crate::build::scan::article_map::to_pretty_url;
    if let Some(previous) = published_record::load_address_baseline(paths, backfill::publish_target(paths).as_deref()) {
        let folder_sources: HashSet<&str> = current_article_map.pages.values().map(String::as_str).collect();
        for (source, old) in &previous.source_to_output {
            let Some(new) = pending.source_to_output().get(source) else { continue };
            if old == new { continue; }
            if folder_sources.contains(source.as_str()) {
                new_renames.insert(to_pretty_url(old), to_pretty_url(new));
            }
            let (old_qr, new_qr) = (qr_key_for_url_path(old), qr_key_for_url_path(new));
            if previous.files.contains_key(&old_qr) && pending.is_registered(&new_qr) {
                new_renames.insert(old_qr, new_qr);
            }
        }
    }
    let current_urls = current_build_urls(current_article_map);
    let merged = merge_redirects(existing.as_ref().unwrap_or(&BTreeMap::new()), &new_renames, &current_urls);

    match existing {
        Some(_) => {
            if let Err(e) = save_redirects(&data_dir, &merged) {
                log::warn!("Failed to save redirects.json: {}", e);
            }
        }
        None => log::warn!(
            "redirects.json could not be read (still in the cloud?) — emitting the \
             redirects this build can see, but NOT rewriting the file: overwriting it \
             from an empty history would erase every earlier redirect"
        ),
    }

    // The three sources, weakest first: a later entry for the same old address
    // replaces an earlier one, so a hand-declared line beats the build's own.
    let trusted = |from: &str, to: &str| Candidate { from: from.into(), to: to.into(), declared: false };
    let mut candidates: Vec<Candidate> = Vec::new();
    if inputs.has_deployed_url {
        candidates.extend(table::FORMER_PATHS.iter().map(|(from, to)| trusted(from, to)));
    }
    candidates.extend(merged.iter().map(|(from, to)| trusted(from, to)));
    candidates.extend(inputs.kind_moves.iter().map(|(from, to)| trusted(from, to)));
    candidates.extend(
        inputs.declared.iter().map(|(from, to)| Candidate { from: from.clone(), to: to.clone(), declared: true }),
    );
    let project_root = paths.project_root();
    let planned = table::plan(candidates, &Probe { pending, source_root: project_root });

    // Every build re-derives the table regardless (no I/O of its own), but
    // writing every stub's bytes to stage on every build — the loop below — is
    // waste once a site has accumulated more than a handful of redirects and
    // nothing about them changed: the same bytes land on the same paths every
    // time. The stub set (old address to link) is the one value every input —
    // a rename, an edited redirects.json, a `[redirects]` line — can only reach
    // the stubs by changing, so comparing it against what the last build of
    // this folder in this process emitted from is the whole gate. Carrying
    // stub paths forward, rather than skipping them outright, is not optional:
    // `seal`'s mark-and-sweep and `remove_stale_html` both delete any
    // `index.html` this build did not register. Copies are not gated: their
    // bytes follow their target, which this signature does not see.
    let stubs: BTreeMap<String, String> = planned
        .iter()
        .filter(|p| p.form == Form::Stub)
        .map(|p| (p.from.served(), p.to.href()))
        .collect();
    let records = crate::system::build_records::records();
    let folder_key = project_root.to_string_lossy().into_owned();
    let unchanged = records.redirect_signature(&folder_key).as_ref() == Some(&stubs)
        && planned
            .iter()
            .filter(|p| p.form == Form::Stub)
            .all(|p| crate::build::io_utils::output_present(&output_dir.join(p.from.key())));

    let probe = Probe { pending, source_root: project_root };
    let copy_sources: Vec<Option<table::CopySource>> = planned
        .iter()
        .map(|p| match (&p.form, &p.to) {
            (Form::Copy, table::Target::Internal(t, _)) => probe.copy_source(t, output_dir),
            _ => None,
        })
        .collect();
    for (p, source) in planned.iter().zip(copy_sources) {
        let sp = p.from.served_path();
        match p.form {
            Form::Stub => {
                let html = generate_redirect_html(&p.to.href());
                if unchanged {
                    // Same bytes already on disk from a previous build in this
                    // session — re-register without rewriting or re-hashing off a
                    // fresh write, so the manifest still carries the path through
                    // `seal`.
                    let hash = compute_binary_hash(html.as_bytes());
                    pending.register_hashed(sp, &hash, crate::build::manifest::HashBucket::Files);
                } else {
                    BuildContext::for_render(output_dir, pending)
                        .emit(sp, html.as_bytes(), HashBucket::Files)
                        .map_err(|e| format!("Failed to emit redirect stub '{}': {}", sp.as_str(), e))?;
                }
            }
            Form::Copy => match source {
                // Generated this build (the feed): small, and rewritten by every
                // build, so its bytes are held like the original's.
                Some(table::CopySource::Stage(path)) => {
                    let bytes = std::fs::read(&path).map_err(|e| format!("Failed to read '{}': {}", path.display(), e))?;
                    BuildContext::for_render(output_dir, pending)
                        .emit_held(sp, bytes, HashBucket::Files)
                        .map_err(|e| format!("Failed to emit redirect copy '{}': {}", sp.as_str(), e))?;
                }
                // The site folder's own file, which may be large: copied and
                // hashed in chunks, never read whole into memory.
                Some(table::CopySource::Folder(path)) => {
                    let dst = output_dir.join(sp.as_str());
                    crate::build::io_utils::copy_output(&path, &dst)
                        .map_err(|e| format!("Failed to copy redirect '{}': {}", sp.as_str(), e))?;
                    let hash = crate::build::assets::paths::compute_binary_hash_file(&dst)?;
                    pending.register_hashed(sp, &hash, HashBucket::Files);
                }
                None => crate::build::cli_output::log_warn_problem!(
                    "redirect '{}': its target could not be read, so no copy was written",
                    p.from.served()
                ),
            },
            Form::TableOnly => {}
        }
    }
    records.record_redirect_signature(&folder_key, stubs);

    if inputs.has_deployed_url {
        BuildContext::for_render(output_dir, pending)
            .emit_held(&crate::build::served_path::ServedPath::for_redirects_manifest(), table::to_json(&planned), HashBucket::Files)
            .map_err(|e| format!("Failed to emit _moss/redirects.json: {}", e))?;
    }

    if !planned.is_empty() {
        if unchanged {
            log::debug!(
                "build: {} redirect(s) unchanged since the last build in this session — stubs not re-written",
                planned.len()
            );
        } else {
            log::info!("build: emitted {} redirect(s) into pending manifest", planned.len());
        }
    }

    Ok(BaselineHealth::of(paths, &baseline))
}

/// What the caller should tell the user about the live-record baseline.
///
/// Returned rather than reported here because this module owns no reporter —
/// and because the two facts it carries are not redirect facts. They are facts
/// about the record, which duplicate-uid resolution reads too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineHealth {
    /// A record was read, or there is honestly nothing to read yet.
    Fine,
    /// A record exists and could not be used.
    Unusable(Unreadable),
    /// This site is published, but nothing here says what is live: either
    /// there is no record at all, or the records were written before moss kept
    /// note IDs in them. Either way the next rename leaves no forwarding link
    /// until one more publish rewrites the record.
    ///
    /// Told to the log, not to the author (2026-08-29): the next publish ends
    /// this on its own, so there is nothing for her to decide, and the notice
    /// that used to carry it read as a fault report.
    NoBaselineYet,
}

impl BaselineHealth {
    fn of(paths: &crate::moss_paths::MossPaths, baseline: &Baseline) -> Self {
        match baseline {
            // A record that predates note IDs is not damaged and waiting will
            // not improve it — it is simply an older shape, and one publish
            // replaces it. That is the same sentence as "no record yet", so it
            // gets the same verdict rather than an alarm of its own.
            Baseline::Unreadable(Unreadable::PredatesUids) => BaselineHealth::NoBaselineYet,
            Baseline::Unreadable(u) => BaselineHealth::Unusable(*u),
            Baseline::Present(_) => BaselineHealth::Fine,
            Baseline::Absent if site_has_been_published(paths) => BaselineHealth::NoBaselineYet,
            Baseline::Absent => BaselineHealth::Fine,
        }
    }
}

/// Has this site ever gone live, according to state kept outside the publish
/// record?
///
/// Asked so that "no baseline" can be told apart from "never published", which
/// is the ordinary state of a site nobody has shipped yet and deserves no
/// advisory at all.
fn site_has_been_published(paths: &crate::moss_paths::MossPaths) -> bool {
    crate::build::site_config::get_domain_config(&paths.project_root().to_string_lossy())
        .map(|c| c.last_deployment_at.is_some())
        .unwrap_or(false)
}

/// Load redirects from `data_dir/redirects.json`.
///
/// `None` means the file is present but could not be read — see
/// `emit_redirect_table`, where that is the difference between merging and
/// erasing. `Some(empty)` means it is genuinely absent (or empty), which is the
/// ordinary state of a site that has never renamed a page.
pub fn load_redirects(data_dir: &Path) -> Option<BTreeMap<String, String>> {
    let path = data_dir.join("redirects.json");
    match crate::build::cloud_readiness::read_input_if_present(&path) {
        Ok(Some(contents)) => Some(serde_json::from_str(&contents).unwrap_or_default()),
        Ok(None) => Some(BTreeMap::new()),
        Err(e) => {
            log::warn!("could not read {} ({e})", path.display());
            None
        }
    }
}

/// Save redirects as pretty-printed JSON to `data_dir/redirects.json`.
///
/// Creates the parent directory if it doesn't exist.
///
/// Deliberately NOT an `io_utils` output write. `.moss/data` is user state —
/// this file is a site's rename history, it syncs on purpose, and it is not
/// regenerable — so the "dataless is absent" rule for regenerable output does not reach it. If
/// the sync client has evicted it, clobbering it with a map that has lost the
/// history is worse than failing, and "unreadable is not absent" applies in its
/// original, unqualified form.
pub fn save_redirects(data_dir: &Path, redirects: &BTreeMap<String, String>) -> Result<(), String> {
    let path = data_dir.join("redirects.json");
    let json = serde_json::to_string_pretty(redirects)
        .map_err(|e| format!("Failed to serialize redirects: {}", e))?;
    if std::fs::read(&path).is_ok_and(|existing| existing == json.as_bytes()) {
        return Ok(()); // unchanged — don't churn the mtime
    }
    if let Some(parent) = path.parent() {
        // allow:raw_write .moss/data, not the build tree
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }
    std::fs::write(&path, json.as_bytes())  // allow:raw_write user state under .moss/data, not regenerable output
        .map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::scan::article_map::ArticleInfo;
    use std::collections::{BTreeMap, HashMap};

    /// Helper to create an ArticleInfo with the given UID (other fields use defaults).
    fn article_info_with_uid(uid: Option<&str>) -> ArticleInfo {
        ArticleInfo {
            source_path: String::new(),
            title: String::new(),
            content: String::new(),
            html_content: None,
            frontmatter: HashMap::new(),
            url_path: String::new(),
            date: None,
            tags: vec![],
            uid: uid.map(|s| s.to_string()),
        }
    }

    /// What a map says is live, as a landed publish would have recorded it.
    fn projection_of(map: &ArticleMap) -> LiveBaseline {
        live_baseline::from_article_map(map)
    }

    // ---------------------------------------------------------------
    // detect_renames
    // ---------------------------------------------------------------

    #[test]
    fn published_folder_paths_and_qr_files_survive_slug_changes() {
        use crate::build::context::BuildContext;
        use crate::build::manifest::{change_set::PublishedSnapshot, published_record, HashBucket, PendingManifest};
        use crate::build::media::qr::qr_key_for_url_path;
        use crate::build::scan::article_map::to_pretty_url;
        use crate::build::served_path::ServedPath;
        use crate::moss_paths::MossPaths;
        use crate::types::content::SiteHashes;

        let test_tmp = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
            .parent().unwrap().join("target/test-tmp");
        std::fs::create_dir_all(&test_tmp).unwrap();
        let tmp = tempfile::TempDir::new_in(&test_tmp).unwrap();
        let paths = MossPaths::new(tmp.path());
        let output = tmp.path().join("output");
        let cases = [
            ("cafe\u{301}/index.md", "cafe/index.html", "café/index.html", None),
            ("folder/index.md", "share-id/index.html", "share/id/index.html", None),
            ("ordinary.md", "path-item/index.html", "path/item/index.html", Some("arti0001")),
            ("note-e\u{301}.md", "note-e/index.html", "note-é/index.html", Some("arti0002")),
        ];
        let mut previous_map = ArticleMap::new();
        let mut current_map = ArticleMap::new();
        let mut record = PublishedSnapshot { target: "moss:fixture".into(), ..Default::default() };
        let mut pending = PendingManifest::new(SiteHashes::default());
        for (source, old, new, uid) in cases {
            record.source_to_output.insert(source.into(), old.into());
            record.files.insert(old.into(), "old-page".into());
            record.files.insert(qr_key_for_url_path(old), "old-qr".into());
            if let Some(uid) = uid {
                let mut info = article_info_with_uid(Some(uid));
                info.source_path = source.into();
                previous_map.articles.insert(to_pretty_url(old), info.clone());
                current_map.articles.insert(to_pretty_url(new), info);
            } else {
                current_map.pages.insert(to_pretty_url(new), source.into());
            }
            let page = ServedPath::from_source(new).unwrap();
            pending.register_source_mapping(source.into(), &page);
            for (key, bytes) in [(new.to_string(), b"<html>current page</html>".as_slice()), (qr_key_for_url_path(new), b"<svg>current QR</svg>".as_slice())] {
                BuildContext::for_render(&output, &mut pending)
                    .emit(&ServedPath::from_source(&key).unwrap(), bytes, HashBucket::Files).unwrap();
            }
        }
        record.triples = Some(projection_of(&previous_map).entries);
        published_record::save(&paths, &record).unwrap();
        emit_redirect_table(&paths, &current_map, &TableInputs { has_deployed_url: true, ..Default::default() }, &output, &mut pending).unwrap();
        let sealed = pending.seal();
        let table: serde_json::Value = serde_json::from_slice(&std::fs::read(output.join("_moss/redirects.json")).unwrap()).unwrap();
        for (_, old, new, _) in cases {
            assert!(sealed.files().contains_key(old), "old page missing from seal: {old}");
            assert!(std::fs::read_to_string(output.join(old)).unwrap().contains(&format!("url=/{}", to_pretty_url(new))));
            let old_qr = qr_key_for_url_path(old);
            assert!(sealed.files().contains_key(&old_qr), "old QR missing from seal: {old_qr}");
            assert_eq!(std::fs::read(output.join(&old_qr)).unwrap(), std::fs::read(output.join(qr_key_for_url_path(new))).unwrap());
            for from in [format!("/{}", to_pretty_url(old)), format!("/{old_qr}")] {
                assert!(table["redirects"].as_array().unwrap().iter().any(|r| r["from"] == from), "missing redirect: {from}");
            }
        }
    }

    #[test]
    fn detect_renames_same_uid_different_url() {
        let mut prev = ArticleMap::new();
        prev.articles.insert(
            "posts/old-slug/".to_string(),
            article_info_with_uid(Some("uid-abc")),
        );

        let mut curr = ArticleMap::new();
        curr.articles.insert(
            "posts/new-slug/".to_string(),
            article_info_with_uid(Some("uid-abc")),
        );

        let renames = detect_renames(&projection_of(&prev), &curr);
        assert_eq!(renames.len(), 1);
        assert_eq!(renames.get("posts/old-slug/").unwrap(), "posts/new-slug/");
    }

    #[test]
    fn detect_renames_same_uid_same_url_no_rename() {
        let mut prev = ArticleMap::new();
        prev.articles.insert(
            "posts/article/".to_string(),
            article_info_with_uid(Some("uid-123")),
        );

        let mut curr = ArticleMap::new();
        curr.articles.insert(
            "posts/article/".to_string(),
            article_info_with_uid(Some("uid-123")),
        );

        let renames = detect_renames(&projection_of(&prev), &curr);
        assert!(renames.is_empty());
    }

    #[test]
    fn detect_renames_uid_only_in_previous_no_rename() {
        let mut prev = ArticleMap::new();
        prev.articles.insert(
            "posts/deleted/".to_string(),
            article_info_with_uid(Some("uid-del")),
        );

        let curr = ArticleMap::new(); // empty — page was deleted

        let renames = detect_renames(&projection_of(&prev), &curr);
        assert!(renames.is_empty());
    }

    #[test]
    fn detect_renames_uid_only_in_current_no_rename() {
        let prev = ArticleMap::new(); // empty — new page

        let mut curr = ArticleMap::new();
        curr.articles.insert(
            "posts/brand-new/".to_string(),
            article_info_with_uid(Some("uid-new")),
        );

        let renames = detect_renames(&projection_of(&prev), &curr);
        assert!(renames.is_empty());
    }

    #[test]
    fn detect_renames_multiple_renames() {
        let mut prev = ArticleMap::new();
        prev.articles.insert(
            "posts/old-a/".to_string(),
            article_info_with_uid(Some("uid-a")),
        );
        prev.articles.insert(
            "posts/old-b/".to_string(),
            article_info_with_uid(Some("uid-b")),
        );

        let mut curr = ArticleMap::new();
        curr.articles.insert(
            "posts/new-a/".to_string(),
            article_info_with_uid(Some("uid-a")),
        );
        curr.articles.insert(
            "posts/new-b/".to_string(),
            article_info_with_uid(Some("uid-b")),
        );

        let renames = detect_renames(&projection_of(&prev), &curr);
        assert_eq!(renames.len(), 2);
        assert_eq!(renames.get("posts/old-a/").unwrap(), "posts/new-a/");
        assert_eq!(renames.get("posts/old-b/").unwrap(), "posts/new-b/");
    }

    #[test]
    fn detect_renames_skips_none_uid() {
        let mut prev = ArticleMap::new();
        prev.articles.insert(
            "posts/no-uid/".to_string(),
            article_info_with_uid(None),
        );

        let mut curr = ArticleMap::new();
        curr.articles.insert(
            "posts/moved-no-uid/".to_string(),
            article_info_with_uid(None),
        );

        let renames = detect_renames(&projection_of(&prev), &curr);
        assert!(renames.is_empty(), "Articles without UIDs should not produce renames");
    }

    #[test]
    fn detect_renames_mixed_uid_and_none() {
        let mut prev = ArticleMap::new();
        prev.articles.insert(
            "posts/has-uid/".to_string(),
            article_info_with_uid(Some("uid-x")),
        );
        prev.articles.insert(
            "posts/no-uid/".to_string(),
            article_info_with_uid(None),
        );

        let mut curr = ArticleMap::new();
        curr.articles.insert(
            "posts/renamed/".to_string(),
            article_info_with_uid(Some("uid-x")),
        );
        curr.articles.insert(
            "posts/also-moved/".to_string(),
            article_info_with_uid(None),
        );

        let renames = detect_renames(&projection_of(&prev), &curr);
        assert_eq!(renames.len(), 1);
        assert_eq!(renames.get("posts/has-uid/").unwrap(), "posts/renamed/");
    }

    // ---------------------------------------------------------------
    // merge_redirects
    // ---------------------------------------------------------------

    #[test]
    fn merge_redirects_chain_resolution() {
        // Existing: A → B. New rename: B → C. Should collapse to A → C.
        let mut existing = BTreeMap::new();
        existing.insert("a/".to_string(), "b/".to_string());

        let mut new_renames = HashMap::new();
        new_renames.insert("b/".to_string(), "c/".to_string());

        let current_urls = HashSet::new();

        let result = merge_redirects(&existing, &new_renames, &current_urls);
        assert_eq!(result.get("a/").unwrap(), "c/");
        assert_eq!(result.get("b/").unwrap(), "c/");
    }

    #[test]
    fn merge_redirects_staleness_removal() {
        // Old redirect A → B, but A now exists as a real page
        let mut existing = BTreeMap::new();
        existing.insert("a/".to_string(), "b/".to_string());

        let new_renames = HashMap::new();

        let mut current_urls = HashSet::new();
        current_urls.insert("a/".to_string());

        let result = merge_redirects(&existing, &new_renames, &current_urls);
        assert!(result.is_empty(), "Redirect should be removed when old path exists in current build");
    }

    #[test]
    fn merge_redirects_self_loop_removal() {
        // A → B → A is a cycle — both entries become self-loops and are removed
        let mut existing = BTreeMap::new();
        existing.insert("a/".to_string(), "b/".to_string());

        let mut new_renames = HashMap::new();
        new_renames.insert("b/".to_string(), "a/".to_string());

        let current_urls = HashSet::new();

        let result = merge_redirects(&existing, &new_renames, &current_urls);
        // Both a/ → b/ → a/ and b/ → a/ → b/ are cycles → self-loops → removed
        assert!(result.is_empty(), "All cyclic redirects should be removed");
    }

    #[test]
    fn merge_redirects_new_overrides_existing() {
        let mut existing = BTreeMap::new();
        existing.insert("a/".to_string(), "b/".to_string());

        let mut new_renames = HashMap::new();
        new_renames.insert("a/".to_string(), "c/".to_string());

        let current_urls = HashSet::new();

        let result = merge_redirects(&existing, &new_renames, &current_urls);
        assert_eq!(result.get("a/").unwrap(), "c/");
    }

    #[test]
    fn merge_redirects_empty_inputs() {
        let existing = BTreeMap::new();
        let new_renames = HashMap::new();
        let current_urls = HashSet::new();

        let result = merge_redirects(&existing, &new_renames, &current_urls);
        assert!(result.is_empty());
    }

    #[test]
    fn merge_redirects_longer_chain() {
        // Simulate three successive renames: A→B, then B→C, then C→D
        // All accumulated in existing.
        let mut existing = BTreeMap::new();
        existing.insert("a/".to_string(), "b/".to_string());
        existing.insert("b/".to_string(), "c/".to_string());

        let mut new_renames = HashMap::new();
        new_renames.insert("c/".to_string(), "d/".to_string());

        let current_urls = HashSet::new();

        let result = merge_redirects(&existing, &new_renames, &current_urls);
        // All should point to the final destination
        assert_eq!(result.get("a/").unwrap(), "d/");
        assert_eq!(result.get("b/").unwrap(), "d/");
        assert_eq!(result.get("c/").unwrap(), "d/");
    }

    // ---------------------------------------------------------------
    // generate_redirect_html
    // ---------------------------------------------------------------

    #[test]
    fn generate_redirect_html_contains_meta_refresh() {
        let html = generate_redirect_html("posts/new-article/");
        assert!(html.contains(r#"content="0; url=/posts/new-article/""#));
    }

    #[test]
    fn generate_redirect_html_contains_canonical() {
        let html = generate_redirect_html("posts/new-article/");
        assert!(html.contains(r#"<link rel="canonical" href="/posts/new-article/">"#));
    }

    #[test]
    fn generate_redirect_html_contains_fallback_anchor() {
        let html = generate_redirect_html("posts/new-article/");
        assert!(html.contains(r#"<a href="/posts/new-article/">/posts/new-article/</a>"#));
    }

    #[test]
    fn generate_redirect_html_has_doctype() {
        let html = generate_redirect_html("somewhere/");
        assert!(html.starts_with("<!DOCTYPE html>"));
    }

    #[test]
    fn generate_redirect_html_already_absolute() {
        let html = generate_redirect_html("/posts/new-article/");
        // Should not double-prefix
        assert!(html.contains(r#"url=/posts/new-article/"#));
        assert!(!html.contains("url=//"));
    }

    // ---------------------------------------------------------------
    // pretty_url_to_fs_path
    // ---------------------------------------------------------------

    #[test]
    fn pretty_url_trailing_slash() {
        assert_eq!(pretty_url_to_fs_path("foo/bar/"), "foo/bar/index.html");
    }

    #[test]
    fn pretty_url_no_trailing_slash() {
        assert_eq!(pretty_url_to_fs_path("foo/bar"), "foo/bar/index.html");
    }

    #[test]
    fn pretty_url_empty_is_root() {
        assert_eq!(pretty_url_to_fs_path(""), "index.html");
    }

    #[test]
    fn pretty_url_single_segment_trailing_slash() {
        assert_eq!(pretty_url_to_fs_path("about/"), "about/index.html");
    }

    #[test]
    fn pretty_url_single_segment_no_slash() {
        assert_eq!(pretty_url_to_fs_path("about"), "about/index.html");
    }

    // ---------------------------------------------------------------
    // load / save round-trip
    // ---------------------------------------------------------------

    #[test]
    fn load_save_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path();

        let mut redirects = BTreeMap::new();
        redirects.insert("old/page/".to_string(), "new/page/".to_string());
        redirects.insert("posts/draft/".to_string(), "posts/published/".to_string());

        save_redirects(data_dir, &redirects).unwrap();
        let loaded = load_redirects(data_dir);

        assert_eq!(loaded, Some(redirects));
    }

    #[test]
    fn load_nonexistent_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("nonexistent_subdir");

        // Provably absent — `Some(empty)`, not `None`. The difference decides
        // whether `emit_redirect_table` rewrites the file or leaves it alone.
        assert_eq!(load_redirects(&data_dir), Some(BTreeMap::new()));
    }

    #[test]
    fn save_creates_parent_dir() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b").join("c");

        let redirects = BTreeMap::new();
        save_redirects(&nested, &redirects).unwrap();

        assert!(nested.join("redirects.json").exists());
    }

    #[test]
    fn save_redirects_is_idempotent() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let mut redirects = BTreeMap::new();
        redirects.insert("/old".to_string(), "/new".to_string());
        redirects.insert("/other".to_string(), "/target".to_string());

        save_redirects(dir.path(), &redirects).unwrap();
        let path = dir.path().join("redirects.json");
        let mtime_before = fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));

        save_redirects(dir.path(), &redirects).unwrap();
        let mtime_after = fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(mtime_before, mtime_after,
            "second save of identical map must not touch mtime");
    }

    // ---------------------------------------------------------------
    // emit_redirect_table (build-time, PendingManifest integration)
    // ---------------------------------------------------------------

    #[test]
    fn emit_redirect_table_stub_in_sealed_manifest() {
        use crate::build::context::BuildContext;
        use crate::build::manifest::{HashBucket, PendingManifest};
        use crate::build::served_path::ServedPath;
        use crate::moss_paths::MossPaths;
        use crate::types::content::SiteHashes;

        // PORTABLE TempDir pattern: artifacts go inside the repo target/ tree.
        let test_tmp = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
            .parent()
            .unwrap()
            .join("target")
            .join("test-tmp");
        std::fs::create_dir_all(&test_tmp).unwrap();
        let tmp = tempfile::TempDir::new_in(&test_tmp).unwrap();

        // Set up a .moss directory structure that MossPaths expects.
        let moss_dir = tmp.path().join(".moss");
        let data_dir = moss_dir.join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let deploy_dir = moss_dir.join("deploy");
        std::fs::create_dir_all(&deploy_dir).unwrap();
        let output_dir = tmp.path().join("output");
        std::fs::create_dir_all(&output_dir).unwrap();

        // Write a deployed snapshot: uid X was at "old/page/"
        let deployed_map = {
            let mut m = ArticleMap::new();
            m.articles.insert(
                "old/page/".to_string(),
                article_info_with_uid(Some("uid-X")),
            );
            m
        };
        let snapshot_path = deploy_dir.join("deployed-article-map.json");
        std::fs::write(&snapshot_path, serde_json::to_string(&deployed_map).unwrap()).unwrap();

        // Current article-map: uid X now at "new/page/"
        let mut current_map = ArticleMap::new();
        current_map.articles.insert(
            "new/page/".to_string(),
            article_info_with_uid(Some("uid-X")),
        );

        let paths = MossPaths::from_moss_dir(moss_dir);

        let mut pending = PendingManifest::new(SiteHashes::default());

        // Also emit "new/page/index.html" so it counts as a touched entry
        // in the build (the new URL is a real page that will appear in sealed.files()).
        let new_sp = ServedPath::from_source("new/page/index.html").unwrap();
        BuildContext::for_render(&output_dir, &mut pending)
            .emit(&new_sp, b"<html>new</html>", HashBucket::Files)
            .unwrap();

        // Call the new function.
        emit_redirect_table(&paths, &current_map, &TableInputs::default(), &output_dir, &mut pending).unwrap();

        // Seal and inspect.
        let sealed = pending.seal();

        // The stub for old/page/ must appear in the sealed manifest.
        let stub_key = "old/page/index.html";
        assert!(
            sealed.files().contains_key(stub_key),
            "sealed manifest must contain redirect stub '{}'; got keys: {:?}",
            stub_key,
            sealed.files().keys().collect::<Vec<_>>()
        );

        // The stub file must exist on disk.
        let stub_file = output_dir.join(stub_key);
        assert!(stub_file.exists(), "stub file must exist at {:?}", stub_file);

        // The recorded hash must match file_entry(compute_binary_hash(...)).
        let expected_html = generate_redirect_html("new/page/");
        let expected_hash = file_entry(&compute_binary_hash(expected_html.as_bytes()));
        assert_eq!(
            sealed.files().get(stub_key).unwrap(),
            &expected_hash,
            "stub hash must equal file_entry(compute_binary_hash(redirect_html))"
        );
    }


    /// `current_build_urls` unions `articles` and `pages` (fix for the
    /// namespace-move redirect clobbering a real folder index at the old
    /// address) — this consumer, the persisted-rename path, must get the
    /// same protection for the same reason: a rename's old URL can just as
    /// easily be reclaimed by a real folder index as by a real article, and
    /// `merge_redirects`'s staleness check is oblivious to which `ArticleMap`
    /// bucket the URL came from. Regresses to `.articles.keys()` alone by
    /// reverting the `current_build_urls` call in `emit_redirect_table`.
    #[test]
    fn emit_redirect_table_skips_a_rename_whose_old_url_is_now_a_pages_entry() {
        use crate::build::context::BuildContext;
        use crate::build::manifest::{HashBucket, PendingManifest};
        use crate::build::served_path::ServedPath;
        use crate::moss_paths::MossPaths;
        use crate::types::content::SiteHashes;

        let test_tmp = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
            .parent()
            .unwrap()
            .join("target")
            .join("test-tmp");
        std::fs::create_dir_all(&test_tmp).unwrap();
        let tmp = tempfile::TempDir::new_in(&test_tmp).unwrap();

        let moss_dir = tmp.path().join(".moss");
        let data_dir = moss_dir.join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let deploy_dir = moss_dir.join("deploy");
        std::fs::create_dir_all(&deploy_dir).unwrap();
        let output_dir = tmp.path().join("output");
        std::fs::create_dir_all(&output_dir).unwrap();

        // Write a deployed snapshot: uid X was at "old/page/"
        let deployed_map = {
            let mut m = ArticleMap::new();
            m.articles.insert(
                "old/page/".to_string(),
                article_info_with_uid(Some("uid-X")),
            );
            m
        };
        let snapshot_path = deploy_dir.join("deployed-article-map.json");
        std::fs::write(&snapshot_path, serde_json::to_string(&deployed_map).unwrap()).unwrap();

        // Current article-map: uid X moved to "new/page/" (a rename this
        // build would otherwise detect), AND "old/page/" is now a real
        // folder-index page — `pages`, not `articles` — reoccupying the
        // address the rename would have redirected.
        let mut current_map = ArticleMap::new();
        current_map.articles.insert(
            "new/page/".to_string(),
            article_info_with_uid(Some("uid-X")),
        );
        current_map.pages.insert("old/page/".to_string(), "old/page/index.md".to_string());

        let paths = MossPaths::from_moss_dir(moss_dir);
        let mut pending = PendingManifest::new(SiteHashes::default());

        let new_sp = ServedPath::from_source("new/page/index.html").unwrap();
        BuildContext::for_render(&output_dir, &mut pending)
            .emit(&new_sp, b"<html>new</html>", HashBucket::Files)
            .unwrap();
        // The real folder-index page this build actually serves at the old
        // address — what a redirect stub there would silently clobber.
        let old_sp = ServedPath::from_source("old/page/index.html").unwrap();
        BuildContext::for_render(&output_dir, &mut pending)
            .emit(&old_sp, b"<html>real page</html>", HashBucket::Files)
            .unwrap();

        emit_redirect_table(&paths, &current_map, &TableInputs::default(), &output_dir, &mut pending).unwrap();

        let sealed = pending.seal();
        let stub_key = "old/page/index.html";
        assert_eq!(
            sealed.files().get(stub_key).map(|h| h.as_str()),
            Some(file_entry(&compute_binary_hash(b"<html>real page</html>")).as_str()),
            "the real page's own bytes must survive — no redirect stub may \
             overwrite them: {:?}",
            sealed.files().get(stub_key)
        );

        let redirects_path = data_dir.join("redirects.json");
        let saved: std::collections::BTreeMap<String, String> = serde_json::from_str(
            &std::fs::read_to_string(&redirects_path).unwrap(),
        )
        .unwrap();
        assert!(
            !saved.contains_key("old/page/"),
            "redirects.json must not persist a redirect over a real pages \
             entry: {:?}",
            saved
        );
    }

    /// The uid-remint bug's shape on the redirect side: the record exists and cannot
    /// be read. Every stub the site has already earned must keep being emitted
    /// — the accumulated `redirects.json` is a separate file and is readable —
    /// while the rename this build would otherwise have detected is NOT
    /// invented, and the caller is handed the fact so it can tell the user.
    #[test]
    fn an_unreadable_record_keeps_old_stubs_and_claims_no_new_rename() {
        use crate::build::manifest::PendingManifest;
        use crate::moss_paths::MossPaths;
        use crate::types::content::SiteHashes;

        let test_tmp = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
            .parent()
            .unwrap()
            .join("target")
            .join("test-tmp");
        std::fs::create_dir_all(&test_tmp).unwrap();
        let tmp = tempfile::TempDir::new_in(&test_tmp).unwrap();
        let moss_dir = tmp.path().join(".moss");
        let data_dir = moss_dir.join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let deploy_dir = moss_dir.join("deploy");
        std::fs::create_dir_all(&deploy_dir).unwrap();
        let output_dir = tmp.path().join("output");
        std::fs::create_dir_all(&output_dir).unwrap();

        // A redirect earned by some earlier build, and a record of what is live
        // that parses as neither format.
        let mut earned = BTreeMap::new();
        earned.insert("ancient/page/".to_string(), "current/page/".to_string());
        save_redirects(&data_dir, &earned).unwrap();
        std::fs::write(deploy_dir.join("deployed-article-map.json"), "not json").unwrap();

        // uid-X moved, and this build has no way to know it.
        let mut current_map = ArticleMap::new();
        current_map
            .articles
            .insert("new/page/".to_string(), article_info_with_uid(Some("uid-X")));

        let paths = MossPaths::from_moss_dir(moss_dir);
        let mut pending = PendingManifest::new(SiteHashes::default());
        let health =
            emit_redirect_table(&paths, &current_map, &TableInputs::default(), &output_dir, &mut pending).unwrap();

        assert_eq!(health, BaselineHealth::Unusable(Unreadable::Corrupt));
        let sealed = pending.seal();
        // Exactly the one stub the site had already earned, and no other: a
        // negative assertion naming a URL would pass just as well if the
        // baseline logic were absent altogether.
        let emitted: Vec<&String> = sealed.files().keys().collect();
        assert_eq!(
            emitted,
            vec!["ancient/page/index.html"],
            "an unreadable record keeps what the site has earned and claims nothing new"
        );
    }

    // ---------------------------------------------------------------
    // emit_redirect_table (the re-emission gate)
    // ---------------------------------------------------------------

    /// Sets up a moss folder with one earned rename (uid-X: `old/page/` ->
    /// whatever `current_map` says), and returns everything a caller needs to
    /// call `emit_redirect_table` more than once against it.
    fn gate_test_fixture() -> (crate::moss_paths::MossPaths, std::path::PathBuf, tempfile::TempDir) {
        let test_tmp = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
            .parent()
            .unwrap()
            .join("target")
            .join("test-tmp");
        std::fs::create_dir_all(&test_tmp).unwrap();
        let tmp = tempfile::TempDir::new_in(&test_tmp).unwrap();

        let moss_dir = tmp.path().join(".moss");
        std::fs::create_dir_all(moss_dir.join("data")).unwrap();
        let deploy_dir = moss_dir.join("deploy");
        std::fs::create_dir_all(&deploy_dir).unwrap();
        let output_dir = tmp.path().join("output");
        std::fs::create_dir_all(&output_dir).unwrap();

        let deployed_map = {
            let mut m = ArticleMap::new();
            m.articles.insert(
                "old/page/".to_string(),
                article_info_with_uid(Some("uid-X")),
            );
            m
        };
        std::fs::write(
            deploy_dir.join("deployed-article-map.json"),
            serde_json::to_string(&deployed_map).unwrap(),
        )
        .unwrap();

        let paths = crate::moss_paths::MossPaths::from_moss_dir(moss_dir);
        (paths, output_dir, tmp)
    }

    /// A second build with the same baseline and the same article map — no
    /// rename detected, `redirects.json` untouched between the two calls —
    /// must not rewrite the stub already on disk. Reverting the gate (always
    /// taking the `emit` branch) turns this red: the second call's write
    /// bumps the mtime.
    #[test]
    fn emit_redirect_table_skips_rewrite_when_nothing_relevant_changed() {
        use crate::build::manifest::PendingManifest;
        use crate::types::content::SiteHashes;

        let (paths, output_dir, _tmp) = gate_test_fixture();
        let mut current_map = ArticleMap::new();
        current_map.articles.insert(
            "new/page/".to_string(),
            article_info_with_uid(Some("uid-X")),
        );

        let mut pending1 = PendingManifest::new(SiteHashes::default());
        emit_redirect_table(&paths, &current_map, &TableInputs::default(), &output_dir, &mut pending1).unwrap();
        let stub_path = output_dir.join("old/page/index.html");
        let mtime1 = std::fs::metadata(&stub_path).unwrap().modified().unwrap();
        let sealed1 = pending1.seal();
        assert!(sealed1.files().contains_key("old/page/index.html"));

        std::thread::sleep(std::time::Duration::from_millis(20));

        // Second build in the same process, nothing redirect-relevant changed.
        let mut pending2 = PendingManifest::new(SiteHashes::default());
        emit_redirect_table(&paths, &current_map, &TableInputs::default(), &output_dir, &mut pending2).unwrap();
        let mtime2 = std::fs::metadata(&stub_path).unwrap().modified().unwrap();
        assert_eq!(
            mtime1, mtime2,
            "unchanged redirects.json and article map must not rewrite the stub file"
        );

        let sealed2 = pending2.seal();
        assert!(
            sealed2.files().contains_key("old/page/index.html"),
            "the stub must stay registered even when the write is skipped, or \
             seal's mark-and-sweep (and remove_stale_html) would delete it"
        );
    }

    /// A second build whose article map moves the SAME uid to a different
    /// URL is exactly the "article map changed" half of the gate: it must
    /// re-emit, with the new target.
    #[test]
    fn emit_redirect_table_reemits_when_article_map_changes() {
        use crate::build::manifest::PendingManifest;
        use crate::types::content::SiteHashes;

        let (paths, output_dir, _tmp) = gate_test_fixture();
        let stub_path = output_dir.join("old/page/index.html");

        let mut first_map = ArticleMap::new();
        first_map.articles.insert(
            "new/page/".to_string(),
            article_info_with_uid(Some("uid-X")),
        );
        let mut pending1 = PendingManifest::new(SiteHashes::default());
        emit_redirect_table(&paths, &first_map, &TableInputs::default(), &output_dir, &mut pending1).unwrap();
        pending1.seal();
        let html1 = std::fs::read_to_string(&stub_path).unwrap();
        assert!(html1.contains("url=/new/page/"));

        // Same uid, moved again — a different article map from the one the
        // last build in this process saw.
        let mut second_map = ArticleMap::new();
        second_map.articles.insert(
            "newer/page/".to_string(),
            article_info_with_uid(Some("uid-X")),
        );
        let mut pending2 = PendingManifest::new(SiteHashes::default());
        emit_redirect_table(&paths, &second_map, &TableInputs::default(), &output_dir, &mut pending2).unwrap();
        pending2.seal();
        let html2 = std::fs::read_to_string(&stub_path).unwrap();
        assert!(
            html2.contains("url=/newer/page/"),
            "a changed article map must re-emit the stub with the new target, got: {html2}"
        );
        assert_ne!(html1, html2);
    }
}
