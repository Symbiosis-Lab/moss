//! Symlink preservation for the build pipeline.
//!
//! Source walkers do not follow symlinks (default `walkdir::WalkDir` behavior).
//! When the asset copy phase encounters a symlink entry, it calls
//! [`handle_symlink_entry`] to recreate the symlink in the output instead of
//! copying bytes. The webserver dereferences at request time, so both the
//! alias path and the canonical path serve the same bytes.
//!
//! Design: docs/archive/2026-04-27-preserve-source-symlinks-design.md

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Returns true if the file at `path` looks like a macOS Finder Bookmark
/// Data file (the modern Alias format). Detection is by magic bytes: the
/// first 4 bytes spell `book`. Cheap — reads at most 4 bytes.
///
/// Platform-agnostic: detection works on every OS because it's just byte
/// inspection. Resolution (`try_resolve_alias`) is macOS-only via
/// CoreFoundation. Non-macOS callers use this to *warn-and-skip* alias files
/// from a Mac-synced repo rather than copying them as 1KB binary blobs into
/// the published output.
///
/// False positives are vanishingly rare (a 4-byte sequence `book` at file
/// start) and harmless: a downstream resolution attempt will fail and the
/// file will be treated as a regular file.
pub(crate) fn is_bookmark_alias_file(path: &Path) -> bool {
    use std::io::Read;
    let mut buf = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut buf))
        .map(|_| &buf == b"book")
        .unwrap_or(false)
}

/// Resolve a macOS Bookmark Data file at `path` to its target's path.
/// Returns `None` if the file is not a bookmark OR if a bookmark's target
/// cannot be resolved. macOS-only.
///
/// Callers that must distinguish "not a bookmark" (fall through) from
/// "bookmark, but resolution failed" (skip — never blob-copy) should use
/// [`probe_alias`] instead. Collapsing both into `None` here is exactly what
/// let an unresolvable iCloud-dehydrated alias fall through to a raw blob
/// copy — the 2026-06-03 `city-heat-map` deploy failure.
///
/// Production now goes through [`probe_alias`]; this 2-state wrapper is
/// retained for tests and as the documented "collapsed" contract.
#[cfg(target_os = "macos")]
#[allow(dead_code)] // production uses probe_alias; kept for tests + doc
pub(crate) fn try_resolve_alias(path: &Path) -> Option<PathBuf> {
    if !is_bookmark_alias_file(path) {
        return None;
    }
    resolve_bookmark_data(path)
}

/// 3-state probe of a regular file for being a macOS Finder Alias.
#[cfg(target_os = "macos")]
enum AliasProbe {
    /// Not a bookmark file — caller should fall through to regular-file handling.
    NotAnAlias,
    /// A bookmark file that resolved to this target path.
    Resolved(PathBuf),
    /// A genuine bookmark file whose target could not be resolved (broken,
    /// cross-volume, or — the common case — iCloud-dehydrated).
    Unresolvable,
}

/// Distinguish "not a bookmark" from "is a bookmark, resolution failed".
/// `try_resolve_alias` collapses both into `None`; keeping them apart lets
/// the caller fall through only for genuine non-aliases and skip (never
/// blob-copy) an unresolvable alias.
#[cfg(target_os = "macos")]
fn probe_alias(path: &Path) -> AliasProbe {
    if !is_bookmark_alias_file(path) {
        return AliasProbe::NotAnAlias;
    }
    match resolve_bookmark_data(path) {
        Some(target) => AliasProbe::Resolved(target),
        None => AliasProbe::Unresolvable,
    }
}

/// Resolve the bookmark payload of a file already confirmed (by magic bytes)
/// to be a Finder Alias. Returns the target's path, or `None` on any
/// resolution failure. macOS-only.
///
/// Uses `CFURLCreateBookmarkDataFromFile` to extract the embedded bookmark
/// from the Finder Alias container format, then resolves via
/// `CFURLCreateByResolvingBookmarkData` with the `WithoutUI` and
/// `WithoutMounting` flags so we never block on user prompts or network /
/// iCloud-download operations during a build. (That is also precisely why an
/// iCloud-dehydrated target resolves to `None` — see [`probe_alias`].)
///
/// Note: Finder Alias files use a container format; the bookmark payload
/// cannot be read as raw bytes with `CFData::from_buffer`. We must use
/// `CFURLCreateBookmarkDataFromFile` to extract it.
#[cfg(target_os = "macos")]
fn resolve_bookmark_data(path: &Path) -> Option<PathBuf> {
    use core_foundation::{base::TCFType, data::CFData, url::CFURL};
    use core_foundation_sys::base::{kCFAllocatorDefault, Boolean};
    use core_foundation_sys::error::CFErrorRef;
    use core_foundation_sys::url::{
        kCFURLBookmarkResolutionWithoutMountingMask,
        kCFURLBookmarkResolutionWithoutUIMask,
        CFURLCreateBookmarkDataFromFile,
        CFURLCreateByResolvingBookmarkData,
    };
    use std::ptr;

    // Step 1: Convert path to CFURL and extract bookmark data from the alias file.
    // Finder Alias files have a container format — the bookmark payload is
    // extracted by CFURLCreateBookmarkDataFromFile, not by reading raw bytes.
    let alias_url = CFURL::from_path(path, false)?;
    // SAFETY: `alias_url` is a live `CFURL` whose +1 reference is held for the
    // duration of this block (it's owned by the Rust binding and dropped at
    // end of scope). `err` is a stack-allocated out-pointer initialized to
    // null. `CFURLCreateBookmarkDataFromFile` follows Apple's Create Rule:
    // on success it returns a +1 `CFDataRef` (consumed below by
    // `wrap_under_create_rule`); on failure it writes a +1 `CFErrorRef` to
    // `err`, which we `CFRelease` to avoid leaking on persistently-broken
    // aliases (relevant in long-lived watch mode).
    let bookmark_data = unsafe {
        let mut err: CFErrorRef = ptr::null_mut();
        let data_ref = CFURLCreateBookmarkDataFromFile(
            kCFAllocatorDefault,
            alias_url.as_concrete_TypeRef(),
            &mut err,
        );
        if data_ref.is_null() {
            if !err.is_null() {
                use core_foundation_sys::base::CFRelease;
                CFRelease(err as *const _);
            }
            return None;
        }
        CFData::wrap_under_create_rule(data_ref)
    };

    // Step 2: Resolve the bookmark to its target path.
    let opts = kCFURLBookmarkResolutionWithoutUIMask
        | kCFURLBookmarkResolutionWithoutMountingMask;

    // SAFETY: `bookmark_data` is a live `CFData` (+1 retained by the Rust
    // binding, dropped at end of scope). `is_stale` and `error` are
    // stack-allocated out-pointers. `relativeToURL` and
    // `resourcePropertiesToInclude` are `null` (allowed per Apple docs).
    // `CFURLCreateByResolvingBookmarkData` follows the Create Rule: on
    // success returns a +1 `CFURLRef` consumed by `wrap_under_create_rule`;
    // on failure writes a +1 `CFErrorRef` we `CFRelease`.
    unsafe {
        let mut is_stale: Boolean = 0;
        let mut error: CFErrorRef = ptr::null_mut();
        let url_ref = CFURLCreateByResolvingBookmarkData(
            ptr::null_mut(),
            bookmark_data.as_concrete_TypeRef(),
            opts,
            ptr::null(),  // relativeToURL
            ptr::null(),  // resourcePropertiesToInclude
            &mut is_stale,
            &mut error,
        );
        if url_ref.is_null() {
            if !error.is_null() {
                use core_foundation_sys::base::CFRelease;
                CFRelease(error as *const _);
            }
            return None;
        }
        let url = CFURL::wrap_under_create_rule(url_ref);
        url.to_path()
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn try_resolve_alias(_path: &Path) -> Option<PathBuf> {
    None
}

/// Outcome of attempting to preserve a single symlink entry.
#[derive(Debug, PartialEq, Eq)]
pub enum SymlinkOutcome {
    /// Symlink was successfully recreated at the output path.
    Preserved { rel_path: String, target: PathBuf },
    /// Symlink target escapes the site root.
    SkippedEscape { rel_path: String },
    /// Symlink target is broken or unreadable.
    SkippedBroken { rel_path: String },
    /// File symlink to a moss-processable file (.md/.ipynb/.qmd).
    SkippedProcessable { rel_path: String, ext: String },
    /// Symlink target is an absolute path. The embedded build-machine prefix
    /// (`/Users/...`, `/home/...`) will not exist on the deploy host, so the
    /// symlink would always 404. Warn-and-skip at build time rather than
    /// ship a known-broken artifact.
    SkippedAbsoluteTarget { rel_path: String, target: PathBuf },
    /// Windows: symlink preservation not supported.
    #[allow(dead_code)] // only constructed on Windows
    SkippedUnsupportedPlatform { rel_path: String },
    /// macOS Finder Alias whose target could not be resolved — almost always
    /// because the target is iCloud-dehydrated and the WithoutMounting +
    /// WithoutUI resolution flags deliberately will not download it (also
    /// broken / cross-volume targets). We MUST NOT fall through to copying
    /// the ~1 KB alias file verbatim: that publishes a garbage blob AND, for
    /// a path previously published as a symlink, collides with it on the
    /// server (the 2026-06-03 `city-heat-map` EISDIR deploy failure). Skip +
    /// warn + count instead, exactly like the non-macOS branch.
    #[allow(dead_code)] // only constructed on macOS
    SkippedUnresolvable { rel_path: String },
}

/// Extensions that moss processes through the page-render pipeline.
/// File symlinks pointing at these are warn-and-skipped (preserving them
/// would surface as a raw download to the browser — worse than no entry).
pub const PROCESSABLE_EXTENSIONS: &[&str] = &["md", "ipynb", "qmd"];

/// Lowercase the directory segments of a relative symlink target so the
/// stored target matches output paths emitted by `slugify_dir_path`. The
/// output tree lowercases every dir segment of every emitted asset
/// ([scan::slug::slugify_dir_path]); a symlink whose target keeps the
/// source casing (e.g. `Resources/...`) points at a path that does not
/// exist on the deployed (case-sensitive) filesystem and 404s.
///
/// If `target_is_dir`, every segment is a dir → all lowercased.
/// Otherwise the basename is preserved verbatim — matches the
/// "asset filenames preserved" rule in `slugify_dir_path` (third-party
/// bundles load `MathJax_Main-Bold.woff` by exact name).
///
/// `.` / `..` and empty segments pass through unchanged. Absolute paths
/// are returned as-is: an absolute symlink target embeds the build
/// machine's path and is already broken-for-deploy regardless of case,
/// out of scope for this normalization.
fn slugify_symlink_target(target: &Path, target_is_dir: bool) -> PathBuf {
    if target.is_absolute() {
        return target.to_path_buf();
    }
    // Non-UTF-8 relative targets fall through unchanged. In practice
    // unreachable (the source repos are git/iCloud-synced text paths) and
    // matches the pre-fix verbatim status quo if it ever does occur.
    let s = match target.to_str() {
        Some(s) => s,
        None => return target.to_path_buf(),
    };
    let segments: Vec<&str> = s.split('/').collect();
    let last_idx = segments.len().saturating_sub(1);
    let out: Vec<String> = segments
        .iter()
        .enumerate()
        .map(|(i, seg)| {
            if seg.is_empty() || *seg == "." || *seg == ".." {
                seg.to_string()
            } else if !target_is_dir && i == last_idx {
                seg.to_string()
            } else {
                seg.to_lowercase()
            }
        })
        .collect();
    PathBuf::from(out.join("/"))
}

/// Preserve a symlink entry from the source tree to the output tree.
///
/// Returns the outcome so the caller can update bookkeeping (e.g.,
/// `live_symlinks`). All log messages are emitted from inside this function.
pub fn handle_symlink_entry(
    entry_path: &Path,
    source_root: &Path,
    canonical_root: &Path,
    output_root: &Path,
) -> SymlinkOutcome {
    let rel_path = match entry_path.strip_prefix(source_root) {
        Ok(rel) => rel.to_string_lossy().to_string(),
        Err(_) => {
            log::warn!("[copy] symlink entry not under source_root; skipping: {}", entry_path.display());
            return SymlinkOutcome::SkippedBroken { rel_path: entry_path.display().to_string() };
        }
    };

    #[cfg(unix)]
    {
        handle_symlink_unix(entry_path, &rel_path, canonical_root, output_root)
    }

    #[cfg(windows)]
    {
        let _ = (canonical_root, output_root);
        log::warn!("[copy] Symlink preservation not supported on Windows; skipping {}", rel_path);
        SymlinkOutcome::SkippedUnsupportedPlatform { rel_path }
    }
}

#[cfg(unix)]
fn handle_symlink_unix(
    entry_path: &Path,
    rel_path: &str,
    canonical_root: &Path,
    output_root: &Path,
) -> SymlinkOutcome {
    use std::fs;

    // Step 1: read the symlink target as stored. Dir segments are lowercased
    // by `slugify_symlink_target` below (Step 9) so the stored target matches
    // the output tree's `slugify_dir_path` casing; file basenames are
    // preserved verbatim.
    let target = match fs::read_link(entry_path) {
        Ok(t) => t,
        Err(e) => {
            log::warn!("[copy] read_link failed for {}: {}", rel_path, e);
            return SymlinkOutcome::SkippedBroken { rel_path: rel_path.to_string() };
        }
    };

    // Step 2: reject absolute targets. The embedded build-machine prefix
    // won't exist on the deploy host, so preserving the symlink would
    // produce a guaranteed 404 (regardless of containment or case). Same
    // policy as `SkippedEscape` / `SkippedProcessable`: surface at build
    // time, don't ship the broken artifact.
    if target.is_absolute() {
        log::warn!(
            "[copy] Skipping symlink with absolute target (won't resolve on deploy): {} -> {}",
            rel_path, target.display()
        );
        return SymlinkOutcome::SkippedAbsoluteTarget {
            rel_path: rel_path.to_string(),
            target,
        };
    }

    // Step 3: resolve target for the containment check.
    // Target is guaranteed relative by Step 2; `entry_path.parent().join(target)`
    // is the absolute path the symlink resolves to.
    let resolved = entry_path
        .parent()
        .map(|p| p.join(&target))
        .unwrap_or_else(|| target.clone());

    // Step 4: canonicalize once; on failure, treat as broken.
    let canonical_target = match resolved.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[copy] canonicalize failed for symlink {} -> {}: {}", rel_path, target.display(), e);
            return SymlinkOutcome::SkippedBroken { rel_path: rel_path.to_string() };
        }
    };

    // Step 5: containment check.
    if !canonical_target.starts_with(canonical_root) {
        log::warn!(
            "[copy] Skipping symlink that escapes site root: {} -> {} (resolved: {})",
            rel_path, target.display(), canonical_target.display()
        );
        return SymlinkOutcome::SkippedEscape { rel_path: rel_path.to_string() };
    }

    // Step 6: reject moss-processable file symlinks.
    if canonical_target.is_file() {
        let ext = canonical_target
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if PROCESSABLE_EXTENSIONS.contains(&ext.as_str()) {
            log::warn!(
                "[copy] File symlink to moss-processable content not supported (use frontmatter permalink instead): {} -> {}",
                rel_path, target.display()
            );
            return SymlinkOutcome::SkippedProcessable {
                rel_path: rel_path.to_string(),
                ext,
            };
        }
    }

    // Step 7: compute disk destination path.
    let dest_path = output_root.join(rel_path);

    // Step 8: remove any existing entry at the disk destination.
    if let Err(e) = remove_existing(&dest_path) {
        log::warn!("[copy] failed to clear output path {}: {}", dest_path.display(), e);
        return SymlinkOutcome::SkippedBroken { rel_path: rel_path.to_string() };
    }

    // Ensure parent directory exists.
    if let Some(parent) = dest_path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            log::warn!("[copy] failed to create parent dir for {}: {}", dest_path.display(), e);
            return SymlinkOutcome::SkippedBroken { rel_path: rel_path.to_string() };
        }
    }

    // Step 9: slugify the relative target's dir segments to match the
    // lowercasing applied to output paths by `slugify_dir_path`. Without
    // this, a symlink whose stored target preserves source casing
    // (e.g. `Resources/...`) points at a path that doesn't exist on the
    // deployed (case-sensitive) filesystem.
    let slugified_target = slugify_symlink_target(&target, canonical_target.is_dir());

    // Step 10: recreate the symlink.
    if let Err(e) = std::os::unix::fs::symlink(&slugified_target, &dest_path) {
        log::warn!("[copy] failed to create symlink {} -> {}: {}", dest_path.display(), slugified_target.display(), e);
        return SymlinkOutcome::SkippedBroken { rel_path: rel_path.to_string() };
    }

    // Per-symlink success; the asset-copy summary reports the counts.
    log::debug!("[copy] Preserved symlink: {} -> {}", rel_path, slugified_target.display());

    SymlinkOutcome::Preserved {
        rel_path: rel_path.to_string(),
        target: slugified_target,
    }
}

/// macOS-only: handle a regular file that is a Finder Bookmark Alias.
/// Resolves it, runs the same containment + processable checks as
/// `handle_symlink_entry`, and writes a real POSIX symlink in the output.
///
/// Returns `None` ONLY if the file is a genuine non-alias (caller should fall
/// through to regular-file handling). Returns `Some(SymlinkOutcome)` when it
/// is an alias — including `SkippedUnresolvable` when it is a real bookmark
/// whose target won't resolve (iCloud-dehydrated / broken). The unresolvable
/// case MUST be `Some` so the caller does not fall through and blob-copy the
/// ~1 KB alias file as a regular MODE_FILE entry (the 2026-06-03 bug).
#[cfg(target_os = "macos")]
pub fn handle_alias_entry(
    entry_path: &Path,
    source_root: &Path,
    canonical_root: &Path,
    output_root: &Path,
) -> Option<SymlinkOutcome> {
    let target = match probe_alias(entry_path) {
        // Genuine non-alias: caller falls through to regular-file copy.
        AliasProbe::NotAnAlias => return None,
        AliasProbe::Resolved(target) => target,
        // Genuine bookmark, unresolvable target. Do NOT fall through to a raw
        // copy — that publishes a garbage blob and, for a path previously
        // published as a symlink, collides with it on the server (EISDIR).
        // Skip + warn + count, exactly like the non-macOS branch below.
        AliasProbe::Unresolvable => {
            let rel_path = entry_path
                .strip_prefix(source_root)
                .map(|r| r.to_string_lossy().to_string())
                .unwrap_or_else(|_| entry_path.display().to_string());
            log::warn!(
                "[copy] macOS Finder Alias could not be resolved — is the target \
                 downloaded from iCloud? (turn off 'Optimize Mac Storage' for the \
                 folder, or right-click → Download Now, then re-publish). Skipping \
                 rather than publishing a broken blob: {}",
                rel_path
            );
            return Some(SymlinkOutcome::SkippedUnresolvable { rel_path });
        }
    };

    let rel_path = match entry_path.strip_prefix(source_root) {
        Ok(rel) => rel.to_string_lossy().to_string(),
        Err(_) => return Some(SymlinkOutcome::SkippedBroken {
            rel_path: entry_path.display().to_string()
        }),
    };

    // Containment: target must be inside canonical_root
    let canonical_target = match target.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[copy] canonicalize failed for alias {}: {}", rel_path, e);
            return Some(SymlinkOutcome::SkippedBroken { rel_path });
        }
    };
    if !canonical_target.starts_with(canonical_root) {
        log::warn!(
            "[copy] Skipping alias that escapes site root: {} -> {}",
            rel_path, canonical_target.display()
        );
        return Some(SymlinkOutcome::SkippedEscape { rel_path });
    }

    // Processable-extension rejection (same as symlinks)
    if canonical_target.is_file() {
        let ext = canonical_target
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if PROCESSABLE_EXTENSIONS.contains(&ext.as_str()) {
            log::warn!(
                "[copy] Alias to moss-processable content not supported: {} -> {}",
                rel_path, canonical_target.display()
            );
            return Some(SymlinkOutcome::SkippedProcessable { rel_path, ext });
        }
    }

    // Output: write a relative POSIX symlink. Compute relative target from
    // the alias's parent dir so the output symlink survives moves.
    let alias_parent_canonical = entry_path
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .unwrap_or_else(|| canonical_root.to_path_buf());
    let relative_target = pathdiff::diff_paths(&canonical_target, &alias_parent_canonical)
        .unwrap_or_else(|| canonical_target.clone());

    // Reject absolute targets. Under normal operation `pathdiff` always
    // produces a relative path (both sides are canonicalized and inside
    // `canonical_root`). The fallback to `canonical_target` (absolute) is
    // defensive against canonicalize edge cases; if we ever hit it, the
    // synthesized symlink would 404 on deploy — fail loudly instead.
    if relative_target.is_absolute() {
        log::warn!(
            "[copy] Skipping alias whose synthesized target is absolute (won't resolve on deploy): {} -> {}",
            rel_path, relative_target.display()
        );
        return Some(SymlinkOutcome::SkippedAbsoluteTarget {
            rel_path,
            target: relative_target,
        });
    }

    // Slugify dir segments so the stored target matches `slugify_dir_path`'s
    // lowercased output tree (the alias resolves on case-insensitive macOS
    // but 404s on case-sensitive deploy filesystems otherwise).
    let relative_target = slugify_symlink_target(&relative_target, canonical_target.is_dir());

    let dest_path = output_root.join(&rel_path);
    if let Err(e) = remove_existing(&dest_path) {
        log::warn!("[copy] failed to clear output path {}: {}", dest_path.display(), e);
        return Some(SymlinkOutcome::SkippedBroken { rel_path });
    }
    if let Some(parent) = dest_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("[copy] failed to create parent dir for {}: {}", dest_path.display(), e);
            return Some(SymlinkOutcome::SkippedBroken { rel_path });
        }
    }
    if let Err(e) = std::os::unix::fs::symlink(&relative_target, &dest_path) {
        log::warn!("[copy] failed to create symlink for alias {}: {}", rel_path, e);
        return Some(SymlinkOutcome::SkippedBroken { rel_path });
    }

    // Per-alias success; the asset-copy summary reports the counts.
    log::debug!(
        "[copy] Resolved alias to symlink: {} -> {}",
        rel_path, relative_target.display()
    );
    Some(SymlinkOutcome::Preserved {
        rel_path,
        target: relative_target,
    })
}

#[cfg(not(target_os = "macos"))]
pub fn handle_alias_entry(
    entry_path: &Path,
    source_root: &Path,
    _canonical_root: &Path,
    _output_root: &Path,
) -> Option<SymlinkOutcome> {
    // Detection runs on every platform; resolution is macOS-only. On
    // Linux/Windows, a Finder Alias file from a Mac-synced repo would
    // otherwise be copied as a 1KB binary blob and served with a 200,
    // which is worse than skipping it. Detect by magic bytes and skip.
    if !is_bookmark_alias_file(entry_path) {
        return None;
    }
    let rel_path = entry_path
        .strip_prefix(source_root)
        .map(|r| r.to_string_lossy().to_string())
        .unwrap_or_else(|_| entry_path.display().to_string());
    log::warn!(
        "[copy] Found macOS Finder Alias '{}' on a non-macOS build; skipping. \
         Resolve and re-publish from a Mac, or replace with a POSIX symlink.",
        rel_path
    );
    Some(SymlinkOutcome::SkippedUnsupportedPlatform { rel_path })
}

/// Clear any existing entry at `path` so the caller can write a fresh
/// symlink there. Distinguishes directories (need `remove_dir_all`) from
/// files/symlinks (`remove_file`). NotFound is a no-op.
///
/// Used by both `handle_symlink_entry` and `sync_dir`'s symlink branch.
/// Centralizing the logic ensures consistent error semantics — in particular,
/// a `remove_dir_all` failure surfaces as an `Err` rather than being
/// silently swallowed.
#[cfg(unix)]
pub(crate) fn remove_existing(path: &Path) -> std::io::Result<()> {
    use std::fs;
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_dir() {
                fs::remove_dir_all(path)
            } else {
                // file, symlink, or other — remove_file handles all of these
                fs::remove_file(path)
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Walk the output tree and remove symlinks not in `live_symlinks`.
/// Must use `follow_links(false)` to avoid descending into the symlinks
/// (which would recurse into the canonical content and risk deleting it).
pub fn remove_stale_symlinks(output_root: &Path, live_symlinks: &HashSet<String>) {
    use walkdir::WalkDir;
    let mut removed = 0u32;
    for entry in WalkDir::new(output_root).into_iter() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        // Critical: filter on path_is_symlink (file_type().is_symlink())
        // BEFORE descending. Default follow_links(false) already enforces
        // no descent into the symlink, but we double-check.
        if !entry.path_is_symlink() {
            continue;
        }
        let rel = match entry.path().strip_prefix(output_root) {
            Ok(r) => r.to_string_lossy().to_string(),
            Err(_) => continue,
        };
        if !live_symlinks.contains(&rel) {
            match std::fs::remove_file(entry.path()) {
                Ok(_) => {
                    log::debug!("[stale-symlinks] Removed: {}", rel);
                    removed += 1;
                }
                Err(e) => {
                    log::debug!("[stale-symlinks] Could not remove {}: {}", rel, e);
                }
            }
        }
    }
    if removed > 0 {
        log::debug!("[stale-symlinks] Removed {} stale symlink(s)", removed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper: build a temp dir with the given layout, return (source_root, output_root).
    /// Layout is a list of `(relative_path, FileSpec)` pairs.
    enum FileSpec<'a> {
        File(&'a str),
        Dir,
        #[cfg(unix)]
        Symlink(&'a str), // relative target spelling
    }

    fn make_fixture(layout: &[(&str, FileSpec)]) -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
        // House rule: test temp artifacts live inside the repo, never /tmp —
        // parallel worktrees collide in system temp and iCloud can zero
        // home-dir artifacts mid-run. Precedent: build/features.rs, onboarding.rs.
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-tmp");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = TempDir::new_in(&base).unwrap();
        let source = tmp.path().join("source");
        let output = tmp.path().join("output");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&output).unwrap();
        for (rel, spec) in layout {
            let path = source.join(rel);
            match spec {
                FileSpec::File(content) => {
                    if let Some(p) = path.parent() {
                        fs::create_dir_all(p).unwrap();
                    }
                    fs::write(&path, content).unwrap();
                }
                FileSpec::Dir => {
                    fs::create_dir_all(&path).unwrap();
                }
                #[cfg(unix)]
                FileSpec::Symlink(target) => {
                    if let Some(p) = path.parent() {
                        fs::create_dir_all(p).unwrap();
                    }
                    std::os::unix::fs::symlink(target, &path).unwrap();
                }
            }
        }
        (tmp, source, output)
    }

    #[cfg(unix)]
    #[test]
    fn preserves_directory_symlink_to_inside_root() {
        let (_tmp, source, output) = make_fixture(&[
            ("resources/app", FileSpec::Dir),
            ("resources/app/index.html", FileSpec::File("<h1>app</h1>")),
            ("myapp", FileSpec::Symlink("resources/app")),
        ]);
        let canonical_root = source.canonicalize().unwrap();
        let entry = source.join("myapp");

        let outcome = handle_symlink_entry(&entry, &source, &canonical_root, &output);

        assert!(matches!(outcome, SymlinkOutcome::Preserved { .. }), "expected Preserved, got {:?}", outcome);
        let out_link = output.join("myapp");
        let meta = fs::symlink_metadata(&out_link).expect("output symlink should exist");
        assert!(meta.file_type().is_symlink(), "output entry should be a symlink");
        let read_target = fs::read_link(&out_link).unwrap();
        assert_eq!(read_target, std::path::PathBuf::from("resources/app"));
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlink_to_outside_root() {
        // Use a relative `..` traversal — absolute targets are rejected
        // earlier by `SkippedAbsoluteTarget`, so the escape check only
        // runs for relative paths that traverse out of the source root.
        // Materialize the sibling target so canonicalize() succeeds and we
        // exercise the containment check rather than `SkippedBroken`.
        let (tmp, source, output) = make_fixture(&[
            ("escape", FileSpec::Symlink("../sibling")),
        ]);
        std::fs::create_dir(tmp.path().join("sibling")).unwrap();
        let canonical_root = source.canonicalize().unwrap();
        let entry = source.join("escape");

        let outcome = handle_symlink_entry(&entry, &source, &canonical_root, &output);

        assert!(matches!(outcome, SymlinkOutcome::SkippedEscape { .. }), "expected SkippedEscape, got {:?}", outcome);
        assert!(!output.join("escape").exists(), "no output entry should exist for escape symlink");
    }

    #[cfg(unix)]
    #[test]
    fn skips_broken_symlink() {
        let (_tmp, source, output) = make_fixture(&[
            ("broken", FileSpec::Symlink("does/not/exist")),
        ]);
        let canonical_root = source.canonicalize().unwrap();
        let entry = source.join("broken");

        let outcome = handle_symlink_entry(&entry, &source, &canonical_root, &output);

        assert!(matches!(outcome, SymlinkOutcome::SkippedBroken { .. }), "expected SkippedBroken, got {:?}", outcome);
        assert!(!output.join("broken").exists());
    }

    #[cfg(unix)]
    #[test]
    fn skips_file_symlink_to_processable_md() {
        let (_tmp, source, output) = make_fixture(&[
            ("posts/long.md", FileSpec::File("# long")),
            ("featured.md", FileSpec::Symlink("posts/long.md")),
        ]);
        let canonical_root = source.canonicalize().unwrap();
        let entry = source.join("featured.md");

        let outcome = handle_symlink_entry(&entry, &source, &canonical_root, &output);

        assert!(matches!(outcome, SymlinkOutcome::SkippedProcessable { ref ext, .. } if ext == "md"),
            "expected SkippedProcessable(md), got {:?}", outcome);
        assert!(!output.join("featured.md").exists());
    }

    #[cfg(unix)]
    #[test]
    fn preserves_file_symlink_to_asset() {
        let (_tmp, source, output) = make_fixture(&[
            ("posters/2024.png", FileSpec::File("PNG-bytes")),
            ("latest.png", FileSpec::Symlink("posters/2024.png")),
        ]);
        let canonical_root = source.canonicalize().unwrap();
        let entry = source.join("latest.png");

        let outcome = handle_symlink_entry(&entry, &source, &canonical_root, &output);

        assert!(matches!(outcome, SymlinkOutcome::Preserved { .. }), "got {:?}", outcome);
        let out_link = output.join("latest.png");
        assert!(fs::symlink_metadata(&out_link).unwrap().file_type().is_symlink());
        // Reading through the symlink should yield the canonical bytes.
        // (We can't actually read since posters/2024.png isn't in `output` —
        // only `latest.png` symlink is. But the symlink's target spelling
        // should round-trip.)
        let read_target = fs::read_link(&out_link).unwrap();
        assert_eq!(read_target, std::path::PathBuf::from("posters/2024.png"));
    }

    #[cfg(unix)]
    #[test]
    fn replaces_existing_directory_at_dest_path() {
        let (_tmp, source, output) = make_fixture(&[
            ("resources/app", FileSpec::Dir),
            ("resources/app/file.txt", FileSpec::File("hi")),
            ("alias", FileSpec::Symlink("resources/app")),
        ]);
        // Pre-populate output with a real directory at "alias"
        let stale_dir = output.join("alias");
        fs::create_dir_all(&stale_dir).unwrap();
        fs::write(stale_dir.join("stale.txt"), "stale").unwrap();

        let canonical_root = source.canonicalize().unwrap();
        let entry = source.join("alias");
        let outcome = handle_symlink_entry(&entry, &source, &canonical_root, &output);

        assert!(matches!(outcome, SymlinkOutcome::Preserved { .. }), "got {:?}", outcome);
        let meta = fs::symlink_metadata(&stale_dir).unwrap();
        assert!(meta.file_type().is_symlink(), "stale dir should have been replaced with a symlink");
    }

    #[cfg(unix)]
    #[test]
    fn replaces_existing_file_at_dest_path() {
        let (_tmp, source, output) = make_fixture(&[
            ("real.txt", FileSpec::File("real")),
            ("alias.txt", FileSpec::Symlink("real.txt")),
        ]);
        // Pre-populate output with a real file at "alias.txt"
        fs::write(output.join("alias.txt"), "stale").unwrap();

        let canonical_root = source.canonicalize().unwrap();
        let entry = source.join("alias.txt");
        let outcome = handle_symlink_entry(&entry, &source, &canonical_root, &output);

        assert!(matches!(outcome, SymlinkOutcome::Preserved { .. }), "got {:?}", outcome);
        let meta = fs::symlink_metadata(output.join("alias.txt")).unwrap();
        assert!(meta.file_type().is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn absolute_target_inside_root_is_rejected() {
        // Even if the absolute target resolves locally, the embedded
        // build-machine prefix won't exist on the deploy host. Warn-and-skip
        // at build time. See also `absolute_symlink_target_is_rejected`.
        let (_tmp, source, output) = make_fixture(&[
            ("resources/app", FileSpec::Dir),
            ("resources/app/index.html", FileSpec::File("hi")),
        ]);
        let canonical_root = source.canonicalize().unwrap();
        let abs_target = canonical_root.join("resources/app");
        std::os::unix::fs::symlink(&abs_target, source.join("alias")).unwrap();
        let entry = source.join("alias");

        let outcome = handle_symlink_entry(&entry, &source, &canonical_root, &output);

        assert!(
            matches!(outcome, SymlinkOutcome::SkippedAbsoluteTarget { .. }),
            "got {:?}",
            outcome
        );
        assert!(fs::symlink_metadata(output.join("alias")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn target_spelling_is_round_tripped() {
        // User wrote a relative target with `./` prefix — preserve it verbatim.
        let (_tmp, source, output) = make_fixture(&[
            ("resources/app", FileSpec::Dir),
            ("resources/app/x.html", FileSpec::File("hi")),
            ("alias", FileSpec::Symlink("./resources/app")),
        ]);
        let canonical_root = source.canonicalize().unwrap();
        let entry = source.join("alias");

        let outcome = handle_symlink_entry(&entry, &source, &canonical_root, &output);

        assert!(matches!(outcome, SymlinkOutcome::Preserved { .. }), "got {:?}", outcome);
        let read_target = fs::read_link(output.join("alias")).unwrap();
        assert_eq!(read_target, std::path::PathBuf::from("./resources/app"));
    }

    #[cfg(unix)]
    #[test]
    fn stale_cleanup_removes_symlink_not_in_live_set() {
        let (_tmp, _source, output) = make_fixture(&[]);
        // Create two symlinks in output: one live, one stale.
        fs::write(output.join("real.txt"), "x").unwrap();
        std::os::unix::fs::symlink("real.txt", output.join("live.link")).unwrap();
        std::os::unix::fs::symlink("real.txt", output.join("stale.link")).unwrap();

        let mut live = HashSet::new();
        live.insert("live.link".to_string());

        remove_stale_symlinks(&output, &live);

        assert!(output.join("live.link").exists(), "live symlink should remain");
        assert!(!output.join("stale.link").exists(), "stale symlink should be removed");
        assert!(output.join("real.txt").exists(), "regular file must NOT be touched");
    }

    #[cfg(unix)]
    #[test]
    fn stale_cleanup_does_not_descend_into_symlink() {
        // Critical regression guard: if we descend into the symlink, we'd
        // visit the canonical target's files and risk deleting them.
        let (_tmp, _source, output) = make_fixture(&[]);
        // Real directory with a real file inside.
        fs::create_dir_all(output.join("canonical")).unwrap();
        fs::write(output.join("canonical/keep.txt"), "keep").unwrap();
        // Symlink pointing at the canonical dir, registered as live.
        std::os::unix::fs::symlink("canonical", output.join("alias")).unwrap();

        let mut live = HashSet::new();
        live.insert("alias".to_string());

        remove_stale_symlinks(&output, &live);

        assert!(output.join("alias").exists(), "live symlink kept");
        assert!(output.join("canonical/keep.txt").exists(), "canonical file MUST survive");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn resolves_real_finder_alias() {
        // Use osascript to create a real Finder alias — most reliable way to
        // produce valid bookmark data without writing the format ourselves.
        let (_tmp, source, _output) = make_fixture(&[
            ("target", FileSpec::Dir),
            ("target/file.txt", FileSpec::File("hi")),
        ]);
        let target_path = source.join("target");
        let alias_path = source.join("target alias");

        let script = format!(
            r#"tell application "Finder" to make alias to (POSIX file "{}" as alias) at (POSIX file "{}" as alias)"#,
            target_path.display(),
            source.display()
        );
        let status = std::process::Command::new("osascript")
            .args(["-e", &script])
            .status();
        if status.map(|s| !s.success()).unwrap_or(true) {
            // osascript not available or denied — skip rather than fail
            eprintln!("skipping test: osascript unavailable");
            return;
        }

        // Finder will have named it "target alias" (with trailing " alias")
        assert!(is_bookmark_alias_file(&alias_path));
        let resolved = try_resolve_alias(&alias_path).expect("alias should resolve");
        // The resolved path should canonicalize to our target dir
        assert_eq!(
            resolved.canonicalize().unwrap(),
            target_path.canonicalize().unwrap()
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn returns_none_for_garbage_book_file() {
        let (_tmp, source, _output) = make_fixture(&[]);
        // Magic bytes match but payload is garbage
        let mut bytes = b"book\0\0\0\0".to_vec();
        bytes.extend_from_slice(&[0xFFu8; 100]);
        std::fs::write(source.join("garbage_alias"), &bytes).unwrap();
        assert!(is_bookmark_alias_file(&source.join("garbage_alias")));
        assert!(try_resolve_alias(&source.join("garbage_alias")).is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unresolvable_bookmark_does_not_fall_through_to_blob_copy() {
        // The 2026-06-03 `city-heat-map` deploy failure. A genuine Finder
        // bookmark file (magic bytes match) whose target cannot be resolved
        // (iCloud-dehydrated in production; a garbage payload here). Pre-fix,
        // `handle_alias_entry` returned `None` for this case, so the caller
        // fell through and blob-copied the ~1 KB alias FILE as a regular
        // MODE_FILE entry — publishing a garbage blob AND colliding with the
        // path's prior symlink on the server (EISDIR). It must instead be
        // skipped (`Some`, but NOT `Preserved`) so the caller never falls
        // through to a raw copy.
        let (_tmp, source, output) = make_fixture(&[]);
        let mut bytes = b"book\0\0\0\0".to_vec();
        bytes.extend_from_slice(&[0xFFu8; 200]);
        let alias = source.join("city-heat-map");
        std::fs::write(&alias, &bytes).unwrap();
        // Precondition: a genuine-but-unresolvable bookmark (NOT a happy path).
        assert!(is_bookmark_alias_file(&alias), "fixture must be a genuine bookmark file");
        assert!(try_resolve_alias(&alias).is_none(), "fixture target must be unresolvable");

        let canonical_root = source.canonicalize().unwrap();
        let outcome = handle_alias_entry(&alias, &source, &canonical_root, &output);

        assert!(
            outcome.is_some(),
            "an unresolvable bookmark must NOT return None (None makes the caller \
             fall through and blob-copy the alias); got {:?}",
            outcome
        );
        assert!(
            matches!(outcome, Some(SymlinkOutcome::SkippedUnresolvable { .. })),
            "an unresolvable bookmark must skip as SkippedUnresolvable; got {:?}",
            outcome
        );
        // No output entry of any kind should be written for it.
        assert!(
            fs::symlink_metadata(output.join("city-heat-map")).is_err(),
            "no output entry should exist for an unresolvable alias"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn non_bookmark_file_returns_none_for_fall_through() {
        // A plain file (not a bookmark) must return None so the caller copies
        // it normally as a regular asset. Guards the fix against over-reaching
        // and swallowing genuine non-alias files.
        let (_tmp, source, output) = make_fixture(&[
            ("notes.txt", FileSpec::File("plain text, not a bookmark")),
        ]);
        let canonical_root = source.canonicalize().unwrap();
        let outcome =
            handle_alias_entry(&source.join("notes.txt"), &source, &canonical_root, &output);
        assert!(
            outcome.is_none(),
            "a non-bookmark file must return None for caller fall-through; got {:?}",
            outcome
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn detects_bookmark_alias_by_magic() {
        let (_tmp, source, _output) = make_fixture(&[]);
        // Write a synthetic bookmark file (magic + minimal padding)
        let mut bytes = b"book\0\0\0\0mark\0\0\0\0".to_vec();
        bytes.resize(64, 0);
        std::fs::write(source.join("alias_file"), &bytes).unwrap();
        std::fs::write(source.join("plain.txt"), b"hello world").unwrap();

        assert!(is_bookmark_alias_file(&source.join("alias_file")));
        assert!(!is_bookmark_alias_file(&source.join("plain.txt")));
        assert!(!is_bookmark_alias_file(&source.join("does_not_exist")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn does_not_misdetect_short_files() {
        let (_tmp, source, _output) = make_fixture(&[]);
        std::fs::write(source.join("two.bin"), b"bo").unwrap();
        std::fs::write(source.join("three.bin"), b"boo").unwrap();
        std::fs::write(source.join("zero.bin"), b"").unwrap();
        assert!(!is_bookmark_alias_file(&source.join("two.bin")));
        assert!(!is_bookmark_alias_file(&source.join("three.bin")));
        assert!(!is_bookmark_alias_file(&source.join("zero.bin")));
    }

    #[cfg(unix)]
    #[test]
    fn stale_cleanup_skips_when_live_set_contains_all() {
        let (_tmp, _source, output) = make_fixture(&[]);
        std::os::unix::fs::symlink("nowhere", output.join("a.link")).unwrap();
        std::os::unix::fs::symlink("nowhere", output.join("b.link")).unwrap();

        let mut live = HashSet::new();
        live.insert("a.link".to_string());
        live.insert("b.link".to_string());

        remove_stale_symlinks(&output, &live);

        // Use symlink_metadata (not exists) because the targets are broken/non-existent;
        // exists() follows symlinks and returns false for broken ones.
        assert!(fs::symlink_metadata(output.join("a.link")).is_ok(), "a.link should still exist");
        assert!(fs::symlink_metadata(output.join("b.link")).is_ok(), "b.link should still exist");
    }

    // ---- Regression: case-sensitivity drift between symlink target and
    // slugified output tree. Source dirs like `Resources/` are emitted as
    // `resources/` by `slugify_dir_path`; if the symlink target keeps the
    // source casing, deploys to a case-sensitive filesystem 404.
    // (See `https://www.yinlab.io/city-heat-map` 2026-05-18.)

    #[cfg(unix)]
    #[test]
    fn relative_dir_symlink_target_lowercases_dir_segments() {
        let (_tmp, source, output) = make_fixture(&[
            ("Resources/App", FileSpec::Dir),
            ("Resources/App/index.html", FileSpec::File("<h1>app</h1>")),
            ("myapp", FileSpec::Symlink("Resources/App")),
        ]);
        let canonical_root = source.canonicalize().unwrap();
        let entry = source.join("myapp");

        let outcome = handle_symlink_entry(&entry, &source, &canonical_root, &output);

        let target = match outcome {
            SymlinkOutcome::Preserved { target, .. } => target,
            other => panic!("expected Preserved, got {:?}", other),
        };
        // Target points at a directory → every segment is a dir → all lowercased.
        assert_eq!(target, std::path::PathBuf::from("resources/app"));
        let on_disk = fs::read_link(output.join("myapp")).unwrap();
        assert_eq!(on_disk, std::path::PathBuf::from("resources/app"));
    }

    #[cfg(unix)]
    #[test]
    fn relative_file_symlink_target_lowercases_dirs_preserves_basename() {
        // Asset filenames must be preserved verbatim (third-party bundles load
        // by exact basename — `MathJax_Main-Bold.woff` mustn't become
        // `mathjax_main-bold.woff`). Only dir segments are lowercased.
        let (_tmp, source, output) = make_fixture(&[
            ("Posters/2024.PNG", FileSpec::File("PNG-bytes")),
            ("latest.png", FileSpec::Symlink("Posters/2024.PNG")),
        ]);
        let canonical_root = source.canonicalize().unwrap();
        let entry = source.join("latest.png");

        let outcome = handle_symlink_entry(&entry, &source, &canonical_root, &output);

        let target = match outcome {
            SymlinkOutcome::Preserved { target, .. } => target,
            other => panic!("expected Preserved, got {:?}", other),
        };
        assert_eq!(target, std::path::PathBuf::from("posters/2024.PNG"));
    }

    #[cfg(unix)]
    #[test]
    fn dot_prefixed_relative_symlink_target_is_lowercased() {
        let (_tmp, source, output) = make_fixture(&[
            ("Resources/App", FileSpec::Dir),
            ("Resources/App/x.html", FileSpec::File("hi")),
            ("alias", FileSpec::Symlink("./Resources/App")),
        ]);
        let canonical_root = source.canonicalize().unwrap();
        let entry = source.join("alias");

        let outcome = handle_symlink_entry(&entry, &source, &canonical_root, &output);

        let target = match outcome {
            SymlinkOutcome::Preserved { target, .. } => target,
            other => panic!("expected Preserved, got {:?}", other),
        };
        // `./` prefix preserved, segments lowercased.
        assert_eq!(target, std::path::PathBuf::from("./resources/app"));
    }

    #[cfg(unix)]
    #[test]
    fn absolute_symlink_target_is_rejected() {
        // Absolute targets embed the build-machine path which won't exist on
        // the deploy host — always 404s regardless of containment or case.
        // Warn-and-skip at build time rather than ship a broken artifact
        // (same policy as `SkippedEscape` / `SkippedProcessable`).
        let (_tmp, source, output) = make_fixture(&[
            ("Resources/App", FileSpec::Dir),
            ("Resources/App/index.html", FileSpec::File("hi")),
        ]);
        let canonical_root = source.canonicalize().unwrap();
        let abs_target = canonical_root.join("Resources/App");
        std::os::unix::fs::symlink(&abs_target, source.join("alias")).unwrap();
        let entry = source.join("alias");

        let outcome = handle_symlink_entry(&entry, &source, &canonical_root, &output);

        assert!(
            matches!(outcome, SymlinkOutcome::SkippedAbsoluteTarget { .. }),
            "expected SkippedAbsoluteTarget, got {:?}",
            outcome
        );
        // No output symlink should be written.
        assert!(
            fs::symlink_metadata(output.join("alias")).is_err(),
            "no output entry should exist for rejected absolute symlink"
        );
    }

    #[cfg(unix)]
    #[test]
    fn multi_segment_dir_symlink_target_lowercases_all_segments() {
        let (_tmp, source, output) = make_fixture(&[
            ("A/B/C", FileSpec::Dir),
            ("A/B/C/index.html", FileSpec::File("hi")),
            ("alias", FileSpec::Symlink("A/B/C")),
        ]);
        let canonical_root = source.canonicalize().unwrap();
        let entry = source.join("alias");

        let outcome = handle_symlink_entry(&entry, &source, &canonical_root, &output);

        let target = match outcome {
            SymlinkOutcome::Preserved { target, .. } => target,
            other => panic!("expected Preserved, got {:?}", other),
        };
        assert_eq!(target, std::path::PathBuf::from("a/b/c"));
    }

    #[cfg(unix)]
    #[test]
    fn parent_traversal_symlink_target_is_lowercased() {
        // `pathdiff::diff_paths` can synthesize `..`-prefixed relative paths
        // for an alias whose target is a sibling. The `..` segment passes
        // through; subsequent dir segments lowercase.
        let (_tmp, source, output) = make_fixture(&[
            ("Sister/App", FileSpec::Dir),
            ("Sister/App/index.html", FileSpec::File("hi")),
            ("here", FileSpec::Dir),
            ("here/alias", FileSpec::Symlink("../Sister/App")),
        ]);
        let canonical_root = source.canonicalize().unwrap();
        let entry = source.join("here/alias");

        let outcome = handle_symlink_entry(&entry, &source, &canonical_root, &output);

        let target = match outcome {
            SymlinkOutcome::Preserved { target, .. } => target,
            other => panic!("expected Preserved, got {:?}", other),
        };
        assert_eq!(target, std::path::PathBuf::from("../sister/app"));
    }

    #[cfg(unix)]
    #[test]
    fn unicode_dir_symlink_target_is_lowercased() {
        // Rust's `str::to_lowercase` is Unicode-aware. Lock in the contract
        // so a future "ASCII fast-path" refactor cannot silently regress
        // non-Latin dir names. Symmetry with `slugify_dir_path`'s
        // `文档/介绍.html` round-trip example.
        let (_tmp, source, output) = make_fixture(&[
            ("Документы/file.txt", FileSpec::File("hi")),
            ("alias.txt", FileSpec::Symlink("Документы/file.txt")),
        ]);
        let canonical_root = source.canonicalize().unwrap();
        let entry = source.join("alias.txt");

        let outcome = handle_symlink_entry(&entry, &source, &canonical_root, &output);

        let target = match outcome {
            SymlinkOutcome::Preserved { target, .. } => target,
            other => panic!("expected Preserved, got {:?}", other),
        };
        assert_eq!(target, std::path::PathBuf::from("документы/file.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn slugify_symlink_target_helper_keeps_absolute_input_verbatim() {
        // Defensive: the `is_absolute()` short-circuit in the helper is now
        // unreachable from the production call sites (both reject absolute
        // before slugifying), but the helper is a small unit worth pinning.
        let abs = Path::new("/Users/a/Resources/App");
        assert_eq!(slugify_symlink_target(abs, true), PathBuf::from(abs));
    }

    #[cfg(unix)]
    #[test]
    fn slugify_symlink_target_matches_slugify_dir_path_for_file_paths() {
        // Drift guard: a file-mode symlink target must match what
        // `slugify_dir_path` would emit for the same path. If the URL-slug
        // rule changes (e.g. JupyterLite `@jupyter-notebook` carve-out at
        // commit a21573395), this test fails until both sites are updated.
        use crate::build::scan::slug::slugify_dir_path;
        let cases = &[
            "Resources/MathJax_Main-Bold.woff",
            "@jupyter-notebook/scope.json",
            "Posts/Sub Section/2024-Photo (1).jpg",
            "文档/介绍.html",
        ];
        for input in cases {
            let via_slugify_dir_path = slugify_dir_path(input);
            let via_symlink_target = slugify_symlink_target(Path::new(input), false);
            assert_eq!(
                via_symlink_target.to_string_lossy(),
                via_slugify_dir_path,
                "drift detected for {:?}", input
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn finder_alias_target_lowercases_dir_segments() {
        // Yi-website regression: a Finder Alias at the project root pointing
        // at `Resources/cities-heat-map-app/` was deploying as a symlink whose
        // target preserved the source casing `Resources/...`. The deploy tree
        // serves `resources/...` (lowercased by `slugify_dir_path`), so the
        // symlink 404s on case-sensitive filesystems.
        let (_tmp, source, output) = make_fixture(&[
            ("Resources/CitiesHeatMapApp", FileSpec::Dir),
            ("Resources/CitiesHeatMapApp/index.html", FileSpec::File("<h1>app</h1>")),
        ]);
        let target_path = source.join("Resources/CitiesHeatMapApp");

        let script = format!(
            r#"tell application "Finder" to make alias to (POSIX file "{}" as alias) at (POSIX file "{}" as alias)"#,
            target_path.display(),
            source.display()
        );
        let ok = std::process::Command::new("osascript")
            .args(["-e", &script])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            eprintln!("skipping test: osascript unavailable");
            return;
        }

        // Find the alias Finder created (named "<target>" or "<target> alias").
        let alias_path = {
            let mut found = None;
            for entry in fs::read_dir(&source).unwrap().flatten() {
                let p = entry.path();
                if is_bookmark_alias_file(&p) {
                    found = Some(p);
                    break;
                }
            }
            match found {
                Some(p) => p,
                None => {
                    eprintln!("skipping test: alias not created");
                    return;
                }
            }
        };

        let canonical_root = source.canonicalize().unwrap();
        let outcome = handle_alias_entry(&alias_path, &source, &canonical_root, &output)
            .expect("alias detection should fire");

        let target = match outcome {
            SymlinkOutcome::Preserved { target, .. } => target,
            other => panic!("expected Preserved, got {:?}", other),
        };
        // Every segment in the target is a dir → all lowercased.
        assert_eq!(target, std::path::PathBuf::from("resources/citiesheatmapapp"));
    }
}
