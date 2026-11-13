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

use crate::build::scan::path_collision::{logical_path, on_disk};
use crate::moss_paths::MossPaths;
use crate::types::{content::SiteHashes, services::BackgroundContext};
use std::collections::{HashMap, HashSet};
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
    /// Outputs a stat proved already in place, and outputs linked from the CAS.
    pub held: u32,
    pub linked: u32,
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
    if entry.stat.size != current_size {
        return None;
    }
    if entry.stat.mtime != current_mtime_secs {
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
        .map_err(|e| log::info!("derive_canonical_url: '{}' rejected by ServedPath: {}", mapped_path, e))
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
        "color_scheme": defaults.color_scheme.fixed(),
    })
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

    if let Some(record) = transform_cache.summary::<SpaInjectRecord>(SPA_INJECT_TRANSFORM, source_oid, &params) {
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

    let result = maybe_inject_spa(target, defaults)?;
    let (returned, record, page_size) = match &result {
        Some(new_hash) => {
            let injected_bytes = std::fs::read(target)
                .map_err(|e| format!("read back {} after inject: {}", target.display(), e))?;
            let content_oid = object_store.store_bytes(&injected_bytes, crate::build::cache::RecordMode::Request)?;
            (
                Some((new_hash.clone(), content_oid.clone())),
                SpaInjectRecord {
                    content_oid: Some(content_oid),
                    xxh3: Some(new_hash.clone()),
                },
                injected_bytes.len() as u64,
            )
        }
        None => (
            None,
            SpaInjectRecord {
                content_oid: None,
                xxh3: None,
            },
            0,
        ),
    };
    transform_cache.put_summary(SPA_INJECT_TRANSFORM, source_oid, source_size, &params, &record, record.content_oid.as_deref().map(|oid| (oid, page_size)));

    Ok(returned)
}

fn is_html_path(key: &str) -> bool {
    let ext = Path::new(key).extension().and_then(|ext| ext.to_str()).unwrap_or("");
    ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm")
}

/// Previous HTML outputs no writer has claimed before search reads staging.
/// Theme assets are deferred; notebooks, carried pages and `spared` addresses
/// (sources in the cloud, forwarding pages the seal writes again) keep theirs,
/// and so does a preserved symlink, which the deferred walk registers after
/// this sweep.
pub(crate) fn stale_html_keys(previous: &SiteHashes, keep: &HashSet<String>, notebook_outputs: &HashSet<String>, spared: &HashSet<String>) -> Vec<String> {
    previous.files.iter().filter(|(key, entry)| {
        is_html_path(key) && !key.starts_with("_moss/theme/")
            && crate::types::content::parse_entry(entry).0 != crate::types::content::MODE_SYMLINK
            && !keep.contains(*key) && !notebook_outputs.contains(*key) && !spared.contains(*key)
    }).map(|(key, _)| key.clone()).collect()
}

/// Whether an authored HTML page the previous build copied has lost its
/// source, and the previous HTML outputs the sweep removes. The manifest
/// carries every previous output until seal, so the hash compare cannot see
/// the deletion; outputs with no source of their own, like forwarding pages,
/// never count as one. A source in the cloud is offline, not deleted. An
/// address an authored page held is never spared as a forwarding page: its
/// staged bytes are the deleted text, which search would index.
pub(crate) fn html_deletions(
    previous: &SiteHashes,
    pending: &crate::build::manifest::PendingManifest,
    structure: &crate::types::content::ProjectStructure,
    folder_path: &str,
    dir_overrides: &HashMap<String, String>,
    late: &crate::build::feeds::kept_addresses::LateRedirects,
) -> (bool, Vec<String>) {
    let held: HashSet<String> = structure.evicted_paths.iter()
        .filter_map(|path| path.strip_prefix(folder_path).ok())
        .map(logical_path)
        .collect();
    let current: HashSet<&str> = structure.html_files.iter().map(|file| file.path.as_str()).collect();
    let deleted = previous.sources.keys().any(|key| is_html_path(key) && !key.starts_with(".moss/")
        && !current.contains(key.as_str()) && !held.contains(key));
    let address = |rel: &String| crate::build::render::resolve_path_with_overrides(rel, dir_overrides);
    let authored: HashSet<String> = previous.sources.keys().filter(|key| is_html_path(key)).map(address).collect();
    let forwarded = crate::build::feeds::kept_addresses::moved_folder_files(&late.folder_moves, pending.blocking_keys())
        .into_iter().map(|(old, _)| old).chain(late.copies.iter().map(|copy| copy.from.clone()));
    let spared: HashSet<String> = held.iter().map(address).chain(forwarded.filter(|key| !authored.contains(key))).collect();
    (deleted, stale_html_keys(previous, pending.blocking_keys(), pending.notebook_outputs(), &spared))
}

/// Remove exactly the deleted HTML outputs before search reads staging. A key
/// that differs from a `live` one only in case is kept when both name the same
/// file, as after a case-only rename on a case-insensitive disk.
pub(crate) fn remove_stale_html(dir: &Path, deleted: &[String], live: &HashSet<String>, _permit: &crate::build::lifecycle::SweepPermit) {
    let disk = |key: &str| crate::build::served_path::ServedPath::from_cached(key).ok().map(|path| path.to_disk(dir));
    let folded: HashMap<String, &String> = live.iter().map(|key| (key.to_lowercase(), key)).collect();
    for key in deleted {
        let Some(path) = disk(key) else { continue; };
        let renamed = folded.get(&key.to_lowercase()).and_then(|live| disk(live))
            .is_some_and(|live| same_file::is_same_file(&path, live).unwrap_or(false));
        if renamed { continue; }
        match fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => log::warn!("[stale-html] Cannot remove {}: {}", key, e),
            _ => {}
        }
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
            _ if moss_core::asset_paths::is_video_still(key) => self.video += 1,
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
            if crate::build::served_path::is_append_only(&key) {
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
        // A hosted-picture family is append-only: a build whose pages no
        // longer carry math or diagrams computes no expected entry for its
        // dir, but the PNGs inside are referenced by already-sent emails.
        let rel = moss_core::slug::normalize_separators(&d.to_string_lossy());
        if crate::build::served_path::is_append_only(&format!("{rel}/")) {
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

/// Resolve source bytes once for both ordinary and theme assets. A cached
/// blob remains usable while its source is evicted; a cold evicted source
/// is requested and deferred without reading it.
fn source_blob(
    path: &Path,
    cached: Option<&crate::build::types::SourceMetadata>,
    previous_write: u64,
    object_store: &crate::build::cache::ObjectStore,
    cache_hits: &mut u32,
    cache_misses: &mut u32,
) -> Result<Option<(String, crate::build::stat::FileStat)>, String> {
    let stat = fs::metadata(path)
        .as_ref()
        .map(crate::build::stat::FileStat::of)
        .unwrap_or(crate::build::stat::FileStat::whole_second(0, 0));
    // A source stat identifies the desired blob; placement verifies or restores
    // its staged copy. Reading the CAS here would rehash every warm asset.
    if let Some(oid) = cached.filter(|entry| entry.stat.vouches_for(&stat, Some(previous_write)))
        .map(|entry| entry.hash.clone())
        .or_else(|| check_source_cache(cached.filter(|e| e.stat.mtime_nanos.is_none()), stat.size, stat.mtime, previous_write))
    {
        *cache_hits += 1;
        return Ok(Some((oid, stat)));
    }
    if crate::build::icloud::is_evicted(path) {
        crate::build::cloud_readiness::request_download(path);
        log::debug!("[background-assets] Deferring {} — still in the cloud", path.display());
        return Ok(None);
    }
    *cache_misses += 1;
    object_store.store_source_file(path, crate::build::cache::RecordMode::Wait)
        .map(|oid| Some((oid, stat)))
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
    transform: crate::build::served_path::ShipTransform,
    mount: &crate::build::site_meta::mount::Mount,
) -> String {
    // A transformed entry's hash describes the bytes ship writes, not the
    // blob's, so it is memoized under its own key, which also carries the
    // transform revision and the mount: neither may recall a hash of other output.
    let copies = transform.copies(mount);
    let key = if copies {
        memo_key.to_string()
    } else {
        format!("{memo_key}+ship{}", crate::build::ship::shipped_key(mount))
    };
    if let Some(hash) = memo.get(&key) {
        return hash;
    }
    let hashed = if copies {
        crate::build::assets::paths::compute_binary_hash_file(target)
    } else {
        std::fs::read(target).map_err(|e| e.to_string()).map(|bytes| crate::build::ship::apply_transform(transform, mount, &bytes))
            .map(|shipped| crate::build::assets::paths::compute_binary_hash(&shipped))
    };
    match hashed {
        Ok(hash) => {
            memo.record(&key, hash.clone());
            hash
        }
        Err(e) => {
            log::info!(
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

/// The alias file itself is a source the sweep walks, published or not:
/// without a `sources` entry it reads as a new file on every pass.
#[cfg(target_os = "macos")]
fn register_alias_source(
    path: &Path,
    source_root: &Path,
    site_hashes: &mut SiteHashes,
    live_source_keys: &mut std::collections::HashSet<String>,
    mut blob: impl FnMut(&Path, &str) -> Result<Option<(String, crate::build::stat::FileStat)>, String>,
) {
    let Ok(rel) = path.strip_prefix(source_root) else { return };
    let rel = logical_path(rel);
    if let Ok(Some((oid, stat))) = blob(path, &rel) {
        site_hashes.sources.insert(rel.clone(), crate::build::types::SourceMetadata::from_stat(oid, stat));
        live_source_keys.insert(rel);
    }
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
    local: Option<&'a Path>,
}

/// Link the blob into output — unless `staged` vouches the file there is
/// already that blob — and, for an image extension, report `AssetReady`;
/// shared by the main asset walk and the `.moss/theme` mirror below. `false`
/// (already logged) on a link failure.
fn place_blob(
    object_store: &crate::build::cache::ObjectStore,
    staged: &mut crate::build::lifecycle::cas_heal::StagedLinks,
    placement: BlobPlacement,
    reporter: &dyn crate::build::ports::reporter::BuildReporter,
    stats: &mut AssetPipelineRunStats,
) -> bool {
    use crate::build::lifecycle::cas_heal::Placement;
    match staged.link(object_store, placement.link_oid, placement.target, placement.local) {
        Placement::Held => stats.held += 1,
        Placement::Linked => stats.linked += 1,
        Placement::Unverified(e) => {
            log::info!("[background-assets] Not linking over unverifiable {}: {}", placement.log_label, e);
            return false;
        }
        Placement::Failed(e) => {
            log::info!("[background-assets] Failed to link {}: {}", placement.log_label, e);
            return false;
        }
    }
    if is_image_extension(placement.ext) {
        reporter.report(&PipelineEvent::AssetReady {
            path: placement.report_path.to_string(),
            asset_type: "image".to_string(),
        });
    }
    true
}

/// The inputs `record_blob` resolves a manifest hash from via
/// `recall_or_hash_output`. A struct, not four positional `&str`s: at the
/// theme call site three of them are copies of the same `oid`, with nothing to
/// catch a swap.
struct OutputHash<'a> {
    memo_key: &'a str,
    target: &'a Path,
    fallback_oid: &'a str,
    log_context: &'a str,
}

/// Resolve `hash` and register it, plus the staged CAS object id, for a blob
/// `place_blob` already linked into `out_path`'s target.
///
/// The sealed hash is of the bytes ship writes: for an HTML entry that is the
/// staged blob AFTER `apply_transform`, not the blob. Sealing the blob's own
/// hash would mismatch whenever the transform changes something (a hand-written
/// page carrying `data-moss-preview`), skewing the generation id and the
/// server diff, and leaving every such upload to the integrity self-heal.
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
    mount: &crate::build::site_meta::mount::Mount,
) {
    let OutputHash { memo_key, target, fallback_oid, log_context } = hash;
    let transform = crate::build::served_path::transform_for(out_path.as_str());
    let hash = recall_or_hash_output(manifest_hash_memo, memo_key, target, fallback_oid, log_context, transform, mount);
    site_hashes.insert_file_hash(out_path, crate::types::content::file_entry(&hash));
    staged_oids.insert(out_path.as_str().to_string(), staged_oid.to_string());
}

/// Live output and source keys plus placement counters for an asset pass.
#[derive(Default)]
struct AssetWalkTally {
    stats: AssetPipelineRunStats,
    /// Output paths the walk found in the source. After the walk, carried-forward
    /// entries not in this set are stale and removed from `site_hashes`.
    live_asset_keys: std::collections::HashSet<String>,
    /// The same keyed by source-relative path, used to prune
    /// `site_hashes.sources`; without it deleted or renamed sources stay in the
    /// metadata cache and `hashes.json` grows without bound.
    live_source_keys: std::collections::HashSet<String>,
    /// Symlinks preserved from source to output, including resolved Finder aliases.
    ///
    /// Real symlinks are not entered into the two sets above: they have no
    /// content hash and are not in the object-store cache. A Finder alias is a
    /// regular file, so its own bytes are a source (`register_alias_source`).
    live_symlinks: std::collections::HashSet<String>,
    /// Output path -> the CAS object id already backing those staged bytes, for
    /// every entry registered via `record_blob`. Threaded onto the re-emitted
    /// `EmitMessage::File` so `ship_phase` can copy straight from the CAS
    /// (`ShipSource::Cas`). A symlink entry or direct-copy fallback never gets
    /// one and ships from the stage path.
    staged_oids: HashMap<String, String>,
}

impl AssetWalkTally {
    fn prune(&self, hashes: &mut SiteHashes) {
        hashes.files.retain(|key, _| self.live_asset_keys.contains(key) || self.live_symlinks.contains(key));
        hashes.sources.retain(|key, _| self.live_source_keys.contains(key));
    }
}

/// Caches and path decisions shared by source HTML and the deferred asset walk.
struct AssetCopy {
    object_store: crate::build::cache::ObjectStore,
    manifest_hash_memo: crate::build::media::manifest_hash_memo::ManifestHashMemo,
    staged: crate::build::lifecycle::cas_heal::StagedLinks,
    transforms: crate::build::cache::TransformCache,
    image_config: crate::build::media::image::ImageCompressionConfig,
    canonical_source_root: std::path::PathBuf,
    converter_bases: std::collections::HashSet<std::path::PathBuf>,
    html_paths: std::collections::HashSet<std::path::PathBuf>,
    prev_hashes_mtime_secs: u64,
}

impl AssetCopy {
    fn new(ctx: &BackgroundContext, html_files: &[crate::types::content::FileInfo]) -> Self {
        let paths = MossPaths::from_moss_dir(ctx.moss_dir.clone());
        let source_root = Path::new(&ctx.source_path);
        Self {
            object_store: crate::build::cache::ObjectStore::for_site(&paths),
            manifest_hash_memo: crate::build::media::manifest_hash_memo::ManifestHashMemo::load(&paths.cache_manifest_hash_memo()),
            staged: crate::build::lifecycle::cas_heal::StagedLinks::load(&paths, "assets", &ctx.staging_dir),
            transforms: crate::build::cache::TransformCache::for_site(&paths),
            image_config: crate::build::media::image::ImageCompressionConfig::default(),
            canonical_source_root: source_root.canonicalize().unwrap_or_else(|e| {
                log::info!("[background-assets] canonicalize source_root failed: {e}; symlinks will not be preserved");
                source_root.to_path_buf()
            }),
            converter_bases: ctx.image_items.iter().filter(|item| item.owns_source_base()).map(|item| item.source_path.clone()).collect(),
            html_paths: html_files.iter().map(|file| on_disk(source_root, &file.path)).collect(),
            prev_hashes_mtime_secs: std::fs::metadata(paths.hashes()).ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs()).unwrap_or(0),
        }
    }

    fn handles_source_html(&self, path: &Path) -> bool {
        let target = crate::build::icloud::icloud_stub_target(path);
        self.html_paths.contains(target.as_deref().unwrap_or(path))
    }
}

/// Outputs and source records produced before preview readiness and search,
/// with the caches the deferred walk goes on to reuse.
pub(crate) struct StagedHtml {
    pub files: HashMap<String, String>,
    pub sources: HashMap<String, crate::build::types::SourceMetadata>,
    pub oids: HashMap<String, String>,
    stats: AssetPipelineRunStats,
    copy: AssetCopy,
}

/// Manifest registrations for walked files, with the oid of each staged copy.
fn file_registrations<'a>(files: &'a HashMap<String, String>, oids: &'a HashMap<String, String>) -> impl Iterator<Item = EmitMessage> + 'a {
    files.iter().map(|(path, entry)| EmitMessage::File {
        rel_path: path.clone(),
        hash: entry.strip_prefix("100644:").unwrap_or(entry).to_string(),
        bucket: HashBucket::Files,
        oid: oids.get(path).cloned(),
    })
}

impl StagedHtml {
    pub(crate) fn registrations(&self) -> impl Iterator<Item = EmitMessage> + '_ {
        file_registrations(&self.files, &self.oids)
    }

    pub(crate) fn apply_to(&self, pending: &mut crate::build::manifest::PendingManifest) {
        for message in self.registrations() {
            if let EmitMessage::File { rel_path, hash, bucket, oid } = message {
                pending.apply_message(rel_path, &hash, bucket, oid);
            }
        }
    }
}

pub(crate) fn stage_source_html(
    ctx: &BackgroundContext,
    html_files: &[crate::types::content::FileInfo],
    pending: &crate::build::manifest::PendingManifest,
    reporter: &dyn crate::build::ports::reporter::BuildReporter,
    asset_registry: Option<std::sync::Arc<crate::types::assets::AssetRegistry>>,
) -> StagedHtml {
    let _phase = crate::build::phase::PhaseTrace::start("stage_source_html");
    let mut copy = AssetCopy::new(ctx, html_files);
    let mut site_hashes = SiteHashes::default();
    let mut run = AssetWalkTally::default();
    for file in html_files {
        let address = crate::build::render::resolve_path_with_overrides(&file.path, &ctx.dir_overrides);
        if pending.blocking_keys().contains(&address) && !ctx.passthrough_roots.contains(&file.path) {
            continue;
        }
        if let Some(entry) = ctx.previous_hashes.files.get(&address) {
            site_hashes.files.insert(address, entry.clone());
        }
        if let Some(entry) = ctx.previous_hashes.sources.get(&file.path) {
            site_hashes.sources.insert(file.path.clone(), entry.clone());
        }
        let path = on_disk(Path::new(&ctx.source_path), &file.path);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) => copy_asset_entry(ctx, &mut copy, &path, meta.file_type(), &mut site_hashes, &mut run, reporter, asset_registry.as_deref()),
            Err(e) => log::info!("[background-assets] Cannot stat {}: {e}", path.display()),
        }
    }
    run.prune(&mut site_hashes);
    StagedHtml { files: site_hashes.files, sources: site_hashes.sources, oids: run.staged_oids, stats: run.stats, copy }
}

fn copy_asset_entry(
    ctx: &BackgroundContext,
    copy: &mut AssetCopy,
    file_path: &Path,
    file_type: std::fs::FileType,
    site_hashes: &mut SiteHashes,
    run: &mut AssetWalkTally,
    reporter: &dyn crate::build::ports::reporter::BuildReporter,
    asset_registry: Option<&crate::types::assets::AssetRegistry>,
) {
    use crate::build::render::resolve_path_with_overrides;
    let AssetCopy { object_store, manifest_hash_memo, staged, transforms, image_config,
        canonical_source_root, converter_bases, prev_hashes_mtime_secs, .. } = copy;
    let prev_hashes_mtime_secs = *prev_hashes_mtime_secs;
    let source_root = Path::new(&ctx.source_path);
    let output_dir = &ctx.staging_dir;
    let prev_sources = &ctx.previous_hashes.sources;
    let markdown_exts = moss_core::page_kind::PAGE_EXTENSIONS;
    let video_exts = ["mov", "mp4", "webm", "avi", "mkv", "m4v"];
    // Symlink branch: preserve the symlink in the output instead of
    // copying bytes. Must run before the `is_file()` filter — symlinks
    // (whether to dirs or files) report `is_file() == false` under the
    // walker's default `follow_links(false)`.
    if file_type.is_symlink() {
        use crate::build::media::symlink::handle_symlink_entry;
        let outcome = handle_symlink_entry(
            file_path,
            source_root,
            canonical_source_root,
            output_dir,
        );
        if preserve_link(&outcome, site_hashes, &mut run.live_symlinks, "symlink") {
            run.stats.preserved_symlinks += 1;
        } else {
            run.stats.skipped_symlinks += 1;
        }
        return;
    }

    // Alias branch (macOS only): regular files that are Finder Bookmark
    // Aliases get resolved and written as POSIX symlinks in the output,
    // identical to how the symlink branch handles real symlinks.
    // Note: `ship_phase` (staging→site) sees the output as a real symlink and
    // handles it via its own symlink branch — no alias branch needed there.
    #[cfg(target_os = "macos")]
    if file_type.is_file() {
        use crate::build::media::symlink::handle_alias_entry;
        if let Some(outcome) = handle_alias_entry(
            file_path,
            source_root,
            canonical_source_root,
            output_dir,
        ) {
            // Count any non-Preserved alias outcome (escape/broken/
            // processable/absolute/unresolvable) as a skipped symlink.
            if preserve_link(&outcome, site_hashes, &mut run.live_symlinks, "alias") {
                run.stats.preserved_aliases += 1;
            } else {
                run.stats.skipped_symlinks += 1;
            }
            let blob = |path: &Path, rel: &str| source_blob(path, prev_sources.get(rel),
                prev_hashes_mtime_secs, object_store, &mut run.stats.cache_hits, &mut run.stats.cache_misses);
            register_alias_source(file_path, source_root, site_hashes, &mut run.live_source_keys, blob);
            return;
        }
        // Only genuine non-alias files fall through to byte copying.
    }

    if !file_type.is_file() {
        return;
    }

    // The scan's logical path, `/`-separated and composed (NFC): it is
    // compared with blocking keys, the prev-source asset cache and the
    // live-source prune map, all keyed that way. Reads use `file_path`.
    let relative_path = match file_path.strip_prefix(source_root) {
        Ok(rel) => logical_path(rel),
        Err(_) => return,
    };

    // Pre-Sonoma eviction (macOS 12–13, still a supported minimum) replaces
    // the asset with a hidden `.name.icloud` placeholder, so the real
    // filename never appears in this walk at all. Left alone that reads as
    // "the user deleted this photo": the key is missing from
    // `live_asset_keys`, `AssetWalkTally::prune` drops its manifest entry,
    // and `remove_stale_files` deletes the published file. Claiming the key
    // the stub stands for is what keeps the previous build's output alive
    // until the download lands — the asset-side counterpart of what
    // `scan.rs` does for pages via `evicted_paths`.
    if let Some(real) = crate::build::icloud::icloud_stub_target(file_path) {
        if let Ok(rel) = real.strip_prefix(source_root) {
            let rel = logical_path(rel);
            run.live_asset_keys.insert(resolve_path_with_overrides(&rel, &ctx.dir_overrides));
            run.live_source_keys.insert(rel.clone());
            crate::build::cloud_readiness::request_download(&real);
            log::debug!("[background-assets] Holding {} — still in the cloud (pre-Sonoma stub)", rel);
        }
        return;
    }

    // Skip OS / VCS metadata that should never reach a live site
    // (.DS_Store, .gitignore, Finder's `Icon\r`, …). The directory
    // exclusion rule in classify::is_excluded_dir_name only handles
    // dot-prefixed dirs; this catches the file-level cases.
    if let Some(file_name) = file_path.file_name().and_then(|n| n.to_str()) {
        if crate::build::scan::classify::is_os_metadata_file(file_name) {
            return;
        }
    }

    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Skip files handled by other phases.
    // Notebooks (.ipynb) are handled by run_notebook_processing in build.rs.
    if markdown_exts.contains(&ext.as_str()) || matches!(ext.as_str(), "pages" | "docx" | "doc" | "ipynb") {
        return;
    }

    // The video worker owns videos outside configured passthrough roots.
    let configured_passthrough = crate::build::scan::classify::is_in_passthrough(
        &relative_path,
        &ctx.passthrough_roots,
    );
    if video_exts.contains(&ext.as_str()) && !configured_passthrough {
        return;
    }

    // Skip root style.css / script.js — the canonical theme location is
    // .moss/theme/; a root-level file is not served, and
    // check_misplaced_theme_files() warns authors about the misplacement.
    // This must be the same predicate the sweep's drift walk uses
    // (`drift_eligible` in build_shell::watch::sweep) — see
    // is_ignored_root_theme_file's doc comment.
    if crate::build::render::is_ignored_root_theme_file(&relative_path) {
        return;
    }

    // The encoder plan is the authority for base ownership. A later stat
    // failure or cloud transition must never give this path a second writer.
    if converter_bases.contains(Path::new(&relative_path)) {
        return;
    }

    // Map asset path through dir_overrides so assets land at their
    // page-tree path (e.g., "交互/sketch.html" → "interactive/sketch.html")
    let mapped_path = resolve_path_with_overrides(&relative_path, &ctx.dir_overrides);

    // Claim before any deferred read: cloud-only sources still exist.
    run.live_asset_keys.insert(mapped_path.clone());
    // Also record the *source-relative* path for `sources` map pruning
    // below. Sources is keyed by source path (pre-mapping), unlike
    // `files` which is keyed by output path (post-mapping).
    run.live_source_keys.insert(relative_path.clone());

    match source_blob(file_path, prev_sources.get(&relative_path), prev_hashes_mtime_secs,
        object_store, &mut run.stats.cache_hits, &mut run.stats.cache_misses)
    {
        Ok(None) => return,
        Ok(Some((oid, src_stat))) => {
            // Raster originals retain their format at the optimized size.
            // Unsupported or unavailable inputs use the verbatim CAS blob;
            // warm transform records can be reused without reading the source.
            let link_oid = if matches!(ext.as_str(), "jpg" | "jpeg" | "png") {
                crate::build::media::fallback_raster::sized_raster_oid_for_original(
                    file_path,
                    &oid,
                    object_store,
                    transforms,
                    image_config,
                    crate::build::media::fallback_raster::SIZED_JPEG_QUALITY,
                    Some((staged, &output_dir.join(&mapped_path))),
                )
                .unwrap_or_else(|| oid.clone())
            } else {
                oid.clone()
            };

            let target = output_dir.join(&mapped_path);
            let placement =
                BlobPlacement { link_oid: &link_oid, target: &target, log_label: &mapped_path, report_path: &mapped_path, ext: &ext, local: (link_oid == oid).then_some(file_path) };
            if !place_blob(object_store, staged, placement, reporter, &mut run.stats) {
                return;
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

            // Inject defaults only into bundled SPA indexes outside passthrough
            // roots, preserving author metadata. Track the post-injection CAS
            // object so deployment reads the same bytes as staging.
            let mut spa_post_oid: Option<String> = None;
            if !configured_passthrough
                && matches!(ext.as_str(), "html" | "htm")
                && is_bundled_spa_index_path(&mapped_path)
            {
                if let Some(ref defaults) = ctx.spa_defaults {
                    let canonical_url_for_folder = derive_canonical_url(defaults, &mapped_path);
                    let borrowed = defaults.as_borrowed(canonical_url_for_folder.as_deref());
                    match maybe_inject_spa_cached(
                        &target,
                        &borrowed,
                        &oid,
                        src_stat.size,
                        object_store,
                        transforms,
                    ) {
                        Ok(Some((_, new_oid))) => {
                            spa_post_oid = Some(new_oid);
                        }
                        Ok(None) => {} // no-op (nothing to inject)
                        Err(e) => {
                            // Fail open — leave the file alone, don't break the build.
                            log::info!(
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
            site_hashes.sources.insert(
                relative_path.clone(),
                crate::build::types::SourceMetadata::from_stat(oid.clone(), src_stat),
            );
            match crate::build::served_path::ServedPath::from_source(&mapped_path) {
                Ok(out_path) => {
                    // The CAS object actually backing `target`'s bytes: the
                    // post-injection object when SPA injection changed
                    // them, otherwise the object `link_to` placed there
                    // (`link_oid`) — `apply_transform`/`transform_for` are
                    // pure and path-only, so a later CAS read reproduces
                    // exactly what reading `target` now would.
                    let staged_oid = spa_post_oid.unwrap_or(link_oid);
                    let hash = OutputHash {
                        memo_key: &staged_oid,
                        target: &target,
                        fallback_oid: &oid,
                        log_context: "output",
                    };
                    record_blob(manifest_hash_memo, site_hashes, &mut run.staged_oids, hash, &out_path, &staged_oid, &ctx.mount);
                    run.stats.copied += 1;
                }
                Err(e) => {
                    log::warn!("[background-assets] Skipping file with invalid path '{}': {}", mapped_path, e);
                    run.stats.skipped += 1;
                }
            }
        }
        Err(e) => {
            log::info!("[background-assets] Failed to store {}: {}", relative_path, e);
            // Fallback: direct copy without CAS
            let target = output_dir.join(&mapped_path);
            if let Err(copy_err) = crate::build::io_utils::copy_output(file_path, &target) {
                log::error!(
                    "[background-assets] Direct copy also failed for {}: {}",
                    relative_path, copy_err
                );
            }
            run.stats.skipped += 1;
        }
    }
}

/// Walk remaining assets, then replace the source cache with both steps' live records.
pub(crate) fn copy_deferred_assets(
    ctx: &BackgroundContext,
    html: StagedHtml,
    reporter: &dyn crate::build::ports::reporter::BuildReporter,
    tx: mpsc::Sender<EmitMessage>,
    asset_registry: Option<std::sync::Arc<crate::types::assets::AssetRegistry>>,
) -> AssetPipelineRunStats {
    use walkdir::WalkDir;
    let _phase = crate::build::phase::PhaseTrace::start("background_asset_copy");
    let source_root = Path::new(&ctx.source_path);
    let output_dir = &ctx.staging_dir;
    let deferred_paths = MossPaths::from_moss_dir(ctx.moss_dir.clone());
    let mut site_hashes = ctx.previous_hashes.clone();
    let mut copy = html.copy;
    let mut run = AssetWalkTally { stats: html.stats, ..Default::default() };
    let layer_sources = crate::config::map_layers::declared_sources(source_root);
    for entry in WalkDir::new(source_root)
        .sort_by(crate::build::scan::path_collision::nfc_name_order)
        .into_iter()
        .filter_entry(|e| crate::build::scan::classify::left_out_with_layer_sources(e, source_root, &layer_sources).is_none())
    {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                crate::build::cli_output::log_note!("[background-assets] Walk error: {}", e);
                continue;
            }
        };
        if copy.handles_source_html(entry.path()) { continue; }
        copy_asset_entry(ctx, &mut copy, entry.path(), entry.file_type(), &mut site_hashes, &mut run, reporter, asset_registry.as_deref());
    }
    let AssetCopy { object_store, manifest_hash_memo, mut staged, prev_hashes_mtime_secs, .. } = copy;
    let prev_sources = &ctx.previous_hashes.sources;
    let emit = |msg| {
        if let Err(e) = tx.blocking_send(msg) {
            log::info!("[background-assets] Failed to send to coordinator: {e}");
        }
    };

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
                    crate::build::cli_output::log_note!("[background-assets] .moss/theme walk error: {}", e);
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let file_path = entry.path();
            let stub_target = crate::build::icloud::icloud_stub_target(file_path);
            let effective = stub_target.as_deref().unwrap_or(file_path);
            let relative_path = match effective.strip_prefix(&moss_assets_dir) {
                Ok(rel) => logical_path(rel),
                Err(_) => continue,
            };

            // Guard: skip style.css and script.js — these are emitted by the
            // blocking phase (as _moss/theme/style.css and _moss/theme/script.js).
            // All other files, including any user-named custom.css/custom.js,
            // mirror through verbatim under _moss/theme/.
            if relative_path == "style.css" || relative_path == "script.js" {
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
                    run.stats.skipped += 1;
                    continue;
                }
            };

            run.live_asset_keys.insert(out_path.as_str().to_string());

            let source_key = format!(".moss/theme/{relative_path}");
            run.live_source_keys.insert(source_key.clone());
            if stub_target.is_some() {
                crate::build::cloud_readiness::request_download(effective);
                continue;
            }
            match source_blob(file_path, prev_sources.get(&source_key), prev_hashes_mtime_secs,
                &object_store, &mut run.stats.cache_hits, &mut run.stats.cache_misses)
            {
                Ok(None) => continue,
                Ok(Some((oid, stat))) => {
                    let target = out_path.to_disk(output_dir);
                    let moss_ext = file_path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let log_label = format!(".moss/theme/{}", relative_path);
                    let placement =
                        BlobPlacement { link_oid: &oid, target: &target, log_label: &log_label, report_path: out_path.as_str(), ext: &moss_ext, local: Some(file_path) };
                    if !place_blob(&object_store, &mut staged, placement, reporter, &mut run.stats) {
                        continue;
                    }
                    site_hashes.sources.insert(source_key,
                        crate::build::types::SourceMetadata::from_stat(oid.clone(), stat));
                    let hash =
                        OutputHash { memo_key: &oid, target: &target, fallback_oid: &oid, log_context: ".moss/theme output" };
                    record_blob(&manifest_hash_memo, &mut site_hashes, &mut run.staged_oids, hash, &out_path, &oid, &ctx.mount);
                    run.stats.copied += 1;
                }
                Err(e) => {
                    log::info!("[background-assets] Failed to store .moss/theme/{}: {}", relative_path, e);
                    run.stats.skipped += 1;
                }
            }
        }
    }

    // Only live source assets and preserved links may re-register prior entries.
    // Rendered pages and media-worker outputs belong to the coordinator.
    run.prune(&mut site_hashes);

    // Stale files, dirs and symlinks leave staging only through the build's
    // permitted sweep (`pipeline::sweep_staging`), never from this worker.

    // Re-register every live asset, including unchanged carry-forwards:
    // the manifest's mark-and-sweep keeps only paths touched this build.
    for msg in file_registrations(&site_hashes.files, &run.staged_oids) {
        emit(msg);
    }
    log::debug!(
        "[background-assets] Sent {} file entries to coordinator",
        site_hashes.files.len()
    );

    // The remaining pass includes the HTML source cache handed over by the
    // early pass, so this replacement covers both phases and prunes deletions.
    site_hashes.sources.extend(html.sources);
    let sources_payload = std::mem::take(&mut site_hashes.sources);
    emit(EmitMessage::SourcesReplace(sources_payload));

    staged.save();
    // Persist the oid→xxh3 memo once per build (not once per asset — see
    // `ManifestHashMemo`'s doc comment). Best-effort: a failed save just
    // means the next build starts cold, same as today.
    if let Err(e) = manifest_hash_memo.save(&deferred_paths.cache_manifest_hash_memo()) {
        log::info!("[background-assets] Failed to persist manifest-hash memo: {}", e);
    }
    manifest_hash_memo.log_summary();

    crate::build::phase::record_count("background_assets_copied", run.stats.copied as usize);
    crate::build::phase::record_count("background_assets_hits", run.stats.cache_hits as usize);
    crate::build::phase::record_count("background_assets_misses", run.stats.cache_misses as usize);
    crate::build::phase::record_count("background_assets_stat_only", run.stats.held as usize);
    crate::build::phase::record_count("background_assets_linked", run.stats.linked as usize);
    log::info!("{}", format_asset_run_summary(&run.stats));
    run.stats
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
pub(crate) mod tests;
