//! Asset copying, cleanup, and synchronization pipeline.
//!
//! This module handles the asset management side of the build process:
//! - Stage-write helpers that return a manifest receipt
//! - Recursive directory copying
//! - Background asset copying via CAS (content-addressed storage)
//! - Stale file and directory cleanup
//!
//! ## One output directory
//!
//! Everything writes to the stage. The generation directory is built from the
//! sealed manifest by `materialize_and_promote` after the build, so nothing
//! here mirrors to a second location — the `canonical_dir` parameter that used
//! to do that had been a literal `None` in production since generations landed.
//!
//! ## Stale Cleanup
//!
//! `copy_deferred_assets()` does NOT delete anything from disk. It prunes its
//! own in-memory bookkeeping — stale `site_hashes.files`/`sources` entries for
//! keys the walk did not (re)claim — and sends the survivors to the
//! coordinator, which seals them into the manifest. Removing files and empty
//! directories that the sealed manifest no longer names, and writing the
//! final `hashes.json`, both happen later, only through the build's permitted
//! sweep (`pipeline::sweep_staging`), never from this worker.

use crate::moss_paths::MossPaths;
use crate::types::{content::SiteHashes, services::BackgroundContext};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tokio::sync::mpsc;

use crate::build::coordinator::EmitMessage;
use crate::build::manifest::HashBucket;
use crate::build::progress::PipelineEvent;

/// Summary returned by `copy_deferred_assets` so callers (and tests) can
/// observe cache effectiveness without parsing log lines. Production
/// callers can ignore the value; the regression test in build.rs asserts
/// against `cache_hits` to lock in the warm-cache fast-path.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct AssetPipelineRunStats {
    pub copied: u32,
    pub skipped: u32,
    pub cache_hits: u32,
    pub cache_misses: u32,
    /// Number of symlinks (and macOS Finder Aliases) that were silently
    /// skipped. Variants: broken target, escape, absolute-path target,
    /// processable extension (.md/.ipynb/.qmd), unsupported platform.
    /// Zero when all symlinks were either preserved or absent.
    pub skipped_symlinks: u32,
    /// Number of symlinks recreated verbatim in the output.
    pub preserved_symlinks: u32,
    /// Number of macOS Finder Aliases resolved and written as symlinks.
    pub preserved_aliases: u32,
}

/// The one-line-per-run asset summary. Split out from the `log::info!` call so
/// a test can assert what the line actually says.
///
/// A site that links its photo library in through a symlink or a Finder Alias
/// is an unusual layout, and it is the first thing worth knowing when that
/// site reports missing images. The per-item lines are DEBUG (they were a
/// flood on a large tree), so if this line does not carry the counts, an
/// uploaded log has no trace of the symlink at all. Preserved counts appear
/// only when non-zero: on the ordinary
/// site they are always 0 and would be two dead clauses on every build.
pub(crate) fn format_asset_run_summary(stats: &AssetPipelineRunStats) -> String {
    let mut line = format!(
        "[background-assets] Complete: {} copied, {} skipped (cache: {} hits, {} misses), {} skipped",
        stats.copied,
        stats.skipped,
        stats.cache_hits,
        stats.cache_misses,
        count_noun(stats.skipped_symlinks, "symlink", "symlinks"),
    );
    if stats.preserved_symlinks > 0 {
        line.push_str(&format!(
            ", {} preserved",
            count_noun(stats.preserved_symlinks, "symlink", "symlinks"),
        ));
    }
    if stats.preserved_aliases > 0 {
        line.push_str(&format!(
            ", {} resolved to {}",
            count_noun(stats.preserved_aliases, "alias", "aliases"),
            if stats.preserved_aliases == 1 { "a symlink" } else { "symlinks" },
        ));
    }
    line
}

/// `1 symlink` / `2 symlinks` — pick the real singular instead of writing
/// `symlink(s)`. Same rule the symlink-skip advisory already follows
/// (`make_symlink_skip_advisory` in `build/progress.rs`): a log line a user
/// may read should not look machine-generated.
fn count_noun(n: u32, singular: &str, plural: &str) -> String {
    format!("{n} {}", if n == 1 { singular } else { plural })
}

/// Source-metadata cache fast-path tolerance for the racy-mtime trick.
///
/// Any source file whose mtime is within this many seconds of the previous
/// hashes.json's own mtime is treated as suspect and re-hashed. The previous
/// hashes file was written atomically AFTER its source files were last
/// stat'd, so any in-place edit during the timer-resolution window will
/// produce mtime ≥ hashes.json mtime — and the re-hash will catch it.
///
/// 1 second is the conservative choice for HFS+ / older NTFS / coarse
/// network filesystems. APFS and ext4 have nanosecond resolution so the
/// window is much smaller in practice.
///
/// Trade-off accepted: an out-of-band copy that preserves mtime+size
/// (e.g. `cp -p` replacing a file with same-sized different content while
/// also setting mtime older than the cache record) ships a stale hash.
/// The next real edit corrects it. See the design discussion in the commit
/// that introduced this cache (perf/asset-cache).
const RACY_MTIME_EPSILON_SECS: u64 = 1;

/// Source-metadata cache hit rule. Returns `Some(cached_oid)` if the cache
/// record matches the current file (size + mtime) AND is not within the
/// racy-mtime window relative to `prev_hashes_mtime_secs`. Otherwise
/// returns `None` and the caller must re-hash the file.
///
/// Pure function — no I/O, no globals — so it's straightforward to unit-test.
fn check_source_cache(
    cached: Option<&crate::build::types::SourceMetadata>,
    current_size: u64,
    current_mtime_secs: u64,
    prev_hashes_mtime_secs: u64,
) -> Option<String> {
    let entry = cached?;
    if entry.size != current_size {
        return None;
    }
    if entry.mtime != current_mtime_secs {
        return None;
    }
    // Racy-mtime: the file's mtime is suspiciously close to (or after) when
    // we wrote the previous hashes. An in-place edit could have happened
    // between cache write and the OS-recorded mtime ticking forward.
    // Re-hash to be safe.
    if prev_hashes_mtime_secs == 0 {
        // Unknown previous-write timestamp — be safe, re-hash. This only
        // hits the very first build (no on-disk hashes.json yet); after
        // that hashes.json always has a real mtime.
        return None;
    }
    if current_mtime_secs + RACY_MTIME_EPSILON_SECS > prev_hashes_mtime_secs {
        return None;
    }
    Some(entry.hash.clone())
}

/// Copy a file into the stage, returning its **receipt**: the xxh3 the manifest
/// would record for the bytes written.
///
/// The hash is taken from `source`, never from the destination. Reading back
/// what moss just wrote under `.moss/build.nosync/` is the failure this whole design
/// exists to delete: the destination can be a cloud-evicted placeholder, and a
/// read of one returns `EDEADLK` under the dataless fail-fast policy. The
/// source is an input, so waiting on it is meaningful; the destination is not.
///
/// `relative_path` is `&ServedPath` (not `&str`) so the type system enforces
/// that every stage emit goes through a `ServedPath` constructor before the
/// path can be joined onto the stage dir. See `build::served_path`.
///
/// For generated content, use [`stage_write`] — it has the bytes already and
/// needs no second read.
pub(crate) fn stage_copy(
    source: &std::path::Path,
    relative_path: &crate::build::served_path::ServedPath,
    staging_dir: &std::path::Path,
) -> Result<String, String> {
    let staging_target = staging_dir.join(relative_path.as_str());
    crate::build::io_utils::copy_output(source, &staging_target)
        .map_err(|e| format!("Failed to copy to staging {}: {}", relative_path, e))?;
    crate::build::assets::paths::compute_binary_hash_file(source)
}

/// Write generated content into the stage, returning its receipt.
pub(crate) fn stage_write(
    content: &str,
    relative_path: &crate::build::served_path::ServedPath,
    staging_dir: &std::path::Path,
) -> Result<String, String> {
    let staging_target = staging_dir.join(relative_path.as_str());
    crate::build::io_utils::write_output(&staging_target, content.as_bytes())
        .map_err(|e| format!("Failed to write to staging {}: {}", relative_path, e))?;
    Ok(crate::build::assets::paths::compute_binary_hash(content.as_bytes()))
}

/// Recursively copy a directory tree, returning one receipt per file copied:
/// the `/`-separated path relative to `dst`, and the xxh3 of the bytes written.
///
/// Hashes come from the source side for the same reason as [`stage_copy`].
/// Separators are normalized here rather than at the registration site,
/// because `ServedPath::for_jupyterlite_asset` preserves its input verbatim —
/// a Windows `\` would reach the manifest and never match the `/`-form keys
/// the stale-cleanup protected set compares against.
pub(crate) fn copy_dir_recursive(
    src: &std::path::Path,
    dst: &std::path::Path,
) -> Result<Vec<(String, String)>, String> {
    let mut receipts = Vec::new();
    copy_tree(src, dst, "", &mut receipts)?;
    Ok(receipts)
}

fn copy_tree(
    src: &std::path::Path,
    dst: &std::path::Path,
    prefix: &str,
    receipts: &mut Vec<(String, String)>,
) -> Result<(), String> {
    crate::build::io_utils::create_output_dir_all(dst).map_err(|e| format!("Failed to create {}: {}", dst.display(), e))?;

    for entry in fs::read_dir(src)
        .map_err(|e| format!("Failed to read {}: {}", src.display(), e))?
    {
        let entry = entry.map_err(|e| format!("Failed to read dir entry: {}", e))?;
        let src_path = entry.path();
        let name = entry.file_name();
        // The receipt name and the written name must be the same string, or the
        // manifest lists a key no file matches. Lossy conversion is what can
        // make them differ, so the write takes the lossy form too — a non-UTF-8
        // filename could not be served under any URL regardless.
        let name = name.to_string_lossy();
        let dst_path = dst.join(name.as_ref());

        if src_path.is_dir() {
            copy_tree(&src_path, &dst_path, &format!("{prefix}{name}/"), receipts)?;
        } else {
            crate::build::io_utils::copy_output(&src_path, &dst_path)
                .map_err(|e| format!("Failed to copy {} to {}: {}", src_path.display(), dst_path.display(), e))?;
            let hash = crate::build::assets::paths::compute_binary_hash_file(&src_path)?;
            receipts.push((format!("{prefix}{name}"), hash));
        }
    }

    Ok(())
}

/// Check if a file extension is a supported image format.
pub(crate) fn is_image_extension(ext: &str) -> bool {
    matches!(ext, "jpg" | "jpeg" | "png" | "gif" | "webp" | "svg" | "avif")
}

/// True when `mapped_path` looks like a bundled-SPA entry — an `index.html`
/// inside a subfolder. Root-level `index.html` is generated by the moss
/// blocking phase from `index.md` (and is in `blocking_keys`, so we never
/// reach this point for it). Restricting to subfolders is a belt-and-braces
/// guard against ever double-tagging a moss-templated page.
fn is_bundled_spa_index_path(mapped_path: &str) -> bool {
    let p = mapped_path.trim_start_matches('/');
    // Must contain a slash AND end with "/index.html".
    p.ends_with("/index.html") && p.matches('/').count() >= 1
}

/// Build a per-folder canonical URL string from the SPA defaults' site host
/// and the SPA's output path. Returns `None` when no host is configured (the
/// site hasn't set `site_url`); the injector then skips the canonical link
/// without affecting other tags.
fn derive_canonical_url(
    defaults: &crate::build::site_meta::spa_inject::SpaDefaultsOwned,
    mapped_path: &str,
) -> Option<String> {
    let host = defaults.site_host.as_deref()?;
    // mapped_path is "<folder>/index.html" — homepage ("index.html") bypasses
    // canonical_url() because ServedPath rejects empty input.
    if mapped_path == "index.html" {
        let scheme = "https";
        let prefix = if defaults.site_www_canonical && !host.starts_with("www.") {
            format!("www.{}", host)
        } else {
            host.to_string()
        };
        return Some(format!("{}://{}/", scheme, prefix));
    }
    let page_path = crate::build::served_path::ServedPath::from_source(mapped_path)
        .map_err(|e| log::warn!("derive_canonical_url: '{}' rejected by ServedPath: {}", mapped_path, e))
        .ok()?;
    Some(crate::build::page::canonical::canonical_url(
        host,
        &page_path,
        defaults.site_www_canonical,
    ))
}

/// Read `target`, run the injector, and (if the result differs) write back
/// the new content. Returns `Ok(Some(xxh3_hex))` when the file was
/// rewritten so the caller can update `hashes.files`. `Ok(None)` means the
/// SPA already had every default tag and nothing changed on disk.
///
/// The returned hash is an xxh3 (16-hex) digest matching `compute_binary_hash`,
/// so that `verify_file_bytes` in `deploy.rs` can round-trip successfully.
///
/// Replaces the hardlink with a regular file: writes to a temp path then
/// renames over the target so a partial write can never leave a half-injected
/// file in place.
fn maybe_inject_spa(
    target: &std::path::Path,
    defaults: &crate::build::site_meta::spa_inject::SpaDefaults<'_>,
) -> Result<Option<String>, String> {
    let original = std::fs::read_to_string(target)
        .map_err(|e| format!("read {}: {}", target.display(), e))?;
    let injected = crate::build::site_meta::spa_inject::inject_defaults(&original, defaults)?;
    if injected == original {
        return Ok(None);
    }
    // `write_output` renames a fresh inode into place, so the write cannot
    // propagate into the CAS-stored blob through a shared hardlink inode — the
    // explicit `remove_file` this used to need is now the primitive's job.
    crate::build::io_utils::write_output(target, injected.as_bytes())
        .map_err(|e| format!("write {}: {}", target.display(), e))?;
    let hash = crate::build::assets::paths::compute_binary_hash(injected.as_bytes());
    Ok(Some(hash))
}

/// Transform name used for cached SPA-injection output in the TransformCache.
const SPA_INJECT_TRANSFORM: &str = "spa/inject";

/// Cached record for a `spa/inject` transform: either the injected output's
/// content-store OID + returned hash, or (when injection was a no-op —
/// `maybe_inject_spa` returned `Ok(None)`) both `None`.
#[derive(serde::Serialize, serde::Deserialize)]
struct SpaInjectRecord {
    content_oid: Option<String>,
    xxh3: Option<String>,
}

/// Build the TransformCache `params` for a `spa/inject` cache entry.
///
/// Every field `inject_defaults` reads must be represented here, INCLUDING
/// `canonical_url` — it's assembled per SPA-folder outside `SpaDefaultsOwned`
/// (see that struct's doc comment), so two different SPA subfolders sharing
/// the same site-wide defaults would alias the same cache entry if it were
/// omitted.
fn spa_inject_params(defaults: &crate::build::site_meta::spa_inject::SpaDefaults<'_>) -> serde_json::Value {
    serde_json::json!({
        "description": defaults.description,
        "canonical_url": defaults.canonical_url,
        "og_tags": defaults.og_tags,
        "twitter_tags": defaults.twitter_tags,
        "apple_touch_icon": defaults.apple_touch_icon,
        "raster_favicons": defaults.raster_favicons,
        "theme_color_light": defaults.theme_color_light,
        "theme_color_dark": defaults.theme_color_dark,
    })
}

/// Read a cached `spa/inject` record, verifying the inner injected-content
/// blob (if any) still exists — `find_cached_output` only verifies the
/// record's OWN blob, not blobs referenced from inside it.
fn read_spa_inject_record(
    object_store: &crate::build::cache::ObjectStore,
    record_oid: &str,
) -> Option<SpaInjectRecord> {
    let blob_path = object_store.get_path(record_oid)?;
    let raw = std::fs::read(blob_path).ok()?;
    serde_json::from_slice(&raw).ok()
}

/// Write a `spa/inject` transform record, merging with any existing record
/// for `source_oid` so other transforms on the same source (unlikely for
/// HTML today, but matches the established `write_cached_meta` pattern)
/// aren't clobbered.
fn write_spa_inject_record(
    object_store: &crate::build::cache::ObjectStore,
    transform_cache: &crate::build::cache::TransformCache,
    source_oid: &str,
    source_size: u64,
    params: &serde_json::Value,
    record: &SpaInjectRecord,
) {
    let json_bytes = match serde_json::to_vec(record) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("Failed to serialize spa/inject record: {}", e);
            return;
        }
    };
    let record_oid = match object_store.store_bytes(&json_bytes) {
        Ok(oid) => oid,
        Err(e) => {
            log::warn!("Failed to store spa/inject record blob: {}", e);
            return;
        }
    };
    let entry = crate::build::cache::TransformEntry {
        oid: record_oid,
        size: json_bytes.len() as u64,
        params: params.clone(),
    };
    let mut rec = transform_cache
        .get(source_oid)
        .unwrap_or_else(|| crate::build::cache::TransformRecord {
            source_oid: source_oid.to_string(),
            source_size,
            transforms: std::collections::HashMap::new(),
        });
    rec.transforms.insert(SPA_INJECT_TRANSFORM.to_string(), entry);
    if let Err(e) = transform_cache.put(&rec) {
        log::warn!("Failed to write spa/inject transform record: {}", e);
    }
}

/// Cache-aware wrapper around `maybe_inject_spa`.
///
/// `source_oid` is the already-verified content hash of `target`'s CURRENT
/// bytes (the raw source — `link_to` hardlinks it into `target` immediately
/// before this runs, so a source-unchanged cache hit alone does NOT imply
/// `target` already holds injected content; the cache below stores the
/// INJECTED OUTPUT, not just an "unchanged" bit, precisely to avoid that
/// trap).
///
/// On a `TransformCache` hit, `link_to`s the previously-computed injected
/// blob straight into `target` (or, if the record says injection was a
/// no-op, leaves `target` as the raw-linked bytes it already has) — zero
/// read/inject/diff. On a miss, runs `maybe_inject_spa` once and stores the
/// result so future builds with an unchanged source AND unchanged
/// `defaults` hit the fast path — including a second call for the
/// canonical-dir mirror in the same build, which becomes a cache hit
/// instead of a second independent read+inject+write.
///
/// Returns the injected content's `(xxh3, content_oid)` together. The caller
/// used to receive only the hash and reuse the PRE-injection `link_oid` it
/// already had for the manifest's `staged_oid` — the two silently diverged the
/// moment injection actually changed the bytes, since `link_to` above places
/// the pre-injection blob and this function then REPLACES `target`'s bytes
/// with a freshly-minted post-injection object. Returning the pair is what
/// lets the caller thread the object that's actually on disk into the
/// manifest instead of the one that used to be there.
fn maybe_inject_spa_cached(
    target: &Path,
    defaults: &crate::build::site_meta::spa_inject::SpaDefaults<'_>,
    source_oid: &str,
    source_size: u64,
    object_store: &crate::build::cache::ObjectStore,
    transform_cache: &crate::build::cache::TransformCache,
) -> Result<Option<(String, String)>, String> {
    let params = spa_inject_params(defaults);

    if let Some(record_oid) =
        transform_cache.find_cached_output(source_oid, SPA_INJECT_TRANSFORM, &params)
    {
        if let Some(record) = read_spa_inject_record(object_store, &record_oid) {
            match record.content_oid {
                Some(content_oid) if object_store.get_path(&content_oid).is_some() => {
                    object_store.link_to(&content_oid, target)?;
                    // Constructed as both-Some or both-None (see below and the
                    // miss arm) — a `None` here means the record was corrupted
                    // by something other than this function, which no amount
                    // of a silent fallback would make correct to ship under.
                    let xxh3 = record.xxh3.expect(
                        "SpaInjectRecord invariant: content_oid and xxh3 are both Some or both None",
                    );
                    return Ok(Some((xxh3, content_oid)));
                }
                None => return Ok(None),
                Some(_) => {} // inner blob GC'd — fall through to a live re-run
            }
        }
        // Record present but unreadable/corrupt — fall through as well.
    }

    let result = maybe_inject_spa(target, defaults)?;
    let (returned, record) = match &result {
        Some(new_hash) => {
            let injected_bytes = std::fs::read(target)
                .map_err(|e| format!("read back {} after inject: {}", target.display(), e))?;
            let content_oid = object_store.store_bytes(&injected_bytes)?;
            (
                Some((new_hash.clone(), content_oid.clone())),
                SpaInjectRecord {
                    content_oid: Some(content_oid),
                    xxh3: Some(new_hash.clone()),
                },
            )
        }
        None => (
            None,
            SpaInjectRecord {
                content_oid: None,
                xxh3: None,
            },
        ),
    };
    write_spa_inject_record(object_store, transform_cache, source_oid, source_size, &params, &record);

    Ok(returned)
}

/// Remove generated HTML pages (index*.html) not produced by the current build.
///
/// Runs on staging in the blocking phase, just before the preview is pointed at this render,
/// so it stops serving pages whose source was deleted. Source HTML assets (interactive embeds
/// like `sketch.html`) are preserved — generated pages always use `index*.html`.
///
/// Staging is served, so this needs a `lifecycle::SweepPermit`.
pub(crate) fn remove_stale_html(
    dir: &Path,
    blocking_keys: &std::collections::HashSet<String>,
    notebook_outputs: &std::collections::HashSet<String>,
    _permit: &crate::build::lifecycle::SweepPermit,
) {
    use walkdir::WalkDir;
    let mut removed = 0u32;
    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let filename = match entry.path().file_name().and_then(|n| n.to_str()) {
            Some(f) => f,
            None => continue,
        };
        // Only target generated pages: index.html, index-2.html, etc.
        // Source HTML assets (interactive embeds) use other names.
        if !filename.starts_with("index") || !filename.ends_with(".html") {
            continue;
        }
        if let Ok(rel) = entry.path().strip_prefix(dir) {
            // Normalize `\`→`/`: keys are compared against `/`-form sets.
            let key = moss_core::slug::normalize_separators(&rel.to_string_lossy());
            // blocking_keys covers this build's blocking-phase pages;
            // notebook_outputs covers background-phase JupyterLite assets
            // (jupyter/{lab,tree,doc,...}/index.html) carried forward from
            // the previous build — they'd otherwise match the generated-
            // page pattern and be deleted before notebook processing
            // re-registers them.
            if !blocking_keys.contains(&key) && !notebook_outputs.contains(&key) {
                let _ = fs::remove_file(entry.path());
                log::debug!("[stale-html] Removed: {}", key);
                removed += 1;
            }
        }
    }
    if removed > 0 {
        log::info!("[stale-html] Removed {} stale page(s) from {}", removed, dir.display());
    }
}

/// What one stale-file pass removed, counted by kind for the one INFO line a
/// sweep writes — never one line per file, so it stays readable in a storm.
#[derive(Debug, Default)]
pub(crate) struct SweepReport {
    pub html: usize,
    pub webp: usize,
    pub video: usize,
    pub symlink: usize,
    pub other: usize,
    /// The first few keys removed, so a log names what went, not just how many.
    pub sample: Vec<String>,
}

impl SweepReport {
    fn record(&mut self, key: &str, is_symlink: bool) {
        let ext = key.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        match ext.as_str() {
            _ if is_symlink => self.symlink += 1,
            "html" => self.html += 1,
            "webp" => self.webp += 1,
            "mp4" | "m3u8" | "ts" | "m4s" => self.video += 1,
            _ if key.ends_with(".thumb.jpg") => self.video += 1,
            _ => self.other += 1,
        }
        if self.sample.len() < 3 {
            self.sample.push(key.to_string());
        }
    }

    pub fn removed(&self) -> usize {
        self.html + self.webp + self.video + self.symlink + self.other
    }

    /// `removed <n> file(s) (<h> html, <w> webp, <v> video, <l> symlink, <o> other; e.g. <keys>)`
    pub fn describe(&self) -> String {
        format!(
            "removed {} file(s) ({} html, {} webp, {} video, {} symlink, {} other; e.g. {})",
            self.removed(),
            self.html,
            self.webp,
            self.video,
            self.symlink,
            self.other,
            self.sample.join(", ")
        )
    }
}

/// Remove files and symlinks from `dir` that `site_hashes` does not name.
///
/// Staging is served, so this needs a `lifecycle::SweepPermit`. A symlink is
/// unlinked as a link and never descended into (`WalkDir` does not follow
/// links), so an alias to a kept directory cannot take its contents with it.
pub(crate) fn remove_stale_files(
    dir: &Path,
    site_hashes: &SiteHashes,
    label: &str,
    permit: &crate::build::lifecycle::SweepPermit,
) -> SweepReport {
    use walkdir::WalkDir;
    let mut report = SweepReport::default();
    for entry in WalkDir::new(dir).into_iter() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let is_symlink = entry.path_is_symlink();
        if !entry.file_type().is_file() && !is_symlink {
            continue;
        }
        if let Ok(rel) = entry.path().strip_prefix(dir) {
            // Normalize `\`→`/`: key is compared against the `/`-form
            // `site_hashes` maps/sets (video/image/notebook outputs + files).
            let key = moss_core::slug::normalize_separators(&rel.to_string_lossy());

            // A running or finished encode's output, or a temp it is writing
            // through, until a build registers it.
            if permit.keeps(&key) {
                continue;
            }

            // Always unlink orphan *.placeholder.svg. An earlier change removed
            // Pattern E (per-video .placeholder.svg generation), but vaults built
            // before that change have these files on disk AND entries in hashes.json#files for
            // them — so the standard "not in files → remove" check below
            // would preserve them indefinitely. Current code never produces
            // .placeholder.svg, so they are always orphans.
            //
            // FIXME: revisit if Pattern E (or any future feature) starts
            // producing legitimate `.placeholder.svg` outputs again — this
            // sweep would silently delete them.
            if key.ends_with(".placeholder.svg") {
                let _ = fs::remove_file(entry.path());
                log::debug!(
                    "[background-assets] Removed orphan placeholder svg from {}: {}",
                    label, key
                );
                report.record(&key, false);
                continue;
            }

            // Email/RSS math PNGs are APPEND-ONLY: their URLs
            // are baked into already-sent emails and cached forever by
            // Gmail's proxy / Apple MPP, so a PNG whose equation was edited
            // or deleted must keep serving its original bytes. Never stale.
            // Only the `.pending.` temp a crashed write left there is.
            if key.starts_with(crate::build::emit::math_png::MATH_PNG_PREFIX) {
                if key.contains(".pending.") {
                    let _ = fs::remove_file(entry.path());
                    report.record(&key, false);
                }
                continue;
            }
            if site_hashes.video_outputs.contains(&key) {
                continue;
            }
            // Parallel to video_outputs: generated .webp files have no
            // source entry in `files`, so stale cleanup would delete them.
            if site_hashes.image_outputs.contains(&key) {
                continue;
            }
            // Same for notebook outputs: JupyterLite assets, viewer HTML
            // wrappers, and .ipynb copies are produced in the background
            // phase from .ipynb sources.
            if site_hashes.notebook_outputs.contains(&key) {
                continue;
            }
            if !site_hashes.files.contains_key(&key) {
                let _ = fs::remove_file(entry.path());
                log::trace!("[background-assets] Removed stale from {}: {}", label, key);
                report.record(&key, is_symlink);
            }
        }
    }
    report
}

/// Remove directories from `dir` not in `expected_dirs`, deepest-first.
/// Returns how many it removed. Staging is served, so this needs a
/// `lifecycle::SweepPermit`.
pub(crate) fn remove_stale_dirs(
    dir: &Path,
    expected_dirs: &std::collections::HashSet<std::path::PathBuf>,
    permit: &crate::build::lifecycle::SweepPermit,
) -> usize {
    use walkdir::WalkDir;
    let mut removed = 0usize;
    let mut actual_dirs: Vec<std::path::PathBuf> = Vec::new();
    for entry in WalkDir::new(dir).into_iter() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_dir() {
            continue;
        }
        if entry.path() == dir {
            continue;
        }
        if let Ok(rel) = entry.path().strip_prefix(dir) {
            actual_dirs.push(rel.to_path_buf());
        }
    }
    actual_dirs.sort_by(|a, b| b.components().count().cmp(&a.components().count()));
    for d in &actual_dirs {
        // `_moss/math/` is append-only — a build whose pages
        // no longer carry math computes no expected entry for it, but the
        // PNGs inside are referenced by already-sent emails.
        if d.starts_with("_moss/math") {
            continue;
        }
        if !expected_dirs.contains(d) && !permit.keeps_dir(d) {
            let abs = dir.join(d);
            match crate::build::io_utils::remove_output_dir_all(&abs) {
                Ok(_) => {
                    removed += 1;
                    log::debug!(
                        "[stale-cleanup] Removed stale directory from {}: {}",
                        dir.display(),
                        d.display()
                    )
                }
                Err(e) => log::debug!(
                    "[stale-cleanup] Could not remove {} from {}: {}",
                    d.display(),
                    dir.display(),
                    e
                ),
            }
        }
    }
    removed
}

/// Compute expected directories from site_hashes keys (file parents + video_output parents + image_output parents).
pub(crate) fn compute_expected_dirs(site_hashes: &SiteHashes) -> std::collections::HashSet<std::path::PathBuf> {
    let mut expected_dirs = std::collections::HashSet::new();
    for key in site_hashes
        .files
        .keys()
        .chain(site_hashes.video_outputs.iter())
        .chain(site_hashes.image_outputs.iter())
        .chain(site_hashes.notebook_outputs.iter())
    {
        let p = std::path::Path::new(key);
        let mut current = p.parent();
        while let Some(dir) = current {
            if dir == std::path::Path::new("") {
                break;
            }
            expected_dirs.insert(dir.to_path_buf());
            current = dir.parent();
        }
    }
    expected_dirs
}

/// True when the background image converter OWNS a webp source's resized base
/// (`copy_deferred_assets` must then SKIP the verbatim copy, else the two writers
/// collide on the same staged `photo.webp` — see the guard's call site).
///
/// The verdict MUST agree byte-for-byte with `collect_images_for_conversion`
/// (the authority), which fed `should_skip` the scan-time `media_meta.size`.
/// `size` is the fresh metadata read: `Some(len)` on success, `None` when the
/// read fails mid-build (source vanished / iCloud dataless / transient stat
/// error). On a read failure copy_deferred cannot recover the authority's scan
/// size, so it fails SAFE toward "converter owns the base" rather than defaulting
/// to `0` — a fabricated `0` is always `< min_size_kb` and flips a small-DIMENSION
/// webp to `AlreadySmall` → verbatim copy, re-introducing the Task-12.6
/// double-writer / full-res clobber. A converted webp whose source is truly
/// unreadable fail-encodes → `set_failed` → warning SVG; never a double write or
/// a full-res clobber. (Design follow-up #6: harden the `.unwrap_or(0)` TOCTOU.)
///
/// `source_oid` is the caller's cheap stat-match resolution (empty string on a
/// miss) — passed through so this call and `collect_images_for_conversion`'s
/// call consult the SAME `should_skip` format-probe cache entry instead of
/// each recomputing the expensive probes independently.
fn webp_converter_owns_base(
    source_path: &Path,
    ext: &str,
    size: Option<u64>,
    config: &crate::build::media::image::ImageCompressionConfig,
    transforms: &crate::build::cache::TransformCache,
    source_oid: &str,
) -> bool {
    match size {
        Some(len) => crate::build::media::image::should_skip(
            source_path,
            ext,
            len,
            config,
            transforms,
            source_oid,
            // A source still in the cloud is filtered out of the encoder's item
            // list (`render/blocking.rs`), so the converter demonstrably does
            // NOT own this base — `SourceInTheCloud` makes `is_none()` false,
            // which is the answer that lets the loop below copy the verbatim
            // blob it already has. Answering "the converter owns it" would
            // leave the base with no writer at all and 404 it.
            crate::build::icloud::is_evicted(source_path),
        )
        .is_none(),
        // Unreadable source → fail safe: let the converter own it (skip verbatim).
        None => true,
    }
}

/// Resolve the xxh3 manifest hash of a just-linked output file, consulting
/// `memo` before reading `target`. `memo_key` is the oid of the CAS blob that
/// was `link_to`'d into `target` (NOT necessarily the source oid — see the
/// sized-raster case at the source-asset call site), because that blob's
/// bytes are what `target` actually holds.
///
/// On a hash failure, returns `fallback` (the CAS-oid stand-in that keeps the
/// asset in the manifest instead of dropping it — see the long comment at
/// each call site) and deliberately does NOT write `fallback` into `memo`:
/// memoizing a fallback would let one transient read failure (e.g. an iCloud
/// eviction race) serve a wrong hash forever instead of self-healing on the
/// next build.
fn recall_or_hash_output(
    memo: &crate::build::media::manifest_hash_memo::ManifestHashMemo,
    memo_key: &str,
    target: &Path,
    fallback: &str,
    log_context: &str,
) -> String {
    if let Some(hash) = memo.get(memo_key) {
        return hash;
    }
    match crate::build::assets::paths::compute_binary_hash_file(target) {
        Ok(hash) => {
            memo.record(memo_key, hash.clone());
            hash
        }
        Err(e) => {
            log::warn!(
                "[background-assets] hashing {} '{}' failed ({}); keeping asset in manifest with CAS-oid fallback",
                log_context,
                target.display(),
                e
            );
            fallback.to_string()
        }
    }
}

/// Register a `Preserved` symlink/alias outcome in `live_symlinks` and the
/// manifest; any other outcome is already logged by `handle_symlink_entry` /
/// `handle_alias_entry`. Returns whether it was `Preserved`, so the caller
/// can credit its own counter — a real symlink and a resolved Finder alias
/// count separately even though they register identically. `kind` names the
/// entry in an invalid-path warning ("symlink" or "alias").
fn preserve_link(
    outcome: &crate::build::media::symlink::SymlinkOutcome,
    site_hashes: &mut SiteHashes,
    live_symlinks: &mut std::collections::HashSet<String>,
    kind: &str,
) -> bool {
    use crate::build::media::symlink::SymlinkOutcome;
    let SymlinkOutcome::Preserved { rel_path, target } = outcome else {
        return false;
    };
    live_symlinks.insert(rel_path.clone());
    // Register in the manifest so deploy uploads the symlink.
    let target_str = target.to_string_lossy();
    match crate::build::served_path::ServedPath::from_source(rel_path) {
        Ok(out_path) => {
            site_hashes.insert_file_hash(&out_path, crate::types::content::symlink_entry(&target_str));
        }
        Err(e) => {
            log::warn!("[background-assets] Skipping {kind} with invalid path '{}': {}", rel_path, e);
        }
    }
    true
}

/// `place_blob`'s inputs, grouped so both call sites below name each field
/// instead of relying on position — the main-walk call repeats one
/// expression for both `log_label` and `report_path`.
struct BlobPlacement<'a> {
    link_oid: &'a str,
    target: &'a Path,
    log_label: &'a str,
    report_path: &'a str,
    ext: &'a str,
}

/// Link the blob into output and, for an image extension, report
/// `AssetReady` — shared by the main asset walk and the `.moss/theme`
/// mirror below. `false` (already logged) on a link failure.
fn place_blob(
    object_store: &crate::build::cache::ObjectStore,
    placement: BlobPlacement,
    reporter: &dyn crate::build::ports::reporter::BuildReporter,
) -> bool {
    if let Err(e) = object_store.link_to(placement.link_oid, placement.target) {
        log::warn!("[background-assets] Failed to link {}: {}", placement.log_label, e);
        return false;
    }
    if is_image_extension(placement.ext) {
        reporter.report(&PipelineEvent::AssetReady {
            path: placement.report_path.to_string(),
            asset_type: "image".to_string(),
        });
    }
    true
}

/// `record_blob`'s hash source: a value already computed byte-exactly (SPA
/// post-injection — the memo would answer for the wrong, pre-injection
/// blob), or the inputs to resolve one via `recall_or_hash_output`. A type,
/// not four more `&str` params dead under `Known` and, at the theme call
/// site, three positional copies of the same `oid` with nothing to catch a swap.
enum OutputHash<'a> {
    Known(String),
    Compute {
        memo_key: &'a str,
        target: &'a Path,
        fallback_oid: &'a str,
        log_context: &'a str,
    },
}

/// Resolve `hash` and register it, plus the staged CAS object id, for a blob
/// `place_blob` already linked into `out_path`'s target.
///
/// This records the staged, PRE-`apply_transform` bytes' hash — for an HTML
/// entry, `ship::verify_ship_integrity` compares POST-transform bytes
/// instead. Benign: an entry here normally carries a live `staged_oid`,
/// which skips that check entirely, and the check is fail-open/log-only on
/// the rare entry that does reach it.
///
/// A hash failure registers `fallback_oid` rather than panicking (this runs
/// inside a `spawn_blocking` worker, where a panic is swallowed) and rather
/// than skipping, which would leave a linked-but-unregistered blob for
/// `remove_stale_files` to delete as stale.
fn record_blob(
    manifest_hash_memo: &crate::build::media::manifest_hash_memo::ManifestHashMemo,
    site_hashes: &mut SiteHashes,
    staged_oids: &mut HashMap<String, String>,
    hash: OutputHash,
    out_path: &crate::build::served_path::ServedPath,
    staged_oid: &str,
) {
    let hash = match hash {
        OutputHash::Known(h) => h,
        OutputHash::Compute { memo_key, target, fallback_oid, log_context } => {
            recall_or_hash_output(manifest_hash_memo, memo_key, target, fallback_oid, log_context)
        }
    };
    site_hashes.insert_file_hash(out_path, crate::types::content::file_entry(&hash));
    staged_oids.insert(out_path.as_str().to_string(), staged_oid.to_string());
}

/// After all assets are placed, stale cleanup runs to remove output files that
/// no longer have a corresponding source.
///
/// Asset registrations go out over `tx` as `EmitMessage { bucket: Files }`; the
/// coordinator owns the manifest and this function never writes `hashes.json`.
/// It reads `ctx.previous_hashes` for the entries it may carry forward and
/// `ctx.blocking_keys` to know which `.html` paths a rendered page already
/// claims — it does NOT hold a copy of this build's manifest.
/// AssetRegistry passed in so audio/PDF/static assets are registered
/// as Ready once they are copied to the output. The registry key is the mapped
/// output-relative path (e.g. `audio/talk.mp3`, `docs/paper.pdf`), matching the
/// URL the preview server normalises from the browser's request.
/// `None` disables registration (headless mode, tests that don't need it).
pub(crate) fn copy_deferred_assets(
    ctx: &BackgroundContext,
    reporter: &dyn crate::build::ports::reporter::BuildReporter,
    tx: mpsc::Sender<EmitMessage>,
    asset_registry: Option<std::sync::Arc<crate::types::assets::AssetRegistry>>,
) -> AssetPipelineRunStats {
    use crate::build::cache::ObjectStore;
    use crate::build::scan::classify::is_excluded_dir_name;
    use crate::build::render::resolve_path_with_overrides;
    use walkdir::WalkDir;

    let deferred_paths = MossPaths::from_moss_dir(ctx.moss_dir.clone());
    let object_store = ObjectStore::for_site(&deferred_paths);
    // See TODO(perf) at both call sites below: a CAS blob's bytes are a pure
    // function of its oid, so the xxh3 manifest hash of a just-linked output
    // file needs no staleness rule — only a cache keyed by oid.
    let manifest_hash_memo =
        crate::build::media::manifest_hash_memo::ManifestHashMemo::load(&deferred_paths.cache_manifest_hash_memo());
    // Sized-raster-original support (plan 2026-07-06): raster originals
    // (jpg/jpeg/png) are deployed as a sized/optimized raster at the SAME output
    // path instead of the full-resolution source, so no full-res original is
    // ever shipped. The output keeps the source format (JPEG stays JPEG, PNG
    // stays a transparency-preserving PNG). It is content-addressed and cached
    // (keyed by source hash + config) via this TransformCache, mirroring the
    // WebP pass.
    let transforms = crate::build::cache::TransformCache::for_site(&deferred_paths);
    let image_config = crate::build::media::image::ImageCompressionConfig::default();
    // Seeded from the PREVIOUS build's manifest, not from this build's.
    //
    // This walk owns exactly one slice of the manifest: static assets. It reads
    // the previous entries so an asset whose source is still on disk but was not
    // re-hashed this build (an iCloud stub, an evicted file) keeps its published
    // hash, and so anything the walk no longer finds is pruned below. Everything
    // else — rendered pages, OG cards, notebook outputs, image and video
    // variants — belongs to the `PendingManifest` the coordinator holds, which
    // already has this build's entries. Re-emitting those from a snapshot taken
    // at the end of the render phase is what forced three hand-written syncs to
    // keep the snapshot current, and would now overwrite a live entry with a
    // stale hash.
    let mut site_hashes = ctx.previous_hashes.clone();
    let source_root = Path::new(&ctx.source_path);
    let output_dir = &ctx.staging_dir;

    // Source-metadata fast-path: reuse a previously-computed `oid` when the
    // source file's (size, mtime) matches what we recorded last build. Avoids
    // re-reading + re-hashing every asset on every deploy. Persisted across
    // runs in `SiteHashes::sources` (key = source-relative path).
    //
    // Racy-mtime safety (Git's trick — see RACY_MTIME_EPSILON below): any
    // entry whose mtime is within EPSILON of the previous hashes.json write
    // time is treated as suspect and re-hashed. The previous hashes file is
    // written atomically AFTER the source files were last stat'd, so any
    // in-place edit within the timer-resolution window will be caught.
    let prev_sources = &ctx.previous_hashes.sources;
    let prev_hashes_mtime_secs: u64 = std::fs::metadata(deferred_paths.hashes())
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut cache_hits: u32 = 0;
    let mut cache_misses: u32 = 0;

    // Markdown extensions that were handled in the blocking phase
    let markdown_exts = ["md", "markdown", "mdown", "mkd"];
    // Video extensions — large files (≥100MB) handled by FFmpeg pipeline,
    // small files copied as regular assets (see size check below)
    let video_exts = ["mov", "mp4", "webm", "avi", "mkv", "m4v"];
    // HTML extensions — also generated in blocking phase
    let html_exts = ["html", "htm"];
    // Document types processed in blocking phase (rendered to HTML)
    let doc_exts = ["pages", "docx", "doc"];

    let mut copied = 0u32;
    let mut skipped = 0u32;
    let mut skipped_symlinks = 0u32;
    // Preserved symlinks / resolved aliases. Counted because the per-item log
    // line is DEBUG: without these the summary reports nothing.
    let mut preserved_symlinks = 0u32;
    // Only the macOS alias branch writes this; elsewhere it stays 0.
    #[allow(unused_mut)]
    let mut preserved_aliases = 0u32;
    // Track which asset keys the background walk actually found in the source.
    // After all walks, carried-forward entries NOT in this set are stale
    // (source file was deleted) and must be removed from site_hashes.
    let mut live_asset_keys = std::collections::HashSet::<String>::new();
    // Parallel set keyed by *source-relative* path (not output path) — used
    // to prune `site_hashes.sources` after the walk. Without this, deleted
    // / renamed source files leave entries in the metadata cache forever
    // and `hashes.json` grows unbounded over the project's lifetime.
    let mut live_source_keys = std::collections::HashSet::<String>::new();
    // Output path → the CAS object id already backing those exact staged
    // bytes, for every entry this walk registers via the `Ok(oid)` arm below
    // (assets) or the `.moss/theme` mirror. Threaded onto the re-emitted
    // `EmitMessage::File` at the end of this function so `ship_phase` can
    // copy straight from the CAS instead of the mutable stage path — see
    // `PendingManifest::ship_sources` (`ShipSource::Cas`). A symlink entry or
    // a direct-copy fallback never gets one, and ships exactly as before.
    let mut staged_oids: HashMap<String, String> = HashMap::new();

    // Symlinks preserved from source to output (also: Finder Aliases
    // resolved to symlinks on macOS). Tracked separately so
    // `remove_stale_symlinks` can prune entries removed between builds.
    //
    // INVARIANT: this single set is reused for stale cleanup of BOTH
    // `output_dir` (staging) because the symlink
    // branch dual-writes the same rel_paths to both. If a future change
    // populates the dirs asymmetrically (e.g. only writing to staging and
    // letting `ship_phase` mirror), `remove_stale_symlinks` may prune a live
    // symlink on the side that wasn't populated. Keep the dual-write.
    //
    // Symlinks are NOT entered into `live_asset_keys` / `live_source_keys`
    // — they have no content hash and aren't in the object-store cache.
    let mut live_symlinks = std::collections::HashSet::<String>::new();

    // Canonical source root, computed once for the containment check
    // inside `handle_symlink_entry`. Matches against canonical paths only.
    let canonical_source_root = match source_root.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            log::warn!("[background-assets] canonicalize source_root failed: {}; symlinks will not be preserved", e);
            source_root.to_path_buf()
        }
    };

    for entry in WalkDir::new(source_root)
        .into_iter()
        .filter_entry(|e| {
            // Only directories may be excluded by name; file entries (e.g.
            // `_43A2045.jpg`) must always pass through.
            if !e.file_type().is_dir() {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            !is_excluded_dir_name(&name)
        })
    {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                log::warn!("[background-assets] Walk error: {}", e);
                continue;
            }
        };

        // Symlink branch: preserve the symlink in the output instead of
        // copying bytes. Must run before the `is_file()` filter — symlinks
        // (whether to dirs or files) report `is_file() == false` under the
        // walker's default `follow_links(false)`.
        if entry.file_type().is_symlink() {
            use crate::build::media::symlink::handle_symlink_entry;
            let outcome = handle_symlink_entry(
                entry.path(),
                source_root,
                &canonical_source_root,
                output_dir,
            );
            if preserve_link(&outcome, &mut site_hashes, &mut live_symlinks, "symlink") {
                preserved_symlinks += 1;
            } else {
                skipped_symlinks += 1;
            }
            continue;
        }

        // Alias branch (macOS only): regular files that are Finder Bookmark
        // Aliases get resolved and written as POSIX symlinks in the output,
        // identical to how the symlink branch handles real symlinks.
        // Note: `ship_phase` (staging→site) sees the output as a real symlink and
        // handles it via its own symlink branch — no alias branch needed there.
        #[cfg(target_os = "macos")]
        if entry.file_type().is_file() {
            use crate::build::media::symlink::handle_alias_entry;
            if let Some(outcome) = handle_alias_entry(
                entry.path(),
                source_root,
                &canonical_source_root,
                output_dir,
            ) {
                // Count any non-Preserved alias outcome (escape/broken/
                // processable/absolute/unresolvable) as a skipped symlink.
                if preserve_link(&outcome, &mut site_hashes, &mut live_symlinks, "alias") {
                    preserved_aliases += 1;
                } else {
                    skipped_symlinks += 1;
                }
                continue;
            }
            // handle_alias_entry returns None ONLY for a genuine non-alias
            // file — fall through to regular-file handling. A bookmark whose
            // target won't resolve now returns Some(SkippedUnresolvable) (and
            // `continue`s above), so an unresolvable alias is never copied as
            // a raw blob here. See media/symlink.rs handle_alias_entry.
        }

        if !entry.file_type().is_file() {
            continue;
        }

        let file_path = entry.path();
        // Normalize `\`→`/`: relative_path feeds `/`-form comparisons (blocking
        // keys, prev-source asset cache, live-source prune map) — un-normalized
        // Windows backslashes miss every match (re-hash/re-copy + duplicate-copy).
        let relative_path = match file_path.strip_prefix(source_root) {
            Ok(rel) => moss_core::slug::normalize_separators(&rel.to_string_lossy()),
            Err(_) => continue,
        };

        // Pre-Sonoma eviction (macOS 12–13, still a supported minimum) replaces
        // the asset with a hidden `.name.icloud` placeholder, so the real
        // filename never appears in this walk at all. Left alone that reads as
        // "the user deleted this photo": the key is missing from
        // `live_asset_keys`, `stale_carried_forward` drops its manifest entry,
        // and `remove_stale_files` deletes the published file. Claiming the key
        // the stub stands for is what keeps the previous build's output alive
        // until the download lands — the asset-side counterpart of what
        // `scan.rs` does for pages via `evicted_paths`.
        if let Some(real) = crate::build::icloud::icloud_stub_target(file_path) {
            if let Ok(rel) = real.strip_prefix(source_root) {
                let rel = moss_core::slug::normalize_separators(&rel.to_string_lossy());
                live_asset_keys.insert(resolve_path_with_overrides(&rel, &ctx.dir_overrides));
                live_source_keys.insert(rel.clone());
                crate::build::cloud_readiness::request_download(&real);
                log::debug!("[background-assets] Holding {} — still in the cloud (pre-Sonoma stub)", rel);
            }
            continue;
        }

        // Skip OS / VCS metadata that should never reach a live site
        // (.DS_Store, .gitignore, Finder's `Icon\r`, …). The directory
        // exclusion rule in classify::is_excluded_dir_name only handles
        // dot-prefixed dirs; this catches the file-level cases.
        if let Some(file_name) = file_path.file_name().and_then(|n| n.to_str()) {
            if crate::build::scan::classify::is_os_metadata_file(file_name) {
                continue;
            }
        }

        let ext = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        // Skip files handled by other phases.
        // Notebooks (.ipynb) are handled by run_notebook_processing in build.rs.
        if markdown_exts.contains(&ext.as_str()) || doc_exts.contains(&ext.as_str()) || ext == "ipynb" {
            continue;
        }

        // Videos outside passthrough roots are the video worker's alone. A
        // passthrough subtree deliberately opts out of that worker, so this
        // loop must copy its video verbatim or the asset disappears. This used
        // to be a size
        // test that had to agree with a differently-spelled size test in
        // `collect_videos_for_conversion`, and on a sub-threshold .mov the two
        // disagreed: the worker transcoded it AND this loop copied the
        // original, so the site shipped both. One owner cannot disagree with
        // itself.
        let configured_passthrough = crate::build::scan::classify::is_in_passthrough(
            &relative_path,
            &ctx.passthrough_roots,
        );
        if video_exts.contains(&ext.as_str()) && !configured_passthrough {
            continue;
        }

        // HTML files: skip if the blocking phase generated this path (e.g., index.html
        // pages built from .md files). Source HTML files (interactive embeds, p5.js
        // sketches) must fall through and be copied as assets.
        //
        // CRITICAL: blocking_keys must only contain keys freshly generated by the
        // blocking phase (via `BuildContext::emit*` in render/blocking.rs). If it
        // also contains carry-forward entries from previous_hashes, source .html
        // files will be incorrectly skipped here, causing 404s on iframe covers.
        // See render/blocking.rs blocking_keys declaration for the full explanation.
        if html_exts.contains(&ext.as_str()) {
            let exact_file_passthrough = ctx.passthrough_roots.contains(&relative_path);
            if ctx.blocking_keys.contains(&relative_path) && !exact_file_passthrough {
                continue;
            }
            // Fall through — copy as asset
        }

        // Skip root style.css / script.js — the canonical theme location is
        // .moss/theme/; a root-level file is not served, and
        // check_misplaced_theme_files() warns authors about the misplacement.
        // This must be the same predicate the sweep's drift walk uses
        // (`drift_eligible` in build_shell::watch::sweep) — see
        // is_ignored_root_theme_file's doc comment.
        if crate::build::render::is_ignored_root_theme_file(&relative_path) {
            continue;
        }

        // Stat the source file once. We need (size, mtime) for the cache
        // lookup AND the cache write-back; doing one stat instead of three
        // (cache lookup, store_file, write-back) is a small but real saving.
        // Moved above the WebP-ownership check below so that check can reuse
        // `cached_oid` instead of resolving its own (see that check's comment).
        let file_stat = fs::metadata(file_path);
        // Preserves the "did the stat succeed" distinction separately from
        // `file_size` below — a stat failure must NOT be confused with a
        // genuine 0-byte file by the WebP-ownership check (see its comment).
        let file_size_opt = file_stat.as_ref().ok().map(|m| m.len());
        let (file_size, file_mtime_secs, file_mtime_nanos) = match &file_stat {
            Ok(m) => {
                let mtime = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
                (
                    m.len(),
                    mtime.map(|d| d.as_secs()).unwrap_or(0),
                    // None ⇔ no readable mtime; the watcher gate then always
                    // hashes instead of trusting a size-only match.
                    mtime.map(|d| d.subsec_nanos()),
                )
            }
            Err(_) => (0, 0, None),
        };

        // Cache fast-path: if we already hashed this exact (size, mtime)
        // last build AND the blob is still in the object store AND we
        // weren't racing the previous hashes.json write, reuse the oid.
        let cached_oid = check_source_cache(
            prev_sources.get(&relative_path),
            file_size,
            file_mtime_secs,
            prev_hashes_mtime_secs,
        )
        .filter(|oid| object_store.get_path(oid).is_some());

        // WebP source ownership: a webp SOURCE's base output path IS the source
        // path (`photo.webp`), so a verbatim copy here targets the SAME staged
        // file that the background image worker (`convert_single_image`) writes
        // the RESIZED base into. When the converter OWNS that base (large,
        // non-animated → in the conversion set), a verbatim copy collides with
        // it — and on a WARM build the fast cache-link copy wins the race,
        // shipping the full-resolution source while the emitted `<img srcset>`
        // base descriptor advertises the smaller deployed width. The converter
        // is the SOLE writer of the resized base, so skip this webp entirely
        // (no write, no manifest entry — the image worker registers it via the
        // coordinator channel, exactly as it does for a jpg/png source whose
        // `photo.webp` base is a differently-named, non-colliding file).
        //
        // The decision here MUST agree byte-for-byte with
        // `collect_images_for_conversion` (which decides the webp IS converted).
        // We call the SAME `should_skip` predicate with the SAME `Default`
        // config both call sites use (this verbatim path + the collector above).
        // `source_oid` is `cached_oid` resolved via the SAME cheap stat-match
        // fast path `collect_images_for_conversion` uses (falls back to ""
        // on a miss, exactly like that call site) — this lets both callers
        // consult the same `should_skip` format-probe cache instead
        // of one of them recomputing the expensive probes on every build:
        //   • None    → webp is converted → converter owns the base → skip here.
        //   • Some(_) → AlreadySmall / AnimatedWebp / NotAnImage / … → the
        //     converter never writes it → fall through and copy verbatim, else
        //     the base 404s. Small + animated webp rely on this.
        //
        // A read failure fails SAFE (converter owns the base) rather than the
        // old `.unwrap_or(0)` — see `webp_converter_owns_base` (follow-up #6).
        if ext == "webp"
            && webp_converter_owns_base(
                file_path,
                &ext,
                file_size_opt,
                &image_config,
                &transforms,
                cached_oid.as_deref().unwrap_or(""),
            )
        {
            continue;
        }

        // Map asset path through dir_overrides so assets land at their
        // page-tree path (e.g., "交互/sketch.html" → "interactive/sketch.html")
        let mapped_path = resolve_path_with_overrides(&relative_path, &ctx.dir_overrides);

        live_asset_keys.insert(mapped_path.clone());
        // Also record the *source-relative* path for `sources` map pruning
        // below. Sources is keyed by source path (pre-mapping), unlike
        // `files` which is keyed by output path (post-mapping).
        live_source_keys.insert(relative_path.clone());

        // An asset whose bytes are still in the cloud is asked for and skipped,
        // NOT read. `store_file` hashes the whole file, and on a miss that read
        // waits out the materialize deadline — 90 s, once per file, in this
        // sequential loop. A vault with a few hundred evicted photos would spend
        // hours here. The supervisor is what waits now, and the arrival triggers
        // the rebuild that stores the asset properly.
        //
        // Both key sets are inserted FIRST, deliberately: they are what tells
        // the stale-cleanup below that this file still exists. Skipping past
        // them would make an offline photo indistinguishable from a deleted one
        // and drop the still-good output the last build produced.
        if cached_oid.is_none() && crate::build::icloud::is_evicted(file_path) {
            crate::build::cloud_readiness::request_download(file_path);
            log::debug!("[background-assets] Deferring {} — still in the cloud", relative_path);
            continue;
        }

        let oid_result = if let Some(oid) = cached_oid {
            cache_hits += 1;
            Ok(oid)
        } else {
            cache_misses += 1;
            object_store.store_file(file_path)
        };

        match oid_result {
            Ok(oid) => {
                // Deploy a SIZED/optimized raster for raster originals
                // (jpg/jpeg/png decodable stills) instead of the full-resolution
                // source. The output stays in the SOURCE's own format — jpg/jpeg
                // → sized JPEG, png → sized PNG with alpha PRESERVED (so a
                // transparent logo/diagram/icon keeps its transparency and the
                // bytes at a `.png` path are a real PNG). Every downstream
                // reference (content <picture> <img> fallback,
                // og:image/twitter:image, newsletter EmailBody images,
                // site-logo, hero) uses this same output path, so none of them
                // ship a full-res original. Non-raster, animated, CMYK, or
                // non-decodable files fall back to a verbatim copy
                // (link_oid == oid). The sized output is content-addressed and
                // cached by source hash, so unchanged images are not re-encoded
                // on incremental builds. The manifest hash below is computed from
                // the OUTPUT file, so it correctly reflects the sized bytes.
                // A source still in the cloud never reaches the sized-raster
                // encode.
                //
                // The deferral above is guarded on `cached_oid.is_none()`,
                // correctly: with a warm OID the bytes are already in the CAS
                // and the output can be linked without touching the source. But
                // `sized_raster_oid_for_original` takes the SOURCE PATH and, on
                // a transform-cache miss, reads it — so a warm OID walked
                // straight past the deferral into a full source read that fails
                // `EDEADLK`. That is the 6,967 `[sized-raster] encode failed …
                // keeping verbatim original` lines in one incident's uploaded
                // log, once per image per build for the whole download window,
                // and a large part of the 7-10 cores it burned.
                //
                // Linking the verbatim CAS blob is the honest fallback: it is
                // the full-resolution original rather than the sized one, which
                // is heavier than we would like but correct and available now.
                // The next build after the file lands re-encodes it properly.
                let source_in_the_cloud = crate::build::icloud::is_evicted(file_path);
                if source_in_the_cloud {
                    crate::build::cloud_ledger::note_unavailable(file_path);
                }
                let link_oid = if source_in_the_cloud {
                    oid.clone()
                } else if matches!(ext.as_str(), "jpg" | "jpeg" | "png") {
                    crate::build::media::fallback_raster::sized_raster_oid_for_original(
                        file_path,
                        &oid,
                        &object_store,
                        &transforms,
                        &image_config,
                        crate::build::media::fallback_raster::SIZED_JPEG_QUALITY,
                    )
                    .unwrap_or_else(|| oid.clone())
                } else {
                    oid.clone()
                };

                let target = output_dir.join(&mapped_path);
                let placement =
                    BlobPlacement { link_oid: &link_oid, target: &target, log_label: &mapped_path, report_path: &mapped_path, ext: &ext };
                if !place_blob(&object_store, placement, reporter) {
                    continue;
                }

                // Register audio/PDF/static assets as Ready so the
                // editor hover-preview can resolve them. Only register non-image,
                // non-video assets here — images are registered in the blocking
                // phase via set_pending (and set_ready'd by run_image_conversion).
                // Videos are also registered in the blocking phase. The registry
                // key matches the URL the preview server normalises: no leading
                // slash, output-relative (e.g. "audio/talk.mp3", "docs/paper.pdf").
                if let Some(ref registry) = asset_registry {
                    if !is_image_extension(&ext) && !video_exts.contains(&ext.as_str()) {
                        registry.set_ready(mapped_path.clone());
                    }
                }

                // Bundled-SPA `<head>` injection (link-preview-defaults Phase 5).
                // After the source HTML has been linked into the output, post-
                // process bundled-SPA `index.html` files (e.g. a Vite app at
                // `cities-heat-map/index.html`) to inject site-level meta
                // defaults. The injector preserves anything the SPA author
                // already set, so it's safe to run unconditionally on these.
                //
                // Restricted to subfolder `index.html` files (root `index.html`
                // is moss-rendered, never a passthrough) — guarantees we never
                // double-tag a moss-templated page.
                //
                // Replaces the hardlink with a fresh file containing the
                // injected content. The `files` hash entry is updated to the
                // post-injection content hash so deploy diffing matches what's
                // on disk.
                let in_passthrough = crate::build::scan::classify::is_in_passthrough(
                    &relative_path,
                    &ctx.passthrough_roots,
                );
                let mut spa_post_hash: Option<String> = None;
                // The CAS object actually backing `target`'s bytes AFTER SPA
                // injection, when injection changed them. `link_oid` (below)
                // names the PRE-injection object `link_to` placed at `target`
                // moments ago; injection then overwrites those bytes in place
                // with a freshly-minted object, so `link_oid` alone would name
                // the wrong blob for `staged_oids` — see
                // `maybe_inject_spa_cached`'s doc comment.
                let mut spa_post_oid: Option<String> = None;
                if !in_passthrough
                    && html_exts.contains(&ext.as_str())
                    && is_bundled_spa_index_path(&mapped_path)
                {
                    if let Some(ref defaults) = ctx.spa_defaults {
                        let canonical_url_for_folder = derive_canonical_url(defaults, &mapped_path);
                        let borrowed = defaults.as_borrowed(canonical_url_for_folder.as_deref());
                        match maybe_inject_spa_cached(
                            &target,
                            &borrowed,
                            &oid,
                            file_size,
                            &object_store,
                            &transforms,
                        ) {
                            Ok(Some((new_hash, new_oid))) => {
                                spa_post_hash = Some(new_hash);
                                spa_post_oid = Some(new_oid);
                            }
                            Ok(None) => {} // no-op (nothing to inject)
                            Err(e) => {
                                // Fail open — leave the file alone, don't break the build.
                                log::warn!(
                                    "[background-assets] SPA inject failed for {}: {}",
                                    mapped_path, e
                                );
                            }
                        }
                    }
                }

                // Record the source-metadata entry (keyed by source-relative
                // path, not output path) so the next build's check_source_cache
                // can short-circuit. Always write — even on cache hit — so
                // the entry stays present after every build.
                let (src_ctime, src_inode) = file_stat
                    .as_ref()
                    .map(crate::build::types::stat_identity)
                    .unwrap_or((None, None));
                site_hashes.sources.insert(
                    relative_path.clone(),
                    crate::build::types::SourceMetadata {
                        hash: oid.clone(),
                        size: file_size,
                        mtime: file_mtime_secs,
                        mtime_nanos: file_mtime_nanos,
                        ctime: src_ctime,
                        inode: src_inode,
                    },
                );
                match crate::build::served_path::ServedPath::from_source(&mapped_path) {
                    Ok(out_path) => {
                        // The CAS object actually backing `target`'s bytes: the
                        // post-injection object when SPA injection changed
                        // them, otherwise the object `link_to` placed there
                        // (`link_oid`) — `apply_transform`/`transform_for` are
                        // pure and path-only, so a later CAS read reproduces
                        // exactly what reading `target` now would.
                        let staged_oid = spa_post_oid.clone().unwrap_or_else(|| link_oid.clone());
                        let hash = spa_post_hash.map(OutputHash::Known).unwrap_or(OutputHash::Compute {
                            memo_key: &link_oid,
                            target: &target,
                            fallback_oid: &oid,
                            log_context: "output",
                        });
                        record_blob(&manifest_hash_memo, &mut site_hashes, &mut staged_oids, hash, &out_path, &staged_oid);
                        copied += 1;
                    }
                    Err(e) => {
                        log::warn!("[background-assets] Skipping file with invalid path '{}': {}", mapped_path, e);
                        skipped += 1;
                    }
                }
            }
            Err(e) => {
                log::warn!("[background-assets] Failed to store {}: {}", relative_path, e);
                // Fallback: direct copy without CAS
                let target = output_dir.join(&mapped_path);
                if let Err(copy_err) = crate::build::io_utils::copy_output(file_path, &target) {
                    log::error!(
                        "[background-assets] Direct copy also failed for {}: {}",
                        relative_path, copy_err
                    );
                }
                skipped += 1;
            }
        }
    }

    // Copy .moss/theme/ to output (verbatim mirror: textures, fonts, video
    // overlays, plus any user subfolders). .moss/ is excluded from the main
    // walkdir (dot-prefix), so we walk it separately. Files land under
    // _moss/theme/<rel> (e.g., .moss/theme/leaves.mp4 → _moss/theme/leaves.mp4).
    let moss_assets_dir = source_root.join(".moss").join("theme");
    if moss_assets_dir.is_dir() {
        for entry in WalkDir::new(&moss_assets_dir).into_iter() {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    log::warn!("[background-assets] .moss/theme walk error: {}", e);
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let file_path = entry.path();
            let relative_path = match file_path.strip_prefix(&moss_assets_dir) {
                Ok(rel) => rel.to_string_lossy().to_string(),
                Err(_) => continue,
            };

            // Guard: skip style.css and script.js — these are emitted by the
            // blocking phase (as _moss/theme/style.css and _moss/theme/script.js).
            // All other files, including any user-named custom.css/custom.js,
            // mirror through verbatim under _moss/theme/.
            if relative_path == "style.css" || relative_path == "script.js" {
                // Every build, unconditionally, for exactly these two files —
                // not actionable by anyone reading the log, so DEBUG not WARN
                // (measured ~170 lines/session in a real upload, 2026-09-15).
                log::debug!(
                    "[background-assets] Skipping .moss/theme/{} — handled by blocking phase",
                    relative_path
                );
                continue;
            }

            // Compute the typed served path. for_theme_asset rejects path-traversal
            // attempts; an Err here is a skip.
            let out_path = match crate::build::served_path::ServedPath::for_theme_asset(&relative_path) {
                Ok(p) => p,
                Err(e) => {
                    log::warn!("[background-assets] Skipping theme asset '{}': {}", relative_path, e);
                    skipped += 1;
                    continue;
                }
            };

            live_asset_keys.insert(out_path.as_str().to_string());

            // Inserted into live_asset_keys first, so the previous build's copy
            // is not read as deleted, and only then skipped. `store_file` hashes
            // the whole file: on an evicted theme asset that is a full
            // materialize wait, serially, for every one of them, while
            // `ui_bound` holds publishing back. Defer instead — the supervisor
            // rebuilds when it lands.
            if crate::build::icloud::is_evicted(file_path) {
                crate::build::cloud_readiness::request_download(file_path);
                log::debug!("[background-assets] Deferring .moss/theme/{} — still in the cloud", relative_path);
                continue;
            }

            match object_store.store_file(file_path) {
                Ok(oid) => {
                    let target = out_path.to_disk(output_dir);
                    let moss_ext = file_path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let log_label = format!(".moss/theme/{}", relative_path);
                    let placement =
                        BlobPlacement { link_oid: &oid, target: &target, log_label: &log_label, report_path: out_path.as_str(), ext: &moss_ext };
                    if !place_blob(&object_store, placement, reporter) {
                        continue;
                    }
                    let hash =
                        OutputHash::Compute { memo_key: &oid, target: &target, fallback_oid: &oid, log_context: ".moss/theme output" };
                    record_blob(&manifest_hash_memo, &mut site_hashes, &mut staged_oids, hash, &out_path, &oid);
                    copied += 1;
                }
                Err(e) => {
                    log::warn!("[background-assets] Failed to store .moss/theme/{}: {}", relative_path, e);
                    skipped += 1;
                }
            }
        }
    }

    // Drop the previous build's entries for assets this walk did not find.
    //
    // The seed is the previous manifest, so anything left in it that the walk
    // did not claim has no source file behind it any more and must not be
    // re-emitted — otherwise mark-and-sweep at seal keeps a key whose file is
    // gone and deploy trips on it.
    //
    // Only two things count as claimed, and neither has to do with rendering:
    // an asset the walk found (`live_asset_keys` — inserted BEFORE the read, so
    // an evicted file still counts), and a symlink the symlink/alias branches
    // recreated (`live_symlinks`; symlinks carry no content hash so they never
    // enter `live_asset_keys` — dropping them is the `cities-heat-map` 404).
    //
    // Rendered pages, OG cards and notebook outputs used to need explicit
    // keep-clauses here because the seed contained them. It no longer does:
    // they are the coordinator's, registered there by this build.
    let stale_carried_forward: Vec<String> = site_hashes
        .files
        .iter()
        .filter(|(key, _)| {
            !live_asset_keys.contains(*key) && !live_symlinks.contains(*key)
        })
        .map(|(key, _)| key.clone())
        .collect();
    for key in &stale_carried_forward {
        site_hashes.files.remove(key);
        log::trace!("[background-assets] Removed stale hash: {}", key);
    }

    // Prune the source-metadata cache against the same source-relative key
    // set used during the walk. Without this, `sources` accumulates entries
    // for every file ever touched by the project — e.g. a renamed image
    // leaves both old and new paths in `hashes.json` indefinitely.
    let stale_sources: Vec<String> = site_hashes
        .sources
        .keys()
        .filter(|k| !live_source_keys.contains(*k))
        .cloned()
        .collect();
    for key in &stale_sources {
        site_hashes.sources.remove(key);
    }
    // One line for the whole prune, not one per entry — a rename/restructure
    // or a first full-cache build could otherwise log ~1300 lines in a
    // single "Send Logs" upload for information no individual key adds over
    // the count (2026-09-15).
    if !stale_sources.is_empty() {
        log::debug!(
            "[background-assets] Removed {} stale source-cache entr{}",
            stale_sources.len(),
            if stale_sources.len() == 1 { "y" } else { "ies" }
        );
    }

    // All background workers (image, video, assets) send their
    // registrations via the coordinator channel, so `video_outputs`,
    // `image_outputs`, and `notebook_outputs` accumulate in the
    // coordinator's PendingManifest — not on disk. The legacy on-disk merge
    // that ran through `tx.is_none()` has been removed.

    // Stale files, dirs and symlinks leave staging only through the build's
    // permitted sweep (`pipeline::sweep_staging`), never from this worker.

    // Send all accumulated file entries to the coordinator (or write to disk
    // in legacy mode). The coordinator merges these with image/video/notebook
    // registrations from the concurrent workers and seals the manifest.
    // Send each file entry to the coordinator. `site_hashes.files` contains:
    //   - carry-forward entries from previous builds (still present, minus
    //     the stale-carried-forward ones pruned above)
    //   - new entries added during this walk (from asset files found in source)
    // The coordinator accumulates these on top of render-phase emits.
    //
    // **Load-bearing invariant for mark-and-sweep at `PendingManifest::seal`:**
    // this loop MUST re-emit every entry in `site_hashes.files`, regardless of
    // whether it changed since the previous build. `PendingManifest::seal`
    // prunes the output buckets to retain only entries that were registered
    // via `register*` / `apply_message` during this build (the `touched` mark
    // set). An optimisation here that sends only newly-added entries would
    // silently delete carry-forward static assets (CSS, JS, fonts, raw
    // images, source HTML) from the deploy manifest — they would no longer
    // be `touched` at seal time, would be pruned from the output buckets,
    // and stale-file cleanup would then delete them from the sealed generation dir.
    //
    // We send ALL entries (not just newly added ones) so the coordinator's
    // PendingManifest has the full picture including carry-forwards. The
    // coordinator started from the render-phase pending which already has
    // render-phase entries; sending carry-forwards here may result in
    // duplicates (same key, same hash) which is idempotent (HashMap insert
    // with same key just overwrites with same value).
    for (rel_path, hash_entry) in &site_hashes.files {
        let msg = EmitMessage::File {
            rel_path: rel_path.clone(),
            hash: hash_entry.strip_prefix("100644:").unwrap_or(hash_entry).to_string(),
            bucket: HashBucket::Files,
            oid: staged_oids.get(rel_path).cloned(),
        };
        if let Err(e) = tx.blocking_send(msg) {
            log::warn!("[background-assets] Failed to send {} to coordinator: {}", rel_path, e);
        }
    }
    log::info!(
        "[background-assets] Sent {} file entries to coordinator",
        site_hashes.files.len()
    );

    // Propagate this build's source-cache (inserts from cache misses, deletes
    // from the live-set prune above) to the coordinator. Without this,
    // `PendingManifest::inner.sources` would remain the previous build's
    // carry-forward and the next build's `prev_sources` (loaded from the
    // sealed `hashes.json`) would be two builds stale, defeating the
    // change-detection cache.
    //
    // `mem::take` (vs. `clone`) moves the ~40KB sources map into the message
    // — `site_hashes` is dropped at the end of this function and has no
    // further reads after this send, so no copy is needed.
    let sources_payload = std::mem::take(&mut site_hashes.sources);
    if let Err(e) = tx.blocking_send(EmitMessage::SourcesReplace(sources_payload)) {
        log::warn!("[background-assets] Failed to send sources to coordinator: {}", e);
    }

    // Persist the oid→xxh3 memo once per build (not once per asset — see
    // `ManifestHashMemo`'s doc comment). Best-effort: a failed save just
    // means the next build starts cold, same as today.
    if let Err(e) = manifest_hash_memo.save(&deferred_paths.cache_manifest_hash_memo()) {
        log::warn!("[background-assets] Failed to persist manifest-hash memo: {}", e);
    }
    manifest_hash_memo.log_summary();

    let stats = AssetPipelineRunStats {
        copied,
        skipped,
        cache_hits,
        cache_misses,
        skipped_symlinks,
        preserved_symlinks,
        preserved_aliases,
    };
    log::info!("{}", format_asset_run_summary(&stats));
    stats
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
