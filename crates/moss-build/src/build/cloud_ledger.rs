//! The one place a build writes down "these bytes were not here."
//!
//! An evicted file raises one question — *the bytes are not here; now what?* —
//! and moss answers it at seven sites in five ways. Before this module none of
//! those answers were recorded anywhere the rest of the build could see, with
//! two consequences measured on a live Google Drive vault:
//!
//! 1. **The gate could not see the images.** `pipeline.rs`'s cloud gate read
//!    `ProjectStructure::evicted_count`, a count taken by the *source scan* —
//!    before the build, pruning dot-directories — so it is stale by construction
//!    for anything the provider evicted afterwards. 578 evicted images could not
//!    influence it, and the user was shown a finished site.
//! 2. **The build rediscovered every absence, once per file per build.** The
//!    supervisor knows exactly which files are in flight, but its pending set is
//!    a `Vec<Pending>` local to `async fn run` with no handle and no state slot
//!    (now `build_shell::watch::sweep`), so the image pipeline could only find out by
//!    trying the read and taking the `EDEADLK`. 6,967 warnings in one log.
//!
//! Both are the same missing structure: [`note_unavailable`] writes down what a
//! build had to do without, and [`outstanding`] is what the gate reads.
//!
//! ## Why there is no shared "in flight" set
//!
//! An earlier shape of this module also republished the supervisor's pending set
//! so the build could ask "is it worth even trying this read?". It was dropped
//! before it shipped: `icloud::is_evicted` answers the same question with one
//! `lstat` that never materializes anything, and it answers it about *now*
//! rather than about the supervisor's last tick. A cache that is slower to be
//! right than the thing it caches is not worth the coherence problem, and moss
//! does not carry modules without consumers.
//!
//! ## Why it is keyed by path and not by folder
//!
//! The call sites that discover an absence are deep in the media pipeline —
//! `fallback_raster`, `rungs`, `collect_images_for_conversion` — and most of
//! them hold a source path and nothing else. Threading a folder root down to
//! each of them would be a wide, mechanical diff whose only purpose is to
//! re-derive something the path already contains. So the store is keyed by
//! absolute path and [`outstanding`] answers a folder question with a prefix
//! scan. At the scale this runs (hundreds to low thousands of paths, consulted
//! once per build) that is far below measurable.
//!
//! ## Why recording is not the caller's job to *remember*
//!
//! Each call site keeps its own rendering decision — `fallback_raster` may still
//! ship the verbatim original, which is the right call for a CMYK JPEG and the
//! right call here too. What it may not do is stay silent. The callers with the
//! most reason to fail open are the ones with the least reason to remember they
//! did, so this module is deliberately a single free function they can call
//! without holding any context.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Sources *this build* could not read because they are still in the cloud.
///
/// Cleared per-root by [`begin_build`] rather than wholesale: two folders can be
/// open at once, and a build of one must not erase what the other recorded.
static UNAVAILABLE: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

fn with<R>(cell: &Mutex<Option<HashSet<PathBuf>>>, f: impl FnOnce(&mut HashSet<PathBuf>) -> R) -> R {
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(HashSet::new))
}

/// Drop everything the previous build of `root` recorded.
///
/// Called once at the top of the pipeline. The record describes *this* build —
/// a file that arrived since the last one must not still be counted against the
/// gate, and the whole point of the gate being the build's own output is that it
/// is recomputed fresh every attempt rather than accumulated.
pub fn begin_build(root: &Path) {
    with(&UNAVAILABLE, |set| set.retain(|p| !p.starts_with(root)));
}

/// Record that `path` could not be read because its content is in the cloud.
///
/// Call this from any site that classifies a read failure as
/// `icloud::is_offline_not_absent` (or that skips a read because
/// `icloud::is_evicted` said the bytes are not there). Cheap and idempotent —
/// call it every time you notice, not only the first.
pub fn note_unavailable(path: &Path) {
    with(&UNAVAILABLE, |set| {
        set.insert(path.to_path_buf());
    });
}

/// How many of `root`'s sources this build had to do without.
///
/// This is the gate's live input, and the reason it is not the scan count: the
/// scan runs before the build and prunes dot-directories, so it cannot see an
/// eviction that happened afterwards. Paths that have since materialized are
/// filtered out, so a count taken after a long build is honest about the present
/// rather than about when each absence was noticed.
pub fn outstanding(root: &Path) -> usize {
    with(&UNAVAILABLE, |set| {
        set.iter()
            .filter(|p| p.starts_with(root))
            .filter(|p| crate::build::icloud::is_still_in_the_cloud(p))
            .count()
    })
}

/// Does `path` decide the site's *shape*?
///
/// The split that decides whether a build is worth publishing. An absent image
/// costs the reader a picture; an absent markdown file costs them the page —
/// its title becomes the directory name and its date becomes `Unknown`, because
/// every rung of `components::date::extract_date_from_doc` falls through on
/// empty frontmatter. The second is not a partial site, it is a wrong one, and
/// moss must not publish it over a site it already has.
///
/// **Listed positively, and the list is what moss renders from**: page sources,
/// `config.toml`, the user stylesheet. The tempting inverse — "structural means
/// anything that is not media" — fails on the vaults this is for. A user's
/// folder holds `.zip`, `.psd`, `.key`, `.sketch`; the provider evicts them like
/// anything else and will not download one nothing ever opens. Under the
/// inverse, one such file withholds every publish forever, and the user has no
/// site at all — the unclearable gate `outcome::Disposition::Report` warns
/// about, reached by a different door. Everything on this list is something
/// moss itself reads, so an arrival always schedules the rebuild that clears it.
///
/// The cost of the positive form is that a *new* page format is decoration
/// until someone adds it here. That is a real gap, and the reason it is
/// acceptable is that a new format has to be added to the scan's classification
/// to be built at all — the same commit passes through here.
pub fn is_structural_source(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        // Page sources — the formats `read_page_source` renders from.
        "md" | "markdown" | "html" | "htm" | "docx" | "doc" | "pages" | "ipynb"
        // `config.toml`, and `.moss/theme/style.css` plus its partials.
        | "toml" | "css"
    )
}

/// How many of `root`'s **structural** sources this build had to do without.
///
/// [`outstanding`] answers "how much of this site is still arriving", which is
/// the right question for a progress counter and for whether to say anything at
/// all. This answers the different question the publish decision turns on: is
/// what this build produced a *true* rendering of the site, or a rendering of
/// the parts that happened to be local? Same `is_still_in_the_cloud` filter, so
/// a source that landed mid-build does not hold anything back.
pub fn structural_outstanding(root: &Path) -> usize {
    with(&UNAVAILABLE, |set| {
        set.iter()
            .filter(|p| p.starts_with(root))
            .filter(|p| is_structural_source(p))
            .filter(|p| crate::build::icloud::is_still_in_the_cloud(p))
            .count()
    })
}

/// Did this build go without a source that decides the site's *shape*?
///
/// The publish decision's own count, above what [`structural_outstanding`] can
/// see on its own. Both halves, for the same reason the gate's `cloud_outstanding`
/// combines them rather than picking one. `ledger_structural` is what a read site actually tried and
/// could not get. `evicted_at_scan` is what was dataless when the walk ran,
/// which is the only half that can see a source **no read site ever reached** —
/// and that is not a corner case, it is the other reported shape: a vault whose
/// page sources were all still downloading produced no documents at all, so the
/// render pass synthesized the empty-folder home page and the user was shown
/// moss's onboarding blueprint grid with their whole site missing behind it.
/// No `read_page_source` call happened, so the ledger alone would
/// still have called that build complete.
///
/// **`evicted_at_scan` must already be filtered to what is still in the cloud**
/// — the caller does it, because this stays pure so the policy is testable off
/// macOS. That filter is what keeps the two decisions in step: it makes
/// "withheld" imply the gate's own count is non-zero, so a build that declines
/// to publish is always one the gate can raise a screen for. Without it a
/// source that arrived mid-build would withhold the publish *and* leave the
/// gate down, and the user would get neither a site nor an explanation.
///
/// Returns a COUNT, not a flag, because the decision has to be explainable.
/// The log that says why a build was withheld used to re-query
/// `cloud_ledger::structural_outstanding()` — a live read, asking about a
/// different moment than the one the decision was made in — so a build
/// withheld on the scan's evidence alone printed "0 structural source(s) are
/// still downloading". The number is now the decision's own.
///
/// The two halves are combined with `max`, exactly as `cloud_outstanding`
/// combines its own two: they are overlapping views of one set, so a source
/// both the scan and a read site saw must not be counted twice.
pub fn structural_missing_count(
    evicted_at_scan: &[std::path::PathBuf],
    ledger_structural: usize,
) -> usize {
    let at_scan = evicted_at_scan
        .iter()
        .filter(|p| is_structural_source(p))
        .count();
    ledger_structural.max(at_scan)
}

/// Every structural source under `root` this build had to do without, as of
/// now.
///
/// The path-returning twin of [`structural_missing_count`]: that function
/// answers "is the build's own decision required" with a count meant to
/// explain itself in a log line; this answers the different
/// question a publish-time refusal has to — which files, so a person can act
/// on the list rather than a number. Same two sources, **unioned** rather than
/// `max`'d, because a name is either on the list or not — there is no double
/// counting to avoid the way there is with two counts of possibly the same set.
///
/// `evicted_at_scan` carries the same contract as
/// [`structural_missing_count`]'s: already filtered to what is still in the
/// cloud, because that filtering needs `icloud::is_still_in_the_cloud`, which
/// stays out of this module so the policy here is testable off macOS.
pub fn structural_stale_paths(evicted_at_scan: &[std::path::PathBuf], root: &Path) -> Vec<PathBuf> {
    let mut stale: HashSet<PathBuf> = evicted_at_scan
        .iter()
        .filter(|p| is_structural_source(p))
        .cloned()
        .collect();
    with(&UNAVAILABLE, |set| {
        stale.extend(
            set.iter()
                .filter(|p| p.starts_with(root))
                .filter(|p| is_structural_source(p))
                .filter(|p| crate::build::icloud::is_still_in_the_cloud(p))
                .cloned(),
        );
    });
    let mut v: Vec<PathBuf> = stale.into_iter().collect();
    v.sort();
    v
}

/// Every path this build recorded under `root`, materialized or not.
///
/// Unfiltered, unlike [`outstanding`] — for diagnostics and tests that want to
/// know what was noticed rather than what is still outstanding.
pub fn noted_under(root: &Path) -> Vec<PathBuf> {
    with(&UNAVAILABLE, |set| {
        let mut v: Vec<PathBuf> = set.iter().filter(|p| p.starts_with(root)).cloned().collect();
        v.sort();
        v
    })
}

#[cfg(test)]
#[path = "cloud_ledger_tests.rs"]
mod tests;
