//! Ship-time pruning of unreferenced `.webp` variants (moss#976 B2).
//!
//! `scan.rs` walks and encodes every image file under the site folder — there
//! is no reference-graph filter anywhere in the encode path (moss#976
//! measured 534 orphaned `.webp` files / 31.1 MB per generation on a real
//! site: vault images under `assets/` that no markdown, template, feed, or
//! plugin output ever links to).
//!
//! **Scope: `.webp` only, deliberately not the raster (`.png`/`.jpg`)
//! fallback tier.** The raster fallback is referenced a third way that never
//! appears in `stage_dir`: `infra/newsletter.rs`'s `ImageContext::EmailBody`
//! renders a bare `<img>` into an in-memory email string at send time, never
//! written to disk (verified — `render_email_html` returns `(String,
//! String)`, no `fs::write` call in that path). A ship-time-only scan is
//! structurally blind to that reference. `.webp` variants have no such
//! out-of-band consumer — the WebP `<source>` only ever exists inside
//! rendered `<picture>` markup that IS in `stage_dir` — so pruning them is
//! safe under invariant 6 without also solving the email case. Pruning the
//! raster tier is moss#976 follow-up work, gated on either scanning
//! newsletter-eligible content or accepting that risk explicitly.
//!
//! ## How "referenced" is decided
//!
//! Every `.html`, `.xml`, `.json`, `.js`, `.txt`, `.css`, `.svg`, and
//! `.ipynb` file under `stage_dir` is scanned for any bare path-like token
//! ending in a known image extension — not an attribute- or tag-specific
//! parse. This is deliberate: `src=`/`srcset=`/`href=`/`data-*=`/CSS
//! `url(...)`/JSON string values/XML `<url>` and enclosure `url=` all put the
//! same shape of token (a quoted or bare path immediately followed by a
//! delimiter) in the output, so one generic scan catches a plugin, a feed,
//! an SVG, a notebook cell, and a stylesheet without a parser per format to
//! keep in sync — at the cost of occasional over-matching, which only makes
//! pruning MORE conservative (a spurious "referenced" verdict costs bytes,
//! never correctness). Per invariant 6, the failure this must never produce
//! is a false NEGATIVE — treating a live reference as absent — and a
//! permissive token match is biased against exactly that.
//!
//! The token's character class is a NEGATED class over URL/attribute
//! delimiters (whitespace, quotes, `<>()`, and `:` to block a URL scheme),
//! not an enumerated allowlist of "safe" bytes. An enumerated allowlist is
//! only ever as complete as the encoder it was copied from — moss shipped
//! exactly that failure once (docs/archive/2026-08-06-orphan-prune-false-negative-and-parse-cache-gate.md
//! Bug A: one un-encoded asset-URL emitter produced raw non-ASCII bytes the
//! old ASCII-only class couldn't span, so the match backtracked to a
//! truncated tail and a live reference read as an orphan). The negated
//! class survives *any* future emitter that ships raw bytes — CJK,
//! Cyrillic, Arabic, Devanagari, combining marks, emoji — because it does
//! not need to enumerate them in advance. See the inline comment on the
//! regex itself for the exact excluded set and why the start class is
//! narrower than the continuation class.
//!
//! Extracted tokens are decoded (percent/entity, query/fragment stripped) and
//! then read TWO ways, whose union is the reference set. Both are
//! over-approximations, and a union of over-approximations is still one —
//! the only direction invariant 6 permits.
//!
//! 1. **Path suffixes** of the token with its `./`/`../`/`/` prefixes
//!    stripped, matched against each `.webp` output key: a reference written
//!    at a different `../` depth than the manifest's site-root-relative key
//!    must still count. See [`path_suffixes`].
//! 2. **Resolved against the file the token was found in**, via
//!    `resolve_to_root_relative`. Suffixes only ever get SHORTER than the
//!    token, so reading (1) alone cannot match a reference written shallower
//!    than the key — which is the ordinary shape for every non-HTML type
//!    scanned here, whose URLs are relative to their own file. A
//!    `gallery/style.css` holding `url(assets/x.webp)` means
//!    `gallery/assets/x.webp`; by suffixes alone it reads as
//!    `assets/x.webp`, matches nothing, and a live reference is deleted.

use std::collections::HashSet;
use std::path::Path;

/// File extensions scanned for embedded image references.
///
/// `svg` and `ipynb` are text formats too (XML and JSON respectively) — a
/// `.webp` referenced only from an SVG `<image href>` or a notebook's
/// markdown/HTML cell output is exactly as prunable-in-error as one
/// referenced only from a feed or a plugin's data attribute. Both parse as
/// UTF-8 text under `std::fs::read_to_string`, so the generic token scan
/// below needs no format-specific change to cover them.
const SCAN_EXTENSIONS: &[&str] = &["html", "xml", "json", "js", "txt", "css", "svg", "ipynb"];

/// What one walk of `stage_dir` learned: the reference set, and whether it is
/// complete.
///
/// The token matching above errs wide on purpose — a spurious "referenced"
/// verdict costs bytes, never correctness. The I/O underneath it used to err
/// the other way: an unreadable page (a cloud eviction, a mid-flight write, a
/// permission fault) was skipped silently, which SHRINKS `tails`, and a smaller
/// reference set authorizes MORE deletion. One unreadable page could therefore
/// permit deleting every variant only that page referenced — and since moss#1085
/// the deletion is persisted as a cross-build verdict, so recovery takes several
/// builds. That is the mechanism behind moss#976 (525 files deleted, 207 images
/// 404-ing for real readers). Pruning saves bytes; deleting wrongly costs the
/// site, so the scan reports its own blindness and the caller fails closed.
pub struct ReferenceScan {
    pub tails: HashSet<String>,
    /// Paths the scan could not read. Non-empty means `tails` is a SUBSET of
    /// the truth, so pruning from it would delete referenced files.
    pub unreadable: Vec<std::path::PathBuf>,
}

/// Walk `stage_dir` and collect every embedded image-path token from the
/// scannable text file types, normalized to a site-root-relative tail with no
/// leading `./`/`../`.
///
/// Pure text scan — no HTML/XML/JSON parsing, so it survives whatever shape
/// a plugin, theme template, or feed emits, per invariant 6.
///
/// Returns a [`ReferenceScan`], not a bare set: a caller that deletes from this
/// answer must know whether the walk could read everything.
pub fn extract_referenced_tails(stage_dir: &Path) -> ReferenceScan {
    // The continuation class is a NEGATED class over URL/attribute
    // delimiters, not an enumerated allowlist — every byte that isn't a
    // structural delimiter is a legal filename byte, ASCII or not. This is
    // the defence-in-depth fix for moss's own contract violation that
    // shipped one un-encoded asset-URL emitter (see
    // docs/archive/2026-08-06-orphan-prune-false-negative-and-parse-cache-gate.md
    // Bug A): the pruner is correct against a fully-percent-encoded URL, but
    // an enumerated allowlist is only ever as complete as the encoder it was
    // copied from, and a *future* un-encoded emitter — raw CJK, Cyrillic,
    // Arabic, Devanagari, combining marks, emoji, whatever a plugin author
    // writes by hand — must not be able to reproduce that failure. A source
    // filename like `Grandma's House.png` is never slugified (only the
    // directory is; filename/case survive verbatim, see
    // `asset_paths::to_webp`), so its emitted URL is `grandma's-house.webp`,
    // apostrophe intact — and a raw CJK filename's URL is the filename
    // verbatim, non-ASCII intact. Missing a character from the continuation
    // class doesn't just mis-normalize the match — it SPLITS it: the regex
    // backtracks to the next start position and extracts a truncated tail
    // (e.g. `s-house.webp`, or the truncated tail of a multi-byte glyph)
    // whose suffixes never equal the real manifest key, so the file reads as
    // unreferenced and gets deleted — exactly the false negative invariant 6
    // forbids. The excluded set is deliberately narrow: whitespace, the two
    // HTML/attribute quote characters, `<`/`>` (tag boundaries), `(`/`)`
    // (CSS `url(...)`, Markdown link targets), and `:` (so a scheme like
    // `https:` can never be part of a match — see the `//`-guard below,
    // which handles the rest of "don't protect an external URL"). Notably
    // NOT excluded: `#`/`;` — `&` and `'` are HTML-escaped to `&amp;`/`&#39;`
    // by `html_escape` before they ever reach a file this scans, so the
    // token must span the whole entity for `html_entities::decode` (in
    // `decode_reference`) to turn it back into the real character
    // afterward.
    //
    // That escaping argument holds for HTML/XML but not for the other
    // `SCAN_EXTENSIONS` (`css`, `js`, `json`, `txt`, `svg`, `ipynb`), where a
    // literal `'` can appear — and neither spelling of the class is safe alone:
    //
    // - excluding `'`, `Grandma's-House.webp` in a `.js` file truncates to
    //   `s-House.webp`, whose suffixes never equal the key → deleted;
    // - including `'`, the far more common shape in those same files is a
    //   quoted list, and `'a.webp','b.webp'` matches as ONE token
    //   `a.webp','b.webp` whose only `/`-suffix is itself → BOTH deleted.
    //
    // So the scan runs BOTH, and unions the tails (see `TOKEN_PATTERNS`). Each
    // pass is an over-approximation of the reference set and a union of
    // over-approximations is still one, which is the only direction invariant 6
    // permits: the extra tokens each pass contributes are tails no manifest key
    // equals, so they protect nothing that should have been pruned. It costs a
    // second regex pass over text already in memory.
    //
    // Also NOT excluded: `,`/`?` — `decode_reference` strips everything
    // from the first `?`/`#` downstream, so over-including a query string in
    // the match is harmless (regex backtracking finds the real `.ext`
    // boundary), and `,` is a legal, unescaped filename byte. The START
    // class is narrower than the continuation class on purpose: it accepts
    // ASCII alnum/underscore, `%`, or any non-ASCII scalar value (so a token
    // can both START and CONTINUE in CJK/Cyrillic/Arabic/Devanagari/emoji), but
    // NOT the wider ASCII punctuation set the continuation class allows —
    // that keeps an unquoted attribute like `data-x=assets/a.webp` from
    // matching starting at the `=` (which would glue a spurious leading byte
    // onto the extracted tail and break the suffix comparison the other
    // direction). `%` IS in the start class, and that is load-bearing rather
    // than cosmetic: moss percent-encodes every non-ASCII segment, so a cover
    // under a CJK directory emits `/%E7%8D%8E%E9%A0%85/%E5%B0%81%E9%9D%A2.w800.webp`
    // — every byte before the extension sits inside an escape. Without `%`
    // the earliest legal start is the `E` of `%E7`, the extracted tail decodes
    // to garbage, no suffix equals the manifest key, and the live reference
    // reads as an orphan. That is the production incident this module's
    // invariant 6 exists to forbid, arriving through the *encoded* path rather
    // than the raw one; it survived the first fix and was caught by
    // `tests/staged_html_manifest_parity.rs`.
    // Case-insensitive on the extension alternation as defense
    // in depth — moss itself always emits lowercase, but nothing enforces
    // that on a plugin-authored reference.
    // Two spellings of the same class, differing only in whether `'` ends a
    // token. See the discussion above: each alone has a false negative the
    // other does not, and the union of the two has neither.
    const TOKEN_PATTERNS: [&str; 2] = [
        r#"[A-Za-z0-9_%\x{80}-\x{10FFFF}][^\s"'<>():]*\.(?i:webp|png|jpe?g|gif|svg|avif)"#,
        r#"[A-Za-z0-9_%\x{80}-\x{10FFFF}][^\s"<>():]*\.(?i:webp|png|jpe?g|gif|svg|avif)"#,
    ];
    let token_res: Vec<regex::Regex> = TOKEN_PATTERNS
        .iter()
        .map(|p| regex::Regex::new(p).expect("static regex"))
        .collect();

    let mut referenced = HashSet::new();
    let (files, mut unreadable) = walk_files(stage_dir);
    for path in files {
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        // Verified not a blind spot: `to_ascii_lowercase()` runs BEFORE the
        // comparison, so `index.HTML` lowercases to "html" and matches the
        // all-lowercase SCAN_EXTENSIONS list same as `index.html` would. A
        // version of this check that compared `ext` against SCAN_EXTENSIONS
        // without lowercasing first would silently skip an uppercase
        // extension — that is not what this line does.
        if !SCAN_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()) {
            continue;
        }
        // Two failures `read_to_string` used to conflate, kept apart because
        // they mean opposite things for pruning. An I/O error is the scan's
        // own blindness — the file may hold references it will never see — so
        // it is reported and the caller declines to delete. Bytes that are not
        // UTF-8 are not an I/O fault: nothing moss emits into a scannable
        // extension is binary, so there is no reference to miss, and the file
        // is skipped as before.
        let text = match std::fs::read(&path) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(_) => continue, // not text — nothing to extract
            },
            Err(_) => {
                unreadable.push(path);
                continue;
            }
        };
        // Once per file, not once per token: the scanned file's own
        // staging-relative path is what the second reading resolves against.
        let doc = path
            .strip_prefix(stage_dir)
            .ok()
            .and_then(|r| r.to_str())
            .map(|r| r.replace('\\', "/"));
        for m in token_res.iter().flat_map(|re| re.find_iter(&text)) {
            // The token charset excludes `:`, so an absolute URL's scheme
            // (`https:`) can never be PART of a match — but the regex still
            // finds a match starting right after "//", e.g. the
            // `example.com/x.webp` tail of `https://example.com/x.webp`.
            // That tail's suffixes would otherwise collide with a same-named
            // local file's key. Reject any match immediately preceded by
            // `//` (covers `https://`, `http://`, and protocol-relative
            // `//`) so an external URL can never falsely protect a local
            // orphan.
            if text.as_bytes()[..m.start()].ends_with(b"//") {
                continue;
            }
            let Some(decoded) = decode_reference(m.as_str()) else {
                continue;
            };
            // Two readings of the same token, unioned, because neither alone
            // is complete and invariant 6 only permits erring wide.
            //
            // SUFFIXES of the token cover a reference written DEEPER than the
            // key — a page at `a/b/c/` writing `../../../assets/x.webp` — and
            // a data file listing site-root-relative keys without a leading
            // `/`.
            //
            // RESOLUTION against the file the token was found in covers the
            // opposite direction, which suffixes structurally cannot: a
            // reference written SHALLOWER than the key. That is the ordinary
            // shape for every non-HTML scanned type, whose URLs are relative
            // to their own file — `gallery/style.css` with
            // `url(assets/x.webp)` means `gallery/assets/x.webp`, and by
            // suffixes alone it reads as `assets/x.webp`, matches no key, and
            // the live reference is pruned. Before moss#1085 that was a
            // deletion the next build undid; now the verdict is persisted and
            // `suppressed_variants` reads it back through this same set, so
            // the variant would stay missing while a stylesheet still asks
            // for it. `resolve_to_root_relative` is the canonical resolver
            // (it already backs frontmatter `cover:`), so this is one call,
            // not a second implementation.
            referenced.extend(path_suffixes(&strip_relative_prefixes(&decoded)));
            if let Some(doc) = doc.as_deref() {
                let resolved =
                    crate::build::markdown::html_post::resolve_to_root_relative(&decoded, doc);
                if !resolved.is_empty() {
                    referenced.insert(resolved);
                }
            }
        }
    }
    ReferenceScan { tails: referenced, unreadable }
}

/// The `.webp` variants a producer must NOT put back into `stage_dir`.
///
/// ONE authority, three readers. The ship-time prune is the only pass in the
/// build that knows what "referenced" means with full information — it runs
/// after every plugin, notebook and feed has written into staging — and its
/// verdict is persisted as `SiteHashes::pruned_image_outputs`. This function
/// hands that verdict to the two producers that run EARLIER and would
/// otherwise each derive their own answer from the disk scan: the image
/// encoder in `run_image_conversion`, and the fingerprint-skip self-heal in
/// `dispatch_image_conversions`.
///
/// Before moss#1085 both derived it themselves, from `scan.rs`'s walk of every
/// image file under the vault, so on any site holding an unreferenced image
/// the producers and the prune disagreed by construction and each build
/// re-created exactly what the previous build had deleted. Measured on
/// harbor 2026-08-19: sixteen consecutive builds, each logging `self-heal:
/// re-materialized 96 staged .webp file(s)` and then `orphan prune: removed 96
/// unreferenced .webp file(s), 3553600 bytes freed` — identical counts,
/// identical bytes, work that could never settle.
///
/// Suppression takes nothing away. Every key in `previous_pruned` is ALREADY
/// absent from staging, because the prune deleted it; declining to re-make it
/// cannot strand a reference that would otherwise have resolved. That is what
/// makes reading a carried verdict safe here even though this call site cannot
/// see the whole staging tree yet.
///
/// The one thing it must still notice is a reference that appeared since, and
/// the scan below is the current build's staging tree — which by this point
/// already holds the rendered pages, the notebook output and whatever the
/// enhance hook wrote, all of which run before the media phase. The one thing
/// it can miss is a reference inside a file the background asset copy has not
/// finished writing: that worker runs CONCURRENTLY with the image worker
/// (`pipeline.rs`, workers 1 and 3 in the same `JoinSet`), so a `.css`/`.js`
/// copied verbatim from the vault may or may not be on disk yet. That is a
/// race with a bounded cost, not a hole: the ship-time prune sees the file,
/// declines to delete the key, and drops it from the verdict, so the next
/// build produces it again — one build of latency, never a permanent
/// deletion. Per invariant 6, the failure this must never produce is a false
/// NEGATIVE, and both paths out are biased against one.
pub fn suppressed_variants(
    stage_dir: &Path,
    previous_pruned: &HashSet<String>,
) -> HashSet<String> {
    if previous_pruned.is_empty() {
        return HashSet::new(); // cold vault / first build — nothing judged yet
    }
    let scan = extract_referenced_tails(stage_dir);
    if !scan.unreadable.is_empty() {
        // The scan is blind to part of the tree, so "no reference here" is not
        // an answer — suppress nothing and let the producers re-make every
        // carried key. Costs one build's encoding; the alternative withholds a
        // variant whose only reference may sit in the file that would not read.
        return HashSet::new();
    }
    previous_pruned
        .iter()
        .filter(|k| !scan.tails.contains(k.as_str()))
        .cloned()
        .collect()
}

/// Every `.webp` key in `image_outputs` whose normalized path is not among
/// `referenced_tails`.
///
/// Pure set arithmetic, and deliberately so: the keys it returns leave the
/// sealed manifest, and `ship_phase` copies `sealed.files()` and nothing else,
/// so dropping the entry is already the whole of "this variant does not ship".
/// This used to unlink the staged file here as well, which is a write into the
/// directory the preview server is reading — see the sweep in
/// `build::pipeline`, which does the unlinking at the next build's start.
pub fn orphaned_webp_keys(
    image_outputs: &HashSet<String>,
    referenced_tails: &HashSet<String>,
) -> HashSet<String> {
    image_outputs
        .iter()
        // `.png`/`.jpg` are the raster fallback tier — see module docs.
        .filter(|key| key.ends_with(".webp"))
        .filter(|key| !referenced_tails.contains(key.as_str()))
        .cloned()
        .collect()
}

/// Decode an extracted token to a path via the shared HTML-reference
/// normalizer ([`moss_core::resolve::fuzzy_path::decode_html_reference_path`]),
/// which every pass deriving an asset key from emitted HTML must use so their
/// keys cannot disagree. Returns `None` for anything that is not a same-site
/// path (absolute URL, protocol-relative, `data:`, `mailto:`).
fn decode_reference(raw: &str) -> Option<String> {
    if raw.starts_with("http:")
        || raw.starts_with("https:")
        || raw.starts_with("//")
        || raw.starts_with("data:")
        || raw.starts_with("mailto:")
    {
        return None;
    }
    let path_only = moss_core::resolve::fuzzy_path::decode_html_reference_path(raw);
    if path_only.is_empty() {
        None
    } else {
        Some(path_only)
    }
}

/// Drop a leading `/` and any run of `./` / `../`, leaving the tail whose
/// suffixes are compared against manifest keys.
fn strip_relative_prefixes(path: &str) -> String {
    let mut s = path.strip_prefix('/').unwrap_or(path);
    loop {
        if let Some(rest) = s.strip_prefix("../") {
            s = rest;
        } else if let Some(rest) = s.strip_prefix("./") {
            s = rest;
        } else {
            break;
        }
    }
    s.to_string()
}

/// All path-component suffixes of `path`, most-specific first:
/// `"a/b/c.webp"` → `["a/b/c.webp", "b/c.webp", "c.webp"]`.
///
/// The matcher only needs membership, so the caller flattens these into one
/// `HashSet` rather than caring about order — this is what makes a reference
/// written at a different relative-`../` depth than the manifest's
/// site-root-relative key still resolve as a match.
fn path_suffixes(path: &str) -> impl Iterator<Item = String> + '_ {
    let parts: Vec<&str> = path.split('/').collect();
    (0..parts.len()).map(move |i| parts[i..].join("/"))
}

/// Every file under `root`, plus every directory the walk could not open.
///
/// An unopenable directory hides a whole subtree of possible references, so it
/// is the same blindness as an unreadable file and travels out the same way —
/// see [`ReferenceScan`].
fn walk_files(root: &Path) -> (Vec<std::path::PathBuf>, Vec<std::path::PathBuf>) {
    let mut out = Vec::new();
    let mut failed = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            failed.push(dir);
            continue;
        };
        for entry in rd.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    (out, failed)
}

#[cfg(test)]
#[path = "orphan_prune_tests.rs"]
mod tests;
