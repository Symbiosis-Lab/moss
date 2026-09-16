//! Retention and garbage collection for the local build store (`.moss/build/`).
//!
//! **Pure I/O, no `tauri`, no app-side singletons** — every entry point takes
//! plain paths and plain data, so this module moves into `crates/moss-build`
//! (which owns cache/CAS and lifecycle by charter, and is CI-asserted
//! tauri-free) unchanged at M6a. Same discipline as `io_utils`, and the reason
//! the config knob is read by `build::site_config` and passed *in* rather than
//! looked up here: `config.toml` has one reader by charter.
//!
//! Two collectors live here, with different shapes and different payoffs:
//!
//! - **Generations** — [`gc_old_generations`] keeps the newest `n` generation
//!   directories plus an explicit root set. On a copy-on-write filesystem the
//!   retained ones are nearly free (`ship` materializes via `fs::copy`, which
//!   reflinks); on ext4 they cost their full byte size, which is why
//!   [`effective_keep_generations`] clamps `n` when the probe says there is no
//!   reflink. See [`supports_cow`].
//!
//! - **The content-addressed cache** — `cache::gc` is a correct mark-and-sweep
//!   that, until moss#976, had no automatic caller at all: the only entry point
//!   was the `run_cache_gc` Tauri command, which nothing in the frontend ever
//!   invoked. Measured on a real site: 10,143 objects on disk against 923
//!   referenced — ~0.5 GB of genuinely unreachable blobs, growing forever.
//!   [`maybe_gc_cache`] supplies the missing trigger, modelled on git's
//!   `gc.auto`.
//!
//! ## Why the cache trigger needs a watermark
//!
//! A bare "more than N objects" threshold either never fires on a small site or
//! fires on *every* build of a large one, because the reachable set of a big
//! site legitimately exceeds any fixed N. git solves this by recording how many
//! loose objects survived the last collection and re-collecting only when the
//! store has grown substantially past that. [`GcWatermark`] is that record, so
//! the store settles: after a sweep the watermark IS the reachable set, and the
//! next sweep waits until the store has roughly doubled.
//!
//! ## Invariants
//!
//! - **GC must never run concurrently with a build.** `cache::gc` assumes a
//!   quiescent cache; sweeping while a build is about to reference a blob would
//!   delete it. Callers hold the per-folder `stage_write_lock`.
//! - **The retention floor is 2, not 1 or 0.** `build/pipeline.rs` serves the
//!   previous generation for zero-flicker preview while `staging/` is rewritten,
//!   and `current` must never dangle during materialize.
//! - **Roots are enumerated, never inferred.** Every generation some other
//!   subsystem may open must appear in the set passed to [`gc_old_generations`].
//!   The non-obvious one is `last_deployed_generation_id` — see
//!   [`gc_roots`].

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::cache;

/// Generation directories retained by default.
///
/// **2 is the floor, not the default.** One generation is impossible: the
/// preview serves the previous generation while `staging/` is rewritten, so
/// live-plus-one is structurally required. The third buys the case where you
/// roll back and discover the previous generation was also bad — cheap on a COW
/// filesystem, and the only reason to keep more than the floor.
///
/// It is deliberately NOT larger. moss is a client that owns the source and has
/// git; deep history of a *derived* tree is not moss's job, and only the newest
/// generation is ever read (the full reader inventory in moss#976 resolves
/// `current_ptr()`, with one exception — see [`gc_roots`]). The prior value was
/// 5, a hardcoded literal with no knob, matching Capistrano's `keep_releases`
/// default — but Capistrano retains on the *server*, where the source is absent
/// and rollback is the only recovery. Borrow the knob, reject the default.
pub const KEEP_GENERATIONS_DEFAULT: usize = 3;

/// Hard floor for generation retention — see [`KEEP_GENERATIONS_DEFAULT`].
///
/// A user who writes `keep_generations = 0` still gets 2. Below this, `current`
/// dangles mid-materialize and the preview flickers.
pub const KEEP_GENERATIONS_FLOOR: usize = 2;

/// Object count below which the cache is never swept automatically.
///
/// A first sweep on a store this small would free a trivial amount and cost a
/// full directory walk. Roughly git's `gc.auto` order of magnitude.
const CACHE_GC_MIN_OBJECTS: usize = 2_048;

/// Growth headroom over the post-sweep watermark before sweeping again:
/// `trigger = watermark * FACTOR + SLACK`. The additive slack keeps a site whose
/// reachable set is tiny from re-sweeping on every build.
const CACHE_GC_GROWTH_FACTOR: usize = 2;
const CACHE_GC_GROWTH_SLACK: usize = 512;

// ---------------------------------------------------------------------------
// Generation retention
// ---------------------------------------------------------------------------

/// Resolve how many generations to keep, honouring the config knob, the floor,
/// and the copy-on-write probe.
///
/// `configured` is `[build].keep_generations` as read by
/// `build::site_config::get_build_keep_generations`, or `None` when unset.
///
/// The COW clamp is the whole point of the probe: retaining generations is
/// cheap only when `fs::copy` reflinks. On ext4 — no reflink, silent full byte
/// copy — a 340 MB site pays ~1 GB for three generations where an APFS user
/// pays ~340 MB plus deltas, and `du` reports the same number on both, so the
/// difference is indistinguishable from normal operation until the disk fills.
pub fn effective_keep_generations(configured: Option<usize>, build_dir: &Path) -> usize {
    let n = configured.unwrap_or(KEEP_GENERATIONS_DEFAULT).max(KEEP_GENERATIONS_FLOOR);
    if n > KEEP_GENERATIONS_FLOOR && !supports_cow(build_dir) {
        log::info!(
            "generation retention: {} → {} — {} has no copy-on-write support, \
             so each retained generation costs its full size on disk",
            n,
            KEEP_GENERATIONS_FLOOR,
            build_dir.display()
        );
        return KEEP_GENERATIONS_FLOOR;
    }
    n
}

/// Assemble the GC root set.
///
/// Enumerated, never inferred. Three roots today:
///
/// 1. `current_gen_id` — the generation just materialized and pointed at.
/// 2. `pinned` — any generation an in-flight deploy is reading, so a GC storm
///    from several fast rebuilds cannot `remove_dir_all` the directory an upload
///    is streaming from.
/// 3. `last_deployed` — `last_deployed_generation_id` from `.moss/state.toml`.
///    This is the non-obvious one, and the reason moss#976 files it as a
///    correctness fix rather than a cleanup: `email/commands.rs`'s
///    publish-before-send gate resolves that id and checks the math PNGs exist
///    inside it. Once that generation aged past the retention window it was
///    deleted and the gate silently failed closed with "Publish the site first"
///    — for a site that *had* been published. It is the only reader in the
///    codebase that opens a non-current generation, and it was unprotected.
///    It also gates lowering retention at all: dropping 5 → 3 without this makes
///    the failure strictly more likely.
pub fn gc_roots(
    current_gen_id: &str,
    pinned: &HashSet<String>,
    last_deployed: Option<&str>,
) -> HashSet<String> {
    let mut roots = HashSet::new();
    roots.insert(current_gen_id.to_string());
    roots.extend(pinned.iter().cloned());
    if let Some(d) = last_deployed {
        roots.insert(d.to_string());
    }
    roots
}

/// List the generation-directory names present under `generations_dir`.
pub fn list_generations(generations_dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(generations_dir) else {
        return Vec::new();
    };
    rd.filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
        .collect()
}

/// Remove old generation directories, keeping the `n` most-recently-modified
/// plus everything in `roots`.
///
/// Errors are logged but non-fatal: the caller logs them as warnings so a GC
/// failure never invalidates a build that already succeeded.
///
/// *Known imprecision:* generation directories are content-addressed, so a
/// rebuild that re-derives an old id refreshes that directory's mtime. "The `n`
/// most recent" therefore means recency-of-materialisation, not build order.
/// Harmless — a re-derived generation is byte-identical to the one it refreshes.
pub fn gc_old_generations(
    generations_dir: &Path,
    roots: &HashSet<String>,
    n: usize,
) -> std::io::Result<()> {
    let mut entries: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(generations_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let mtime = e.metadata().ok()?.modified().ok()?;
            Some((mtime, e.path()))
        })
        .collect();

    // Sort newest first.
    entries.sort_by(|a, b| b.0.cmp(&a.0));

    for (_, path) in entries.iter().skip(n) {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if roots.contains(name) {
            continue; // pinned: current, in-flight deploy, or last-deployed
        }
        if let Err(e) = std::fs::remove_dir_all(path) {
            log::warn!("generation GC: failed to remove {:?}: {}", path, e);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Cache GC trigger (git's `gc.auto`)
// ---------------------------------------------------------------------------

/// How many objects survived the last automatic sweep.
///
/// Persisted next to the cache so the trigger is stable across app restarts. A
/// missing or corrupt file reads as "never swept", which is the safe direction —
/// the worst case is one extra sweep.
#[derive(serde::Serialize, serde::Deserialize, Debug, Default)]
struct GcWatermark {
    objects_after_gc: usize,
}

fn watermark_path(build_dir: &Path) -> PathBuf {
    build_dir.join("cache").join("gc-watermark.json")
}

fn load_watermark(build_dir: &Path) -> Option<usize> {
    let raw = std::fs::read_to_string(watermark_path(build_dir)).ok()?;
    serde_json::from_str::<GcWatermark>(&raw)
        .ok()
        .map(|w| w.objects_after_gc)
}

fn save_watermark(build_dir: &Path, objects_after_gc: usize) {
    let Ok(json) = serde_json::to_string(&GcWatermark { objects_after_gc }) else {
        return;
    };
    // `.moss/build/**` is regenerable output, so this goes through io_utils:
    // a cloud-evicted destination is *absent*, not something to materialize
    // (ADR-043). A raw `fs::write` here would `EDEADLK` the build.
    if let Err(e) = super::io_utils::write_output(&watermark_path(build_dir), json.as_bytes()) {
        log::warn!("cache GC: failed to record watermark: {}", e);
    }
}

/// Count blobs in `cache/objects`.
///
/// The store is one fanout level deep (`objects/ab/cdef…`), so this walks the
/// fanout directories rather than recursing arbitrarily.
fn count_objects(build_dir: &Path) -> usize {
    let Ok(fanout) = std::fs::read_dir(build_dir.join("cache").join("objects")) else {
        return 0;
    };
    fanout
        .filter_map(|e| e.ok())
        .map(|e| {
            let p = e.path();
            if p.is_dir() {
                std::fs::read_dir(&p).map(|d| d.count()).unwrap_or(0)
            } else {
                1
            }
        })
        .sum()
}

/// Decide whether the store has grown enough to be worth sweeping.
///
/// Split out as a pure function so the policy is testable without a filesystem —
/// the settling behaviour described in the module header is the thing worth
/// pinning, and it is entirely arithmetic.
fn should_gc_cache(objects_on_disk: usize, watermark: Option<usize>) -> bool {
    if objects_on_disk < CACHE_GC_MIN_OBJECTS {
        return false;
    }
    match watermark {
        None => true,
        Some(w) => objects_on_disk > w * CACHE_GC_GROWTH_FACTOR + CACHE_GC_GROWTH_SLACK,
    }
}

/// Run `cache::gc` if the object store has grown past its watermark.
///
/// Returns the result when a sweep ran, `None` when the threshold was not met.
///
/// **The caller must hold the per-folder `stage_write_lock`** (or be on a path
/// where no concurrent build is possible). `cache::gc` assumes a quiescent
/// cache; a concurrent build could be about to reference a blob this sweep is
/// deleting.
///
/// Blocking — walks and unlinks. Async callers wrap it in `spawn_blocking`.
pub fn maybe_gc_cache(build_dir: &Path) -> Option<cache::GcResult> {
    let before = count_objects(build_dir);
    if !should_gc_cache(before, load_watermark(build_dir)) {
        return None;
    }

    let result = cache::gc(build_dir);
    let after = before.saturating_sub(result.objects_removed);
    save_watermark(build_dir, after);
    log::info!(
        "cache GC: {} → {} objects ({} removed, {} transforms, {} bytes freed)",
        before,
        after,
        result.objects_removed,
        result.transforms_removed,
        result.bytes_freed
    );
    Some(result)
}

// ---------------------------------------------------------------------------
// Copy-on-write probe
// ---------------------------------------------------------------------------

/// Does `dir`'s filesystem support reflink copies?
///
/// `ship::materialize_and_promote` copies a whole generation with `fs::copy`,
/// which the kernel turns into a reflink on APFS and on Btrfs/XFS — an
/// independent inode at near-zero disk cost. **ext4 has no reflink and silently
/// does a full byte copy.** Nothing in moss detected, asserted, logged, or fell
/// back, and `du` reports the same number either way, so the difference was
/// invisible until the disk filled.
///
/// Hardlinking is not the fallback — ADR-013 bans it in the output tree because
/// iCloud "optimize storage" zeroes by inode, turning every hardlinked copy into
/// a 0-byte stub at once. The only lever is retaining fewer generations, which is
/// what [`effective_keep_generations`] does with this answer.
///
/// Probes by attempting the clone rather than matching a filesystem name, so a
/// filesystem moss has never heard of still gets the right answer. A probe that
/// cannot run at all reports "supported": that preserves the behaviour moss
/// shipped with, and a false positive costs disk rather than correctness.
pub fn supports_cow(dir: &Path) -> bool {
    probe_cow(dir).unwrap_or(true)
}

#[cfg(target_os = "linux")]
fn probe_cow(dir: &Path) -> Option<bool> {
    use std::os::unix::io::AsRawFd;

    // FICLONE — _IOW(0x94, 9, int). Clones src's extents into dst; fails with
    // EOPNOTSUPP (EINVAL on some stacks) when the filesystem has no reflink.
    const FICLONE: libc::c_ulong = 0x4009_4409;

    std::fs::create_dir_all(dir).ok()?;
    let src = dir.join(".moss-cow-probe-src");
    let dst = dir.join(".moss-cow-probe-dst");
    // allow:raw_write probe scratch, not an output artifact — deleted below
    std::fs::write(&src, b"moss copy-on-write probe").ok()?;
    let src_f = std::fs::File::open(&src).ok()?;
    // allow:raw_write probe scratch, not an output artifact — deleted below
    let dst_f = std::fs::File::create(&dst).ok()?;
    let rc = unsafe { libc::ioctl(dst_f.as_raw_fd(), FICLONE, src_f.as_raw_fd()) };
    drop(src_f);
    drop(dst_f);
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&dst);
    Some(rc == 0)
}

#[cfg(target_os = "macos")]
fn probe_cow(dir: &Path) -> Option<bool> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    std::fs::create_dir_all(dir).ok()?;
    let c = CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let name: Vec<u8> = st
        .f_fstypename
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| b as u8)
        .collect();
    // APFS is the only Apple filesystem `fclonefileat` works on; an HFS+
    // external drive pays the full byte copy exactly like ext4.
    Some(name.eq_ignore_ascii_case(b"apfs"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn probe_cow(_dir: &Path) -> Option<bool> {
    // Windows: `std::fs::copy` uses `CopyFileEx`, which does not block-clone
    // even on ReFS. Anything else: unknown, and an unknown filesystem is far
    // more likely to lack reflink than to have it.
    Some(false)
}

#[cfg(test)]
#[path = "store_gc_tests.rs"]
mod tests;
