//! Full-text search index generation (Pagefind, embedded as a library).
//!
//! moss embeds the [`pagefind`] crate rather than shelling out to the CLI or
//! shipping a whole-index-in-one-file client library. Pagefind indexes
//! *already-rendered HTML* and writes a sharded, WASM-backed static bundle;
//! the browser fetches only the chunks a query touches. No server, no API
//! key — the CDN serving the site stays the only server.
//!
//! # Where this runs
//!
//! **Nowhere in the build.** This module is the pure indexing engine: hand it a
//! directory of rendered HTML, get a bundle back. Scheduling, publication and
//! manifest registration live in its sibling [`search_lane`], which owns the
//! whole of the adopt-don't-await contract.
//!
//! Pagefind is the archetypal **generation-free** worker: its output is a pure
//! function of the emitted page set, referenced by a fixed URL, and its cost
//! scales with *corpus size* (386 pages) rather than *delta size* (1 page).
//! Indexing a real 386-page site takes ~5.2 s, and most of that is not even the index
//! build: `get_files()` is `build_indexes()` (790–1070 ms) plus
//! `write_files_to_memory()`, which gzips the ~8 MB bundle at
//! `Compression::best()` single-threaded (2987–3723 ms). It used to be
//! dispatched into the build's worker `JoinSet`, where the seal awaited it and
//! the seal in turn held the stage-write lock the *next* build needed — so
//! every save paid for the previous save's index.
//!
//! [`search_lane`]: super::search_lane
//!
//! # Gating
//!
//! One switch: `[site].search` in the project's `.moss/config.toml` — the
//! per-site Services-tab toggle, absent key = off. Resolved once at the
//! `SiteConfig` construction site in `build/pipeline.rs` into
//! `LayoutConfig::assets.search` and read by both the nav button and the
//! lane. (Search graduated out of `experimental.preview_features`
//! 2026-08-31.)
//!
//! There is deliberately no second, mode-dependent switch. There used to be —
//! `site_url.is_deployed()` — and it meant an author who enabled search saw
//! nothing in preview at all. It was deleted rather than replaced with a
//! preview-mode bit, because build output is mode-independent by design; see
//! `render/blocking.rs`'s `has_search`.
//!
//! # Chinese segmentation
//!
//! Pagefind's plain indexer splits text on whitespace, so a run of Han
//! characters ("這是一段文字") indexes as one giant token and Chinese queries
//! match almost nothing — unacceptable given 潮汐 (harborweekly), the
//! feature's validation target, is majority-Chinese-content.
//!
//! Pagefind's own answer is its `extended` feature (charabia → lindera). moss
//! does not use it: it cost +48.1 MB of release binary and pulled a second
//! reqwest/hyper-util/rustls stack plus a dozen duplicate crate trees into a
//! repo whose dependency rule is "no duplicate stacks" (see the comment above
//! the `pagefind` line in `Cargo.toml`).
//!
//! Instead moss segments the text itself before Pagefind ever sees it, the
//! same trick `vitepress-plugin-pagefind` uses (it pre-segments with the
//! browser's `Intl.Segmenter`): [`segment_html`] rewrites every text node,
//! replacing each run of Han characters with its `jieba-rs` words joined by
//! spaces. Pagefind then indexes ordinary whitespace-delimited words.
//!
//! The segmented HTML is written to a throwaway [`tempfile::TempDir`] and
//! Pagefind is pointed at *that*; `site_dir` is the real build output that
//! gets deployed and is never mutated.
//!
//! Scope: Han runs only. Japanese kana and Hangul are left untouched — jieba
//! is a Chinese dictionary and would mis-split them. Latin/ASCII text is
//! byte-for-byte unchanged. `<script>`, `<style>`, `<code>` and `<pre>` are
//! skipped so code samples and URLs are not corrupted.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::LazyLock;

use jieba_rs::Jieba;
use lol_html::html_content::ContentType;
use lol_html::{element, text, HtmlRewriter, Settings};
use rayon::prelude::*;

/// One file of the generated Pagefind bundle, addressed by its path *within*
/// the bundle directory (e.g. `pagefind.js`, `wasm.en.pagefind`,
/// `fragment/en_1a2b3c.pf_fragment`).
#[derive(Debug, Clone)]
pub struct SearchIndexFile {
    /// Path relative to the bundle root, exactly as Pagefind reports it.
    pub rel_path: String,
    /// File contents (already gzip-compressed where Pagefind compresses).
    pub bytes: Vec<u8>,
}

/// One built bundle, together with **how much of the corpus it actually
/// covers**.
///
/// The census is not diagnostics: it is what stops a partial index from being
/// published under the fingerprint of the complete page set. Every page-level
/// failure in [`segmented_copy`] is a skip-with-a-warning (one malformed page
/// should cost its own indexing, not the whole site's), and the directory walk
/// swallows its own errors — so "zero pages" and "this site has no indexable
/// content" are the same value at this level. Only the caller, which knows how
/// many pages the page set contains, can tell them apart, and it can only do so
/// if the numbers travel with the bundle. See `search_lane::publish_bundle`.
#[derive(Debug, Clone, Default)]
pub struct SearchIndex {
    /// The bundle files, addressed within the bundle directory.
    pub files: Vec<SearchIndexFile>,
    /// Pages handed to Pagefind: found by the walk **and** read and segmented.
    pub pages: usize,
    /// Pages the walk found but that could not be read or rewritten. Non-zero
    /// means this bundle covers less than the tree it was pointed at.
    pub skipped: usize,
}

/// Glob Pagefind walks over the site output. Matches Pagefind's own default
/// (`**/*.{html}`) — spelled out here so a future exclusion (e.g. skipping
/// `_moss/`) has one place to live.
const INDEX_GLOB: &str = "**/*.html";

/// Elements whose text is markup, not prose. Segmenting them would insert
/// spaces into code samples, URLs, JS string literals and CSS.
const SKIP_TEXT_IN: &str = "script,style,code,pre";

/// Wall-clock breakdown of one index build, in milliseconds.
///
/// Which phase dominates was a load-bearing unknown, and it
/// is now measured: neither segment nor fossick, and not the index build
/// either — `emit_bundle` is ~78% of the run and ~78% of THAT is a
/// single-threaded gzip of the whole bundle. A retained per-page
/// `PagefindIndex` would not shrink it. Nothing in Pagefind's API has an
/// incremental form; the answer is to
/// stop *awaiting* the index rather than to make it smaller.
///
/// Exposed through [`last_index_timing`] rather than returned, mirroring
/// `build::parse_cache::last_stats` — the measurement is for a `--ignored`
/// real-vault test, and threading it back through `build_search_index` would
/// put a benchmark in the build's signature.
#[derive(Debug, Clone, Copy, Default)]
pub struct IndexTiming {
    pub pages: usize,
    pub segment_ms: u128,
    pub fossick_ms: u128,
    /// `get_files()` end to end: `build_indexes()` + `write_files_to_memory()`.
    /// The gzip half dominates — see the note at the call site.
    pub emit_bundle_ms: u128,
}

static LAST_TIMING: std::sync::Mutex<Option<IndexTiming>> = std::sync::Mutex::new(None);

/// Number of whole-site index computations this process has run. Test-facing;
/// this is what "a burst of N rebuilds produces one reindex, not N" is measured
/// with, now that the lane rather than a ticket decides.
static INDEX_BUILDS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The most recent [`IndexTiming`], or `None` if no index has been built in
/// this process.
pub fn last_index_timing() -> Option<IndexTiming> {
    LAST_TIMING.lock().ok().and_then(|t| *t)
}

/// How many whole-site index builds have actually run (see [`INDEX_BUILDS`]).
pub fn index_build_count() -> usize {
    INDEX_BUILDS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Serializes every test that builds an index.
///
/// [`INDEX_BUILDS`] is process-global and `cargo test` runs the suite on
/// several threads, so without this a sibling test's index build lands inside
/// another's measured delta and "a burst of N requests ran one index" fails for
/// a reason that has nothing to do with the lane.
#[cfg(test)]
pub(crate) fn lock_index_counter() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The jieba dictionary, loaded once per process.
///
/// `Jieba::new()` parses a ~5 MB embedded dictionary. It used to be built once
/// per index build, which is per-publish today and would be per-*save* once
/// preview indexes — a fixed cost paid again for every rebuild, and
/// one that no per-page cache could ever collapse. It is immutable and `Sync`,
/// so the segmentation threads share this one.
static JIEBA: LazyLock<Jieba> = LazyLock::new(Jieba::new);

/// True for the Han ranges jieba is trained on. Kana and Hangul are
/// deliberately absent — see the module docs.
fn is_han(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0x4DBF        // CJK Unified Ideographs Extension A
        | 0x4E00..=0x9FFF      // CJK Unified Ideographs
        | 0xF900..=0xFAFF      // CJK Compatibility Ideographs
        | 0x2_0000..=0x2_A6DF  // Extension B
        | 0x2_A700..=0x2_EBEF  // Extensions C–F
        | 0x2_F800..=0x2_FA1F  // Compatibility Ideographs Supplement
    )
}

/// Replace every run of Han characters with its jieba words joined by single
/// spaces, leaving everything else byte-for-byte alone. Returns `None` when
/// the input has no Han at all, so pure-Latin text costs one scan and no
/// allocation.
///
/// A space is also inserted where a Han run abuts non-space text ("moss是一個"
/// → "moss 是 一個"): without it Pagefind would index "moss是" as one token.
// Every slice below is bounded by an index from `str::find`/`str::len`, which
// only ever land on a char boundary — none of these can panic.
#[allow(clippy::string_slice)]
fn segment_han_runs(text: &str, jieba: &Jieba) -> Option<String> {
    if !text.chars().any(is_han) {
        return None;
    }

    let mut out = String::with_capacity(text.len() + text.len() / 4);
    let mut rest = text;
    while let Some(start) = rest.find(is_han) {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail.find(|c: char| !is_han(c)).unwrap_or(tail.len());

        if out.chars().next_back().is_some_and(|c| !c.is_whitespace()) {
            out.push(' ');
        }
        let words: Vec<&str> = jieba
            .cut(&tail[..end], true)
            .into_iter()
            .map(|t| t.word)
            .collect();
        out.push_str(&words.join(" "));

        rest = &tail[end..];
        if rest.chars().next().is_some_and(|c| !c.is_whitespace()) {
            out.push(' ');
        }
    }
    out.push_str(rest);
    Some(out)
}

/// Rewrite the text nodes of an HTML document through [`segment_han_runs`].
///
/// Streaming rewrite via `lol_html` (moss's existing HTML-rewriting tool, cf.
/// `build/site_meta/spa_inject.rs`) rather than a DOM parse: only text nodes
/// are ever touched, so tag names, attributes (`href`, `src`, `id`, `class`,
/// `data-*`) and document structure round-trip unchanged.
///
/// Text nodes can arrive in several chunks; they are buffered and replaced as
/// one at `last_in_text_node` so a word is never split across a chunk
/// boundary. Replacement uses `ContentType::Html` because `as_str()` hands
/// back the raw source text — entities such as `&amp;` must go back out as
/// they came in, not re-escaped.
fn segment_html(html: &str, jieba: &Jieba) -> Result<String, String> {
    // Depth of the innermost SKIP_TEXT_IN element we are inside, if any.
    let skip_depth = Rc::new(RefCell::new(0usize));
    let depth_for_open = Rc::clone(&skip_depth);
    let buffer = RefCell::new(String::new());

    let mut output = Vec::with_capacity(html.len());
    {
        let mut rewriter = HtmlRewriter::new(
            Settings {
                element_content_handlers: vec![
                    element!(SKIP_TEXT_IN, move |el| {
                        let closing = Rc::clone(&depth_for_open);
                        // No end tag in the source (self-closing / malformed)
                        // means no text children to protect, so no depth.
                        if let Some(handlers) = el.end_tag_handlers() {
                            *depth_for_open.borrow_mut() += 1;
                            handlers.push(Box::new(move |_end| {
                                let mut d = closing.borrow_mut();
                                *d = d.saturating_sub(1);
                                Ok(())
                            }));
                        }
                        Ok(())
                    }),
                    text!("*", |chunk| {
                        if *skip_depth.borrow() > 0 {
                            return Ok(());
                        }
                        let mut buf = buffer.borrow_mut();
                        buf.push_str(chunk.as_str());
                        if chunk.last_in_text_node() {
                            let replacement =
                                segment_han_runs(&buf, jieba).unwrap_or_else(|| buf.clone());
                            chunk.replace(&replacement, ContentType::Html);
                            buf.clear();
                        } else {
                            chunk.remove();
                        }
                        Ok(())
                    }),
                ],
                ..Settings::default()
            },
            |c: &[u8]| output.extend_from_slice(c),
        );
        rewriter.write(html.as_bytes()).map_err(|e| e.to_string())?;
        rewriter.end().map_err(|e| e.to_string())?;
    }

    String::from_utf8(output).map_err(|e| e.to_string())
}

/// Build a throwaway copy of `site_dir`'s HTML tree with Han text segmented,
/// and return the `TempDir` owning it. Only `.html` files are copied —
/// `INDEX_GLOB` is the only thing Pagefind reads, and everything else in the
/// build output (assets, JSON, the bundle itself) is irrelevant to indexing.
///
/// `site_dir` is the deployed build output and is never written to. The
/// returned `TempDir` deletes the copy on drop, including on the error paths
/// after it.
///
/// A file that fails to read or rewrite is skipped with a warning rather than
/// failing the build: one malformed page should cost its own indexing, not the
/// whole site's.
///
/// The per-page work runs on rayon's global pool, like the markdown-parse and
/// HTML-render loops in `render/blocking.rs`. It is safe to nest here in a way
/// it would not be inside those loops: `segmented_copy` runs to completion
/// *before* Pagefind (itself a rayon user) is handed the directory, so the two
/// never contend for the pool. Keep them sequential phases — see
/// `scan/scan.rs` and `media/image.rs`, which both carry scars from nesting.
///
/// Returns the scratch dir plus `(pages copied, pages skipped)` — the census
/// [`SearchIndex`] carries onward. A skip is a *shortfall*, not a detail: the
/// bundle built from this copy covers fewer pages than the tree it was pointed
/// at, and only the caller can decide whether that is publishable.
fn segmented_copy(site_dir: &Path) -> Result<(tempfile::TempDir, usize, usize), String> {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let temp = tempfile::tempdir()
        .map_err(|e| format!("failed to create search-index scratch dir: {}", e))?;
    let copied = AtomicUsize::new(0);

    // Walking is I/O-bound and cheap; collect first so the expensive part
    // (read → segment → write, dominated by jieba) is what gets parallelised.
    let pages: Vec<std::path::PathBuf> = walkdir::WalkDir::new(site_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_type().is_file()
                && e.path().extension().and_then(|x| x.to_str()) == Some("html")
        })
        .map(|e| e.path().to_path_buf())
        .collect();

    pages
        .par_iter()
        .try_for_each(|path| -> Result<(), String> {
            let Ok(rel) = path.strip_prefix(site_dir) else {
                return Ok(());
            };
            let html = match std::fs::read_to_string(path) {
                Ok(html) => html,
                Err(e) => {
                    log::warn!(target: "search", "skipping {:?} for search index: {}", rel, e);
                    return Ok(());
                }
            };
            let segmented = match segment_html(&html, &JIEBA) {
                Ok(s) => s,
                Err(e) => {
                    log::warn!(target: "search", "skipping {:?} for search index: {}", rel, e);
                    return Ok(());
                }
            };
            let dest = temp.path().join(rel);
            if let Some(parent) = dest.parent() {
                // Concurrent create_dir_all on the same ancestor is fine —
                // it treats AlreadyExists as success.
                // allow:raw_write a tempfile scratch dir, not the build tree
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to write search-index scratch dir: {}", e))?;
            }
            std::fs::write(&dest, segmented)  // allow:raw_write scratch TempDir for the pagefind index, never the output tree
                .map_err(|e| format!("failed to write search-index scratch dir: {}", e))?;
            copied.fetch_add(1, Ordering::Relaxed);
            Ok(())
        })?;

    let copied = copied.load(Ordering::Relaxed);
    Ok((temp, copied, pages.len() - copied))
}

/// Build a Pagefind index over a directory of rendered HTML and return the
/// bundle as in-memory files. Reads `site_dir` and writes only into a
/// temporary segmented copy of it (see [`segmented_copy`]); publishing the
/// bundle is the caller's job (see [`search_lane`][super::search_lane]).
///
/// Uncancellable convenience form of [`build_search_index_cancellable`], for
/// the publish-path settle and for tests.
///
/// # Async bridging
///
/// `pagefind`'s programmatic API (`pagefind::api::PagefindIndex`) is
/// tokio-async, while `generate_blocking_content` — the whole blocking render
/// phase — is synchronous and may itself be running inside a `spawn_blocking`
/// worker of moss's main runtime. Building a nested runtime and calling
/// `block_on` from a runtime *thread* panics, so this spawns a plain
/// `std::thread`, stands up a private current-thread runtime there, and joins
/// it. That is unconditionally safe regardless of the ambient context, and
/// costs one thread once per publish build.
///
/// Returns `Err` with a human-readable message on any indexing failure. The
/// caller treats that as non-fatal: a site that can't be indexed still ships.
pub fn build_search_index(site_dir: &Path) -> Result<SearchIndex, String> {
    build_search_index_cancellable(site_dir, &|| false)
        .map(|index| index.unwrap_or_default())
}

/// [`build_search_index`] with cooperative cancellation at phase boundaries.
///
/// `cancelled` is polled after segmentation, after `add_directory` and
/// immediately before `get_files()` — and nowhere inside them, because none of
/// the three is interruptible. `get_files()` is the 3–4 s gzip block, so the
/// worst case of a late cancellation is one wasted pass **off the critical
/// path**; the boundary before it is the one that matters, since a superseded
/// request usually arrives while the previous one is still fossicking.
///
/// `Ok(None)` means cancelled: nothing was produced and nothing is wrong.
pub fn build_search_index_cancellable(
    site_dir: &Path,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Option<SearchIndex>, String> {
    INDEX_BUILDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    // Held for the whole indexing run; dropped (and deleted) on every exit
    // path below, including the error ones.
    let t_segment = std::time::Instant::now();
    let (segmented, copied, skipped) = segmented_copy(site_dir)?;
    let segment_ms = t_segment.elapsed().as_millis();
    if cancelled() {
        return Ok(None);
    }
    let dir = segmented.path().to_path_buf();

    // `std::thread::scope` (not `Builder::spawn` + `join`) so the borrowed
    // `cancelled` predicate can cross into the indexing thread without being
    // `'static`. The thread is joined before the scope ends, so the borrow is
    // sound and the ambient-runtime argument above is unaffected.
    std::thread::scope(|scope| {
        let handle = std::thread::Builder::new()
        .name("moss-pagefind".into())
        .spawn_scoped(scope, move || -> Result<Option<SearchIndex>, String> {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("failed to start search-index runtime: {}", e))?;

            rt.block_on(async move {
                // `None` config = Pagefind's own defaults: root selector
                // `html`, `keep_index_url` false (so `/posts/hello/index.html`
                // is reported as `/posts/hello/`, matching moss's pretty
                // URLs), no playground output, language read per-page from
                // `<html lang>` (moss emits per-page `lang`, so a bilingual
                // site gets one index per language automatically).
                let mut index = pagefind::api::PagefindIndex::new(None)
                    .map_err(|e| format!("failed to create search index: {}", e))?;

                let t_fossick = std::time::Instant::now();
                let page_count = index
                    .add_directory(
                        dir.to_string_lossy().into_owned(),
                        Some(INDEX_GLOB.to_string()),
                    )
                    .await
                    .map_err(|e| format!("failed to read pages for search index: {}", e))?;
                let fossick_ms = t_fossick.elapsed().as_millis();

                if page_count == 0 {
                    // Not an error and not necessarily a success — the census
                    // travels so `publish_bundle` can tell "no indexable
                    // content" from "the tree went away".
                    return Ok(Some(SearchIndex { files: Vec::new(), pages: copied, skipped }));
                }
                // The boundary that earns cancellation its keep: everything
                // after this point is the uncancellable gzip.
                if cancelled() {
                    return Ok(None);
                }

                // `get_files()` is `build_indexes()` PLUS `write_files_to_memory()`
                // — and the second half dominates. It gzips the whole bundle at
                // `Compression::best()`, single-threaded
                // (pagefind-1.5.2/src/output/mod.rs:322-378). Measured split on a
                // 386-page corpus: build_indexes 790-1070ms, serialize+gzip
                // 2987-3723ms.
                //
                // This field used to be named `build_indexes_ms`, which attributed
                // all of it to the index build. That conflation is what hid the
                // real cost: it made a retained per-page
                // index look like the lever, when ~78% of the time is a
                // compression pass that a per-page index would not shrink.
                // Renamed to `emit_bundle_ms` so the name cannot mislead again.
                // Neither half has an incremental form in Pagefind's API.
                let t_build = std::time::Instant::now();
                let files = index
                    .get_files()
                    .await
                    .map_err(|e| format!("failed to build search index: {}", e))?;

                let timing = IndexTiming {
                    pages: page_count,
                    segment_ms,
                    fossick_ms,
                    emit_bundle_ms: t_build.elapsed().as_millis(),
                };
                log::info!(
                    target: "timing",
                    "[search] {} pages: segment {}ms, fossick {}ms, emit_bundle {}ms (index build + gzip)",
                    timing.pages,
                    timing.segment_ms,
                    timing.fossick_ms,
                    timing.emit_bundle_ms,
                );
                if let Ok(mut slot) = LAST_TIMING.lock() {
                    *slot = Some(timing);
                }

                Ok(Some(SearchIndex {
                    files: files
                        .into_iter()
                        .map(|f| SearchIndexFile {
                            rel_path: f.filename.to_string_lossy().replace('\\', "/"),
                            bytes: f.contents,
                        })
                        .collect(),
                    pages: copied,
                    skipped,
                }))
            })
        })
        .map_err(|e| format!("failed to spawn search-index thread: {}", e))?;

        handle
            .join()
            .map_err(|_| "search-index thread panicked".to_string())?
    })
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
