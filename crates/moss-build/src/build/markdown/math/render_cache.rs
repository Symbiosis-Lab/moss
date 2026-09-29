//! The math render cache (both levels) and the dedicated render pool.
//!
//! Split out of `math.rs` under the sibling-file pattern (matching this
//! module's own `font.rs`) purely to keep that file under the workspace's
//! size gate — everything here is still conceptually part of `typeset`, and
//! `math.rs`'s "Render caching" module doc section is where the design is
//! explained. This file is the implementation.

use super::{MathRefusal, RawRender, TypesetMath};

/// Schema version for the math render cache (both levels). Bump this,
/// independent of RaTeX's own version, whenever this cache's key or stored
/// shape changes; folded into the on-disk transform's `params` so an old
/// entry is a clean miss instead of a misread. RaTeX's own version already
/// invalidates separately: it is part of
/// [`crate::build::emit::math_png::content_hash`], which both cache levels
/// key on.
const RENDER_CACHE_VERSION: u32 = 1;

/// Transform name under which the disk level stores its entries in
/// [`crate::build::cache::TransformCache`] — the `format-probe` pattern
/// (`build/media/image.rs`): the "source" is a synthetic OID (this
/// equation's own content address, not a real file), and the "output blob"
/// is a small JSON encoding of [`TypesetMath`] rather than a converted asset.
///
/// **Disk-level GC**: `crate::build::cache::gc` (mark-and-sweep, auto-triggered
/// by `store_gc::maybe_gc_cache` as the store grows) DOES eventually reclaim
/// these records, but only incidentally, not because it understands "math
/// entry, superseded." Its mark phase treats a transform record as live when
/// its `source_oid` is a content hash `HashIndex` currently has for a real
/// vault file; our `source_oid` is `content_hash(tex, display)` — never a
/// real file's hash — so it can NEVER be marked live and is condemned purely
/// once its record file is older than `cache::RECORD_TTL` (90 days),
/// regardless of whether the equation is still on the site. That bounds disk
/// growth (nothing here leaks forever), but it does NOT track supersession
/// the way the in-memory level's `parse_cache::math_cache_finish_build` now
/// does: an actively-used equation's record can still be swept if 90 days
/// pass without a fresh WRITE — `disk_cache_lookup` never touches the file's
/// mtime, only `disk_cache_store` (a miss) does — costing one avoidable
/// re-render next time, never a wrong one. Not fixed here: a corpus-aware
/// disk sweep would duplicate `math_cache_finish_build`'s logic against a
/// different store, and nothing has measured this 90-day churn as a real
/// cost yet.
const RENDER_CACHE_TRANSFORM: &str = "math-render";

/// The process-wide "current site" disk cache the render cache's on-disk
/// level reads and writes, set once per build by [`set_disk_cache`]. `None`
/// — the state in every test and in any fragment-render path that never
/// calls the setter — just skips the disk level: `typeset` falls back to
/// computing fresh, exactly as it did before this cache existed.
///
/// A `Mutex<Option<TransformCache>>` rather than a `OnceLock`: unlike
/// [`super::font::pinned`], which accepts "one font for the process" because
/// a font choice is a machine fact, the cache directory is a BUILD fact — a
/// long-lived watch process can move roots — so this must accept being
/// overwritten. Readers lock only long enough to `clone()` the two `PathBuf`s
/// out (see `ObjectStore`/`TransformCache`'s `Clone` derives); the actual
/// disk I/O then runs unsynchronized, so concurrent Loop-render workers never
/// serialize on this lock the way they would if it held the I/O itself.
static DISK_CACHE: std::sync::Mutex<Option<crate::build::cache::TransformCache>> =
    std::sync::Mutex::new(None);

/// Point the render cache's disk level at `cache`. Call once per build, as
/// early as the site's `TransformCache` is available (production: alongside
/// `ParseSession::begin` in `render/blocking.rs`, which runs once per build
/// before any page's math is rendered). Idempotent and safe to call again on
/// the next build in a long-lived watch process — see [`DISK_CACHE`].
pub fn set_disk_cache(cache: crate::build::cache::TransformCache) {
    if let Ok(mut slot) = DISK_CACHE.lock() {
        *slot = Some(cache);
    }
}

/// On-disk render-cache read: the `format-probe` pattern applied to a
/// synthetic source (see [`RENDER_CACHE_TRANSFORM`]'s doc). `None` whenever
/// [`DISK_CACHE`] hasn't been set (tests, fragment-render paths) or on any
/// miss — the caller falls through to a real render either way.
pub(super) fn disk_cache_lookup(key: &str) -> Option<TypesetMath> {
    let cache = DISK_CACHE.lock().ok()?.clone()?;
    let params = serde_json::json!({ "v": RENDER_CACHE_VERSION });
    let oid = cache.find_cached_output(key, RENDER_CACHE_TRANSFORM, &params)?;
    let blob_path = cache.objects().get_path(&oid)?;
    let bytes = std::fs::read(blob_path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// On-disk render-cache write. Best-effort, mirroring `should_skip`'s
/// write-back: a failed store just means the next build recomputes this
/// equation too, never a wrong result now.
pub(super) fn disk_cache_store(key: &str, value: &TypesetMath) {
    let Some(cache) = DISK_CACHE.lock().ok().and_then(|c| c.clone()) else {
        return;
    };
    let Ok(bytes) = serde_json::to_vec(value) else {
        return;
    };
    let Ok(blob_oid) = cache.objects().store_bytes(&bytes) else {
        return;
    };
    let params = serde_json::json!({ "v": RENDER_CACHE_VERSION });
    let mut record = cache
        .get(key)
        .unwrap_or_else(|| crate::build::cache::TransformRecord {
            source_oid: key.to_string(),
            source_size: bytes.len() as u64,
            transforms: std::collections::HashMap::new(),
        });
    record.transforms.insert(
        RENDER_CACHE_TRANSFORM.to_string(),
        crate::build::cache::TransformEntry {
            oid: blob_oid,
            size: bytes.len() as u64,
            params,
        },
    );
    if let Err(e) = cache.put(&record) {
        log::warn!("[math-render] failed to write render-cache record: {e}");
    }
}

/// Count of equations actually sent through the render pool (a cache miss at
/// both levels) — never incremented on a hit. Test-visible so the render
/// cache's whole point ("an unchanged equation costs zero typesets") is
/// something a test can assert on directly instead of inferring from timing.
///
/// Keyed by the SAME cache key as the render cache itself (not a single
/// scalar): `cargo test` runs this crate's tests concurrently in one process,
/// so a bare counter would mix in every other test's unrelated equations. A
/// per-equation count is immune to that — a test using its own unique tex
/// string reads a count no other test ever touches.
static TYPESET_COUNTS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, u64>>> =
    std::sync::OnceLock::new();

fn typeset_counts() -> &'static std::sync::Mutex<std::collections::HashMap<String, u64>> {
    TYPESET_COUNTS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Record a real typeset (a cache miss at both levels) for `key` — called
/// once from `typeset`, right before the render pool is asked to do the work.
pub(super) fn record_typeset(key: &str) {
    if let Ok(mut counts) = typeset_counts().lock() {
        *counts.entry(key.to_string()).or_insert(0) += 1;
    }
}

/// How many times `(tex, display)` has actually been sent through the render
/// pool (a cache miss at both levels) in this process, ever. See
/// [`TYPESET_COUNTS`] for why this is per-equation rather than one scalar.
pub fn typeset_count(tex: &str, display: bool) -> u64 {
    let key = crate::build::emit::math_png::content_hash(tex, display);
    typeset_counts()
        .lock()
        .ok()
        .and_then(|c| c.get(&key).copied())
        .unwrap_or(0)
}

/// Stack size for every render-pool worker (layer C). RaTeX renders the
/// spike's deepest admissible input in well under this; a 2 MiB (Rust
/// default spawned-thread) stack aborts on inputs an 8 MiB stack renders
/// fine (#144), so we give it 16 MiB and let the length + nesting caps bound
/// the rest. `deep_nesting_renders_on_the_pool_without_overflow` proves a
/// pool worker, not just a freshly spawned thread, still has this margin.
const RENDER_STACK_BYTES: usize = 16 * 1024 * 1024;

/// The dedicated large-stack render pool (layer C). Reused across renders
/// instead of spawning a fresh 16 MiB-stack thread per equation — the cost
/// is real: a 100-equation page paid 100 thread creations for one edited
/// equation before this pool existed. Sized to
/// [`std::thread::available_parallelism`] (falling back to 1) rather than a
/// single worker: moss's outer build parallelism already renders many pages'
/// math concurrently, and a lone worker would serialize a math-heavy cold
/// build behind one equation at a time, trading a thread-spawn cost for a
/// worse one.
///
/// A page-render call already running on the OUTER (default) rayon pool
/// dispatches into this SEPARATE pool via `install` below, so a fully busy
/// build can have both pools' workers active at once — up to ~2×
/// `available_parallelism` OS threads, i.e. deliberate oversubscription, not
/// a bug. It is not a deadlock: `install` on a foreign pool blocks the
/// calling (outer-pool) thread while the work runs on `RENDER_POOL`'s OWN
/// worker threads, which never need to borrow a slot back from the outer
/// pool to make progress — the two pools have no shared work queue for a
/// blocked thread to be waiting on.
static RENDER_POOL: std::sync::LazyLock<rayon::ThreadPool> = std::sync::LazyLock::new(|| {
    rayon::ThreadPoolBuilder::new()
        .thread_name(|i| format!("moss-math-render-{i}"))
        .stack_size(RENDER_STACK_BYTES)
        .num_threads(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
        )
        .build()
        .expect("moss-math-render pool")
});

/// Layer C — run RaTeX on [`RENDER_POOL`], a dedicated pool whose workers
/// carry an explicit 16 MiB stack, and catch a panic inside it. RaTeX has no
/// rayon dependency, so there is no worker-pool nesting corruption.
///
/// `catch_unwind` replaces the old spawn-per-call `thread::Builder::join`'s
/// implicit panic capture — same net, applied explicitly here since
/// `ThreadPool::install` propagates a panic instead of returning it. Either
/// way, this net only exists where a panic *unwinds*: moss release builds set
/// `panic = "abort"`, under which a panic — like a stack overflow — takes the
/// whole process down before any `catch_unwind` runs. `catch_unwind` and this
/// function's `Ok(Err(RenderFailed))` fallback are therefore load-bearing
/// only in debug and CI (`panic = "unwind"` there); in release, everything
/// upstream of here (layer A's guard, in particular) is what actually keeps a
/// panicking input from ever reaching the pool.
pub(super) fn render_on_big_stack(tex: &str) -> Result<RawRender, MathRefusal> {
    let owned = tex.to_string();
    RENDER_POOL.install(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| super::render_inner(&owned)))
            .unwrap_or(Err(MathRefusal::RenderFailed))
    })
}

#[cfg(test)]
pub(super) fn disk_cache_lock_for_tests() -> DiskCacheTestGuard {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    DiskCacheTestGuard {
        _lock: LOCK.lock().unwrap_or_else(|e| e.into_inner()),
    }
}

/// Serializes tests that arm [`DISK_CACHE`] against each other, and clears it
/// again on drop — the `parse_cache::store_lock_for_tests` pattern, applied
/// to this module's own process-global. Any test NOT holding this guard runs
/// with `DISK_CACHE` at its default `None` unless a guarded test happens to
/// be concurrently mid-flight; every disk-cache test below therefore uses a
/// tex string unique to itself so an unrelated concurrent render can never
/// collide with the key it is asserting on.
#[cfg(test)]
pub(super) struct DiskCacheTestGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl Drop for DiskCacheTestGuard {
    fn drop(&mut self) {
        if let Ok(mut slot) = DISK_CACHE.lock() {
            *slot = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `typeset_count(tex, display)` is a per-equation count (see
    // `TYPESET_COUNTS`'s doc for why), so every tex string below is unique to
    // its test — an unrelated concurrent test's equation can never share this
    // one's counter entry, and cargo's default parallel test execution needs
    // no `--test-threads=1` to make these assertions exact rather than "at
    // least".

    #[test]
    fn repeat_render_of_unchanged_equation_skips_the_typeset_but_a_changed_one_does_not() {
        let a = r"\alpha_{cache_probe_a1f0}";
        let b = r"\beta_{cache_probe_a1f0}";

        let fresh = super::super::render_math(a, false).expect("should typeset");
        assert_eq!(typeset_count(a, false), 1, "first render of a new equation must typeset");

        let cached = super::super::render_math(a, false).expect("should typeset");
        assert_eq!(
            typeset_count(a, false),
            1,
            "re-rendering the SAME equation must not typeset again"
        );
        // The counter alone only proves RaTeX was skipped, not that what came
        // back instead is right — a cache hit that silently returned garbage
        // would still pass the count-based assertion above.
        assert_eq!(
            cached, fresh,
            "a cache hit must reproduce the fresh render byte for byte, not just skip the typeset"
        );

        super::super::render_math(b, false).expect("should typeset");
        assert_eq!(
            typeset_count(b, false),
            1,
            "a DIFFERENT equation must still typeset exactly once"
        );
    }

    #[test]
    fn disk_cache_round_trips_a_stored_typeset() {
        // Exercises `disk_cache_lookup`/`disk_cache_store` directly rather
        // than through `typeset`, deliberately: `typeset` would also touch
        // the in-memory level (`parse_cache::math_cache_*`), which is a
        // process-global shared with every other concurrently running test —
        // clearing it here to force a disk-only path would risk evicting
        // ANOTHER test's live entry mid-run. This proves the disk level's own
        // mechanics in isolation, with no such shared side effect.
        let _guard = disk_cache_lock_for_tests();
        let dir = tempfile::tempdir().unwrap();
        set_disk_cache(crate::build::cache::TransformCache::new(
            dir.path().join("transforms"),
            crate::build::cache::ObjectStore::new(dir.path().join("objects")),
        ));

        let key = "disk-cache-roundtrip-probe-9f3c";
        assert!(disk_cache_lookup(key).is_none(), "must miss before anything is stored");

        let value = TypesetMath {
            svg: r#"<svg viewBox="0 0 10 10"><path d="M0 0"/></svg>"#.to_string(),
            width_em: 1.25,
            height_em: 0.75,
            depth_em: 0.1,
        };
        disk_cache_store(key, &value);

        let round_tripped = disk_cache_lookup(key).expect("must hit after being stored");
        assert_eq!(round_tripped.svg, value.svg);
        assert_eq!(round_tripped.width_em, value.width_em);
        assert_eq!(round_tripped.height_em, value.height_em);
        assert_eq!(round_tripped.depth_em, value.depth_em);
    }

    #[test]
    fn deep_nesting_renders_on_the_pool_without_overflow() {
        // At the cap, not over it — `guard` must let this through so the
        // render pool, not layer A, is what this test exercises.
        let n = super::super::MAX_NESTING;
        let deep = format!("{}a{}", r"\frac{".repeat(n), "}{b}".repeat(n));

        // More concurrent renders than the pool has worker threads (unless
        // this machine has 8+ cores, in which case they simply all run at
        // once) — either way, every one must get a full 16 MiB stack, not
        // whatever a shared or undersized stack would give it.
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let tex = deep.clone();
                std::thread::spawn(move || render_on_big_stack(&tex))
            })
            .collect();
        for h in handles {
            let raw = h
                .join()
                .expect("a render-pool call must not panic or abort the test process")
                .expect("nesting within the cap must still render, not refuse");
            assert!(raw.svg.contains("<path"), "must carry real glyph outlines");
        }
    }
}
