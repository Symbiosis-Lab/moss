//! Typed URL resolution: walk a [`Document`] and classify every
//! [`Url::Unresolved`] into a [`Url::Resolved`] with the right [`UrlKind`].
//!
//! One typed visitor replaces the two line-level "Stage 1" regex passes it was
//! migrated from (`markdown_refs` for bare-filename image refs,
//! `markdown_links` for standard `[text](url)` links), both deleted after
//! parity was proven — history in `docs/archive/2026-05-28-*`. Two properties
//! come from the AST rather than from code here: `Inline::Image::src` (always
//! an asset URL) is structurally distinct from `Inline::Link::url` (may be a
//! markdown target), and code is never visited by [`visit_urls_mut`], so no
//! fence tracking is needed.
//!
//! `resolve_link_urls` emits a `moss-resolved:<path>` sentinel and leaves the
//! URL `Url::Unresolved` so src-tauri's `classify_url_prod` can apply
//! `page_map` / `external_url_map` / wikilink-class-aware decoding. That
//! sentinel IS the moss-core ↔ src-tauri layering seam: moss-core resolves
//! filesystem paths, src-tauri owns the deployed URL space.
//!
//! ## OutgoingLink contract
//!
//! [`UrlResolution::outgoing`] carries the same load-bearing shape (target_path,
//! link_type, document-order sequence) the deleted passes produced. It uses
//! parsed inline text for `display_text` where they used the raw source between
//! `[` and `]`; `display_text` has no production consumer, so the divergence is
//! non-breaking (see `link_wrapping_image_target_path`).

use super::document::Document;
use super::node::{Block, Inline};
use super::shortcode::Shortcode;
use super::url::{ResolvedUrl, Url, UrlKind};
use super::visit::visit_urls_mut;
use crate::content_graph::ContentGraph;
use crate::resolve::asset_class::{resolve_asset_ref, AssetIndex, AssetResolution};
use crate::resolve::fuzzy_path::{resolve_reference, ResolvedRef};
use crate::resolve::{Diagnostic, DiagnosticKind, LinkType, OutgoingLink};

// ---------------------------------------------------------------------------
// GraphAssetIndex: adapts ContentGraph to the AssetIndex trait so the pure
// engine (resolve_asset_ref) can run against a real content graph.
// ---------------------------------------------------------------------------

/// Adapts [`ContentGraph`] to the [`AssetIndex`] trait.
///
/// Wraps a borrowed `ContentGraph` so that `resolve_asset_ref` (the pure
/// shared engine in `moss_core::resolve::asset_class`) can be driven by the
/// build-time in-memory index — identical to how `FsAssetIndex` in src-tauri
/// drives it from the live filesystem. Exposed `pub` so integration tests and
/// editor↔build parity tests can construct both adapters over the same file set.
pub struct GraphAssetIndex<'a>(pub &'a ContentGraph);

impl<'a> AssetIndex for GraphAssetIndex<'a> {
    fn contains(&self, p: &str) -> bool {
        self.0.asset_contains(p)
    }
    fn contains_ci(&self, p: &str) -> Option<String> {
        self.0.asset_contains_ci(p)
    }
    fn find_by_suffix(&self, s: &str) -> Vec<String> {
        self.0.asset_find_by_suffix(s)
    }
}

/// What one `resolve_urls` walk learned: the dependency edges the build needs,
/// and the references it could not resolve. A miss produces a diagnostic and
/// keeps the author's bytes — never a guessed URL (moss#903 bug 3). The host
/// surfaces diagnostics; moss-core does no I/O.
#[derive(Debug, Default)]
pub struct UrlResolution {
    pub outgoing: Vec<OutgoingLink>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Walk every URL in `doc` and classify it into [`Url::Resolved`].
///
/// Returns the [`UrlResolution`] for the walk: the dependency edges discovered
/// plus a diagnostic per unresolvable reference.
///
/// # Arguments
///
/// * `doc` — the typed document. Every [`Url::Unresolved`] is replaced in
///   place with a [`Url::Resolved`]. URLs that are already [`Url::Resolved`]
///   are left untouched (idempotent on a resolved document).
/// * `graph` — the content graph for bare-filename / cross-page lookups.
/// * `source_path` — the file containing the URLs, used by
///   [`resolve_reference`] for relative-path disambiguation. It does NOT enter
///   the emitted URL (see [`ContentGraph::pinned_url`]).
pub fn resolve_urls(
    doc: &mut Document,
    graph: &ContentGraph,
    source_path: &str,
) -> UrlResolution {
    // Phase 1: walk asset URLs (image refs) and accumulate their
    // OutgoingLink entries. This pass replaces the deleted Stage 1
    // `resolve::markdown_refs::resolve_markdown_refs` — it only touches
    // asset URLs and produces OutgoingLink for resolved bare-filename
    // images. The companion AST visitor lives at `resolve_image_urls`
    // below.
    let mut found = UrlResolution::default();
    resolve_image_urls(doc, graph, source_path, &mut found);

    // Phase 2: walk link URLs and accumulate their OutgoingLink entries.
    // Replaces the deleted Stage 1
    // `resolve::markdown_links::resolve_markdown_links` — only touches
    // link URLs and produces OutgoingLink for resolved cross-page links.
    // The AST visitor lives at `resolve_link_urls` below.
    //
    // Two-pass ordering matches Stage 1's resolve.rs sequence (refs first,
    // then links). The image-URL display_text comes from alt; the link-URL
    // display_text comes from the link text. Each phase appends to the
    // shared `outgoing` Vec in document order.
    resolve_link_urls(doc, graph, source_path, &mut found);

    // Phase 3 (NOT done by default): the renderer's invariant requires
    // every URL be `Url::Resolved` at HTML emission time. For non-graph
    // URLs the visitor left as `Url::Unresolved` (resolver-prefixed,
    // anchors that fell through, edge cases), the caller is responsible
    // for one more classification pass before rendering. Callers that
    // need a complete classification can call
    // [`classify_remaining_urls`] explicitly. The src-tauri host pipeline
    // chains a second `visit_urls_mut` to apply its `classify_url_prod`
    // for page_map-aware decoding of the three sentinel prefixes.

    found
}

// ---------------------------------------------------------------------------
// Phase 1: image (Inline::Image::src) URL resolution
// ---------------------------------------------------------------------------

/// Walk every `Inline::Image::src` URL and resolve bare-filename references.
///
/// Replaces the deleted Stage 1 `resolve::markdown_refs::resolve_markdown_refs`
/// (Phase 4 PR7a, 2026-05-28). Contract:
/// - Only touches Inline::Image::src URLs (not Link URLs).
/// - Bare filename + has-extension + no-path-separators → graph lookup.
/// - On `Found`: rewrite to relative asset path; push OutgoingLink.
/// - On `Unresolved`: leave URL as author-input (mark resolved-as-asset so
///   the renderer accepts it).
/// - Pipe-bearing URLs pass through unchanged (Phase 3 PR3 contract).
/// - External / data / mailto / anchor / explicit-relative pass through.
fn resolve_image_urls(
    doc: &mut Document,
    graph: &ContentGraph,
    source_path: &str,
    found: &mut UrlResolution,
) {
    walk_inline_images_mut(doc, &mut |inline| {
        let (src, alt) = match inline {
            Inline::Image { src, alt, .. } => (src, alt.clone()),
            _ => return,
        };
        resolve_asset_url(src, &alt, graph, source_path, found);
    });
    // Hero/Gallery shortcodes carry image URLs as typed fields on the
    // shortcode args (not as `Inline::Image`). Walk those structural
    // URLs through the same bare-filename resolver so wikilink targets
    // like `![[hero.jpg]]` resolve to `assets/hero.jpg` against the
    // graph, mirroring the `Inline::Image` path. Regression fix for
    // the chps-site home hero (2026-05-29): the previous skip left
    // `args.image` as `Url::Unresolved("hero.jpg")` → the renderer
    // emitted `<img src="hero.jpg">` instead of the depth-correct
    // `assets/hero.jpg`.
    for block in &mut doc.blocks {
        resolve_shortcode_image_urls(block, graph, source_path, found);
    }
}

/// Resolve one image-kind `Url` field against the content graph.
///
/// Extracted from `resolve_image_urls`'s per-`Inline::Image` body so the
/// same logic can apply to structural image URLs that live on shortcode
/// args (Hero, Gallery). Behavior:
/// - Already `Url::Resolved` → no-op.
/// - Pipe-bearing → pass through verbatim (Phase 3 PR3 contract).
/// - External, anchor, data URLs → pass through (engine returns NotFound for
///   these, so they fall through to the verbatim passthrough arm).
/// - Separator-bearing, bare filename, or `/`-absolute → routed through
///   [`resolve_asset_ref`] (the unified engine). On `Resolved` / `Ambiguous`:
///   emit the target's pinned URL and push an OutgoingLink. On `NotFound`: keep
///   the author's bytes and record a diagnostic — the build never hard-fails on
///   an unresolved asset ref, and never invents a path either.
///
/// Every resolved href is [`ContentGraph::pinned_url`], so the referencing
/// page's depth and the target folder's case are structurally out of the answer
/// (moss#903 bug 3: the same embed emitted a working URL from the vault root and
/// a broken one from a note two folders down, because the href was computed
/// relative to the referencing file).
fn resolve_asset_url(
    url: &mut Url,
    alt: &str,
    graph: &ContentGraph,
    source_path: &str,
    found: &mut UrlResolution,
) {
    let raw = match url {
        Url::Unresolved(s) => s.clone(),
        Url::Resolved(_) => return,
    };

    // Pipe-bearing URLs pass through unchanged (Phase 3 PR3): authors
    // use `![[file.jpg|attrs]]` for typed params; pipe in standard
    // markdown URL is literal and intentionally 404s.
    if raw.contains('|') {
        *url = Url::Resolved(ResolvedUrl::new(raw, UrlKind::Asset));
        return;
    }

    // External, anchor, and data URLs are not asset filesystem references.
    // Pass them through before invoking the engine (which only understands
    // filesystem paths) so we don't misinterpret `https://...` as a path.
    if raw.starts_with('#')
        || raw.starts_with("http://")
        || raw.starts_with("https://")
        || raw.starts_with("//")
        || raw.starts_with("data:")
        || raw.starts_with("mailto:")
    {
        *url = Url::Resolved(ResolvedUrl::new(raw, UrlKind::Asset));
        return;
    }

    // Route ALL remaining refs (bare filenames, separator paths, absolute
    // `/…` paths) through the unified asset engine. This replaces BOTH the
    // old `is_bare_filename` branch (which called `resolve_reference`) and
    // the old passthrough branch (which emitted the verbatim separator path,
    // causing 404s for cross-directory relative paths).
    //
    // NOTE: provenance (SeparatorFallback / CaseMismatch / Ambiguous) is
    // intentionally NOT logged here — moss-core is the pure, side-effect-free
    // kernel (no `log`/I/O). The advisory author-facing warning is surfaced by
    // the editor adapter (`editor::asset_resolver`) via the `@codemirror/lint`
    // hover tooltip. The build's job here is only to emit a correct URL; a
    // build-time console warning is a deferred follow-up (would require
    // surfacing provenance to the src-tauri build layer).
    match resolve_asset_ref(&raw, source_path, &GraphAssetIndex(graph)) {
        AssetResolution::Resolved { root_rel, provenance: _ } => {
            pin_asset_url(url, root_rel, alt, graph, found);
        }
        AssetResolution::Ambiguous { chosen, candidates: _ } => {
            pin_asset_url(url, chosen, alt, graph, found);
        }
        AssetResolution::NotFound => {
            // Keep the author's bytes (an unresolved asset ref never fails the
            // build) and say so. Synthesizing a plausible-looking path is what
            // shipped a 404 that looked like a working link.
            found.diagnostics.push(Diagnostic {
                message: format!("Unresolved asset reference: {raw}"),
                source_path: source_path.to_string(),
                reference: raw.clone(),
                // The one blocking kind. `raw` is the author's literal
                // spelling, which is the string they will search for.
                kind: DiagnosticKind::MissingAsset,
            });
            *url = Url::Resolved(ResolvedUrl::new(raw, UrlKind::Asset));
        }
    }
}

/// Emit `root_rel`'s pinned URL and record the dependency edge — the one place a
/// resolved asset reference becomes an href. Neither the referencing page nor
/// the authored spelling of the reference reaches the emitted URL.
fn pin_asset_url(
    url: &mut Url,
    root_rel: String,
    alt: &str,
    graph: &ContentGraph,
    found: &mut UrlResolution,
) {
    let pinned = graph.pinned_url(&root_rel);
    found.outgoing.push(OutgoingLink {
        target_path: root_rel,
        display_text: alt.to_string(),
        link_type: LinkType::Standard,
    });
    *url = Url::Resolved(ResolvedUrl::new(pinned, UrlKind::Asset));
}

/// Recursively descend into shortcode-bearing blocks and resolve any
/// structural image `Url` fields (HeroShortcode::image, GalleryItem::src).
/// Container shortcodes (Grid, Hero overlay) may nest other shortcodes —
/// recurse through their inner blocks. Skips Inline::Image-bearing
/// blocks because the `walk_inline_images_mut` pass above already
/// handled them.
fn resolve_shortcode_image_urls(
    block: &mut Block,
    graph: &ContentGraph,
    source_path: &str,
    found: &mut UrlResolution,
) {
    match block {
        Block::Shortcode(sc) => match sc {
            Shortcode::Hero(args) => {
                if let Some(image_url) = args.image.as_mut() {
                    resolve_asset_url(image_url, "", graph, source_path, found);
                }
                // Multi-image hero: every extra slide resolves exactly like
                // the primary — skipping this re-creates the 2026-05-29
                // chps-site regression (raw filenames at depth) per slide.
                for image_url in &mut args.extra_images {
                    resolve_asset_url(image_url, "", graph, source_path, found);
                }
                // Overlay may itself contain shortcodes (e.g. `::::buttons`
                // inside `:::hero`); recurse so any nested Hero/Gallery
                // structural image URLs resolve too.
                for nested in &mut args.overlay {
                    resolve_shortcode_image_urls(nested, graph, source_path, found);
                }
            }
            Shortcode::Gallery(args) => {
                for item in &mut args.items {
                    let alt = item.alt.clone();
                    resolve_asset_url(&mut item.src, &alt, graph, source_path, found);
                }
            }
            Shortcode::Grid(args) => {
                for cell in &mut args.cells {
                    for nested in cell {
                        resolve_shortcode_image_urls(nested, graph, source_path, found);
                    }
                }
            }
            Shortcode::Subscribe(_) | Shortcode::Buttons(_) | Shortcode::Recent(_) | Shortcode::Apply(_) => {}
        },
        // Container blocks: recurse so nested shortcodes (Hero inside a
        // Callout, Grid inside a list, etc.) are reached.
        Block::Callout { children, .. }
        | Block::BlockQuote(children)
        | Block::FootnoteDefinition { children, .. } => {
            for nested in children {
                resolve_shortcode_image_urls(nested, graph, source_path, found);
            }
        }
        Block::List { items, .. } => {
            for item_blocks in items {
                for nested in item_blocks {
                    resolve_shortcode_image_urls(nested, graph, source_path, found);
                }
            }
        }
        Block::LinkCard { children, .. } => {
            for nested in children {
                resolve_shortcode_image_urls(nested, graph, source_path, found);
            }
        }
        // Leaf / inline-only blocks: nothing structural to resolve here.
        Block::Heading { .. }
        | Block::Paragraph(_)
        | Block::Table { .. }
        | Block::Figure { .. }
        | Block::CodeBlock { .. }
        | Block::ThematicBreak
        | Block::Other(_) => {}
    }
}

// ---------------------------------------------------------------------------
// Phase 2: link (Inline::Link::url + Block::LinkCard::url) URL resolution
// ---------------------------------------------------------------------------

/// Walk every link URL (Inline::Link::url, Block::LinkCard::url) and
/// resolve markdown / asset targets via the content graph.
///
/// Mirrors `markdown_links::resolve_markdown_links`:
/// - Only touches Link URLs (image URLs were handled in phase 1).
/// - Resolvable targets (not external / not anchor / not protocol /
///   not absolute-path / not already-prefixed) → graph lookup.
/// - On `Found`: classify into Internal (markdown) / Asset (binary) and
///   push OutgoingLink with target_path = resolved path.
/// - On `Unresolved`: leave URL author-input; Stage 1 emitted a diagnostic
///   here, but PR6 mirrors the byte-equivalence contract (no diagnostic in
///   the OutgoingLink Vec since Diagnostic is a separate stream).
/// - Anchor / mailto / tel / external pass through with the matching
///   UrlKind so the renderer attaches the right attributes.
fn resolve_link_urls(
    doc: &mut Document,
    graph: &ContentGraph,
    source_path: &str,
    found: &mut UrlResolution,
) {
    walk_links_mut(doc, &mut |link_url, display_text, is_wikilink| {
        let raw = match link_url {
            Url::Unresolved(s) => s.clone(),
            Url::Resolved(_) => return,
        };

        // Author-facing short-circuits: classify and stop.
        if let Some(rest) = raw.strip_prefix("mailto:") {
            *link_url = Url::Resolved(ResolvedUrl::new(format!("mailto:{rest}"), UrlKind::Mailto));
            return;
        }
        if let Some(rest) = raw.strip_prefix("tel:") {
            *link_url = Url::Resolved(ResolvedUrl::new(format!("tel:{rest}"), UrlKind::Tel));
            return;
        }
        if raw.starts_with('#') {
            // Same-page anchor. For wikilinks (`[[#Heading]]`) slug the
            // fragment so it matches the rendered heading id; markdown
            // anchors (`[x](#frag)`) stay raw (literal author-supplied id).
            let href = if is_wikilink {
                slug_wikilink_suffix(&raw)
            } else {
                raw
            };
            *link_url = Url::Resolved(ResolvedUrl::new(href, UrlKind::Anchor));
            return;
        }
        if raw.starts_with("http://")
            || raw.starts_with("https://")
            || raw.starts_with("//")
            || raw.starts_with("data:")
        {
            *link_url = Url::Resolved(ResolvedUrl::new(raw, UrlKind::External));
            return;
        }

        // Stage 1 carry-over: URLs already prefixed with a resolver
        // sentinel (`moss-resolved:`, `moss-newtab:`, `wikilink:`) carry
        // Stage 1 / upstream state the visitor cannot decode in isolation
        // — the final pretty URL depends on the host's `page_map`, which
        // lives in src-tauri's pipeline context. Leave these as
        // `Url::Unresolved` so the host's per-URL classifier
        // (`classify_url_prod` in src-tauri's pipeline) can apply the
        // page_map-aware decoding. This preserves the byte-equivalence
        // contract (no OutgoingLink emitted for already-resolved targets
        // — Stage 1 already counted them) while letting the host close
        // the prefix-decoding loop.
        if raw.starts_with("moss-resolved:")
            || raw.starts_with("moss-newtab:")
            || raw.starts_with("wikilink:")
        {
            // Leave Unresolved; host pass classifies.
            return;
        }

        // Absolute filesystem path — treat as opaque. Mirrors
        // markdown_links: `if url.starts_with('/') { return false; }`.
        if raw.starts_with('/') {
            *link_url = Url::Resolved(ResolvedUrl::new(raw, UrlKind::Internal));
            return;
        }

        // Resolvable: split query/fragment, look up the path against the
        // content graph, push OutgoingLink, emit the `moss-resolved:`
        // sentinel for the host classifier. Mirrors
        // markdown_links::rewrite_line byte-for-byte: same sentinel shape
        // (`moss-resolved:<path>[<suffix>]`), same suffix concatenation.
        //
        // Phase 4 PR7a-stage1b (2026-05-28): moss-core resolves the
        // filesystem path; src-tauri's `classify_url_prod` decodes the
        // sentinel into the final pretty / external / asset URL using
        // `page_map`, `external_url_map`, and the wikilink-class signal.
        // The sentinel IS the moss-core ↔ src-tauri layering seam — the
        // visitor must NOT collapse it to a final `Url::Resolved` or
        // page_map decoding silently breaks.
        let (path_part, suffix) = split_path_suffix(&raw);
        match resolve_reference(path_part, graph, source_path) {
            ResolvedRef::Found(resolved) => {
                found.outgoing.push(OutgoingLink {
                    target_path: resolved.clone(),
                    display_text: display_text.to_string(),
                    link_type: LinkType::Standard,
                });
                // For wikilinks, slug the `#fragment` so the emitted href
                // matches the rendered heading id. The `?query` portion (if
                // any) is preserved by `slug_wikilink_suffix`. Markdown links
                // keep their suffix raw — a literal author-supplied URL.
                let sentinel = match suffix {
                    Some(s) => {
                        let s = if is_wikilink {
                            slug_wikilink_suffix(s)
                        } else {
                            s.to_string()
                        };
                        format!("moss-resolved:{}{}", resolved, s)
                    }
                    None => format!("moss-resolved:{}", resolved),
                };
                *link_url = Url::Unresolved(sentinel);
            }
            ResolvedRef::Unresolved => {
                // Mirrors Stage 1: leave the URL as authored — no
                // `moss-resolved:` prefix, no synthesized target. Internal so
                // the renderer's `Url::Resolved` invariant holds.
                found.diagnostics.push(Diagnostic {
                    message: format!("Unresolved link target: {raw}"),
                    source_path: source_path.to_string(),
                    reference: raw.clone(),
                    kind: DiagnosticKind::Other,
                });
                *link_url = Url::Resolved(ResolvedUrl::new(raw, UrlKind::Internal));
            }
        }
    });
}

/// Split a URL into (path, suffix) where `suffix` is `?query` and/or
/// `#fragment` in source order. Suffix is opaque — round-trip parity with
/// `crate::build::markdown::pipeline::classify_url_prod` (the src-tauri
/// decoder) is the contract; this function must not reorder, normalize,
/// or escape the suffix bytes.
///
/// The parallel src-tauri implementation lives at
/// `src-tauri/src/build/markdown/pipeline.rs::split_path_suffix` and must
/// share this exact shape.
fn split_path_suffix(url: &str) -> (&str, Option<&str>) {
    let q = url.find('?');
    let h = url.find('#');
    let cut = match (q, h) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };
    match cut {
        #[allow(clippy::string_slice)]
        Some(pos) => (&url[..pos], Some(&url[pos..])),
        None => (url, None),
    }
}

/// Slug the `#fragment` of a wikilink `suffix` so the emitted href matches
/// the rendered heading id (`obsidian_heading_anchor`). Only the fragment is
/// transformed: any leading `?query` is preserved verbatim. Block refs
/// (`#^id`) keep their id raw (minus the caret), mirroring
/// [`crate::resolve::wikilink_dispatch`]'s `build_anchor`. This must ONLY be
/// called for wikilinks (`is_wikilink: true`); regular markdown links keep
/// their fragment raw (it is a literal URL — `#L42`, hand-authored ids, etc.).
///
/// `suffix` is the value returned by [`split_path_suffix`] — it begins with
/// `?` or `#`. Shapes handled:
/// - `#frag` → `#<slug>`
/// - `?query` → `?query` (no fragment, untouched)
/// - `?query#frag` → `?query#<slug>` (query verbatim, fragment slugged)
///
/// `pub` (ADR-036 stage 2): `src-tauri`'s `newsletter.rs` has no content
/// graph to resolve wikilinks through — it just needs this same fragment
/// half — so it calls this directly instead of carrying its own copy of the
/// block-ref-vs-heading-anchor branch.
pub fn slug_wikilink_suffix(suffix: &str) -> String {
    use crate::heading::anchor::obsidian_heading_anchor;

    // Find the fragment (`#…`); everything before it is a `?query` we leave
    // untouched. There is at most one `#` in a well-formed suffix.
    match suffix.find('#') {
        None => suffix.to_string(), // query-only (or empty) — nothing to slug
        Some(h) => {
            #[allow(clippy::string_slice)]
            let (head, frag_with_hash) = (&suffix[..h], &suffix[h + 1..]);
            let slugged = if let Some(block_id) = frag_with_hash.strip_prefix('^') {
                // Block ref: keep the id raw (caret stripped). Matches build_anchor.
                block_id.to_string()
            } else {
                obsidian_heading_anchor(frag_with_hash)
            };
            format!("{head}#{slugged}")
        }
    }
}

// ---------------------------------------------------------------------------
// Phase 3: ensure no Url::Unresolved survives
// ---------------------------------------------------------------------------

/// Classify any URL left as `Url::Unresolved` after phases 1 + 2 into a
/// best-effort `Url::Resolved`. The renderer's invariant requires no
/// `Url::Unresolved` reaches HTML emission; this is the safety net that
/// catches URLs the per-kind phases didn't visit (e.g., a future
/// `Inline::Link` variant added before its phase-2 arm is wired).
///
/// Callers that follow [`resolve_urls`] with their own per-URL classifier
/// (e.g., src-tauri's pipeline calling `classify_url_prod` for
/// resolver-prefix decoding) should NOT call this — let the secondary
/// classifier handle the remaining URLs. Callers that have no secondary
/// pass should call this to maintain the render invariant.
pub fn classify_remaining_urls(doc: &mut Document) {
    visit_urls_mut(doc, |url| {
        if let Url::Unresolved(raw) = url {
            // Conservative fallback: treat as External (opens in new tab,
            // no graph lookup). Unknown URLs are external by nature;
            // guessing Internal would be wrong and new-tab is safe.
            let kind = classify_unresolved_kind(raw);
            let raw_owned = std::mem::take(raw);
            *url = Url::Resolved(ResolvedUrl::new(raw_owned, kind));
        }
    });
}

/// Best-effort kind classification for an Unresolved URL that escaped the
/// per-kind phases. Mirrors the prefix-based detection in
/// `pipeline::classify_url_prod` for consistency.
fn classify_unresolved_kind(raw: &str) -> UrlKind {
    if raw.starts_with("mailto:") {
        UrlKind::Mailto
    } else if raw.starts_with("tel:") {
        UrlKind::Tel
    } else if raw.starts_with('#') {
        UrlKind::Anchor
    } else if raw.starts_with("http://")
        || raw.starts_with("https://")
        || raw.starts_with("//")
        || raw.starts_with("data:")
    {
        UrlKind::External
    } else {
        UrlKind::Internal
    }
}

// ---------------------------------------------------------------------------
// Per-kind walkers (image-only / link-only)
// ---------------------------------------------------------------------------

/// Walk every `Inline::Image` in the document and invoke `f` with a `&mut`
/// reference to the inline. Used by phase 1 — separates image src
/// classification from link URL classification.
fn walk_inline_images_mut<F>(doc: &mut Document, f: &mut F)
where
    F: FnMut(&mut Inline),
{
    for block in &mut doc.blocks {
        walk_images_in_block(block, f);
    }
}

fn walk_images_in_block<F>(block: &mut Block, f: &mut F)
where
    F: FnMut(&mut Inline),
{
    match block {
        Block::Heading { children, .. } | Block::Paragraph(children) => {
            for inline in children {
                walk_images_in_inline(inline, f);
            }
        }
        Block::Callout { children, .. }
        | Block::BlockQuote(children)
        | Block::FootnoteDefinition { children, .. } => {
            for nested in children {
                walk_images_in_block(nested, f);
            }
        }
        Block::List { items, .. } => {
            for item_blocks in items {
                for nested in item_blocks {
                    walk_images_in_block(nested, f);
                }
            }
        }
        Block::Table { header, rows, .. } => {
            for cell in header {
                for inline in cell {
                    walk_images_in_inline(inline, f);
                }
            }
            for row in rows {
                for cell in row {
                    for inline in cell {
                        walk_images_in_inline(inline, f);
                    }
                }
            }
        }
        Block::Shortcode(sc) => {
            walk_images_in_shortcode(sc, f);
        }
        Block::Figure { image, caption, .. } => {
            walk_images_in_inline(image, f);
            if let Some(cap) = caption {
                for inline in cap {
                    walk_images_in_inline(inline, f);
                }
            }
        }
        Block::LinkCard { children, .. } => {
            for nested in children {
                walk_images_in_block(nested, f);
            }
        }
        Block::CodeBlock { .. } | Block::ThematicBreak | Block::Other(_) => {}
    }
}

fn walk_images_in_shortcode<F>(sc: &mut Shortcode, f: &mut F)
where
    F: FnMut(&mut Inline),
{
    match sc {
        Shortcode::Subscribe(_) | Shortcode::Buttons(_) | Shortcode::Recent(_) | Shortcode::Apply(_) => {}
        Shortcode::Gallery(args) => {
            // Gallery items carry their src as a structural `Url` on
            // GalleryItem, not as an `Inline::Image`. The Inline-image
            // walker has nothing to do here; the structural URL is
            // resolved by `resolve_shortcode_image_urls` instead.
            let _ = args;
        }
        Shortcode::Hero(args) => {
            // Hero's image is a structural `Url` field, not an
            // `Inline::Image` — resolved by `resolve_shortcode_image_urls`.
            // The overlay blocks may still contain `Inline::Image`s
            // (e.g. inside markdown paragraphs); descend so those reach
            // the inline walker.
            for block in &mut args.overlay {
                walk_images_in_block(block, f);
            }
        }
        Shortcode::Grid(args) => {
            for cell_blocks in &mut args.cells {
                for block in cell_blocks {
                    walk_images_in_block(block, f);
                }
            }
        }
    }
}

fn walk_images_in_inline<F>(inline: &mut Inline, f: &mut F)
where
    F: FnMut(&mut Inline),
{
    match inline {
        Inline::Image { .. } => {
            f(inline);
        }
        Inline::Link { children, .. } => {
            for nested in children {
                walk_images_in_inline(nested, f);
            }
        }
        Inline::Emphasis(children) | Inline::Strong(children) | Inline::Strikethrough(children) => {
            for nested in children {
                walk_images_in_inline(nested, f);
            }
        }
        Inline::Text(_)
        | Inline::Code(_)
        | Inline::LineBreak
        | Inline::FootnoteRef(_)
        | Inline::TaskMarker(_)
        | Inline::Other(_) => {}
    }
}

/// Walk every link URL in the document (Inline::Link::url +
/// Block::LinkCard::url) and invoke `f` with `(&mut Url, display_text)`.
///
/// The `display_text` is the link text (concatenated from the Link's
/// children) — needed for the OutgoingLink::display_text contract.
fn walk_links_mut<F>(doc: &mut Document, f: &mut F)
where
    F: FnMut(&mut Url, &str, bool),
{
    for block in &mut doc.blocks {
        walk_links_in_block(block, f);
    }
}

fn walk_links_in_block<F>(block: &mut Block, f: &mut F)
where
    F: FnMut(&mut Url, &str, bool),
{
    match block {
        Block::Heading { children, .. } | Block::Paragraph(children) => {
            for inline in children {
                walk_links_in_inline(inline, f);
            }
        }
        Block::Callout { children, .. }
        | Block::BlockQuote(children)
        | Block::FootnoteDefinition { children, .. } => {
            for nested in children {
                walk_links_in_block(nested, f);
            }
        }
        Block::List { items, .. } => {
            for item_blocks in items {
                for nested in item_blocks {
                    walk_links_in_block(nested, f);
                }
            }
        }
        Block::Table { header, rows, .. } => {
            for cell in header {
                for inline in cell {
                    walk_links_in_inline(inline, f);
                }
            }
            for row in rows {
                for cell in row {
                    for inline in cell {
                        walk_links_in_inline(inline, f);
                    }
                }
            }
        }
        Block::Shortcode(sc) => {
            walk_links_in_shortcode(sc, f);
        }
        Block::Figure { caption, .. } => {
            if let Some(cap) = caption {
                for inline in cap {
                    walk_links_in_inline(inline, f);
                }
            }
        }
        Block::LinkCard { url, children } => {
            // Compound-link card: the wrapping href is a link URL. Use
            // the inner text content as display_text by recursively
            // gathering it from the children (best-effort — empty string
            // if no text is found).
            // LinkCard wrapping href is never a wikilink (it's a compound
            // markdown link card), so pass is_wikilink=false.
            let display = gather_text_blocks(children);
            f(url, &display, false);
            for nested in children {
                walk_links_in_block(nested, f);
            }
        }
        Block::CodeBlock { .. } | Block::ThematicBreak | Block::Other(_) => {}
    }
}

fn walk_links_in_shortcode<F>(sc: &mut Shortcode, f: &mut F)
where
    F: FnMut(&mut Url, &str, bool),
{
    match sc {
        Shortcode::Subscribe(_) | Shortcode::Recent(_) | Shortcode::Apply(_) => {}
        Shortcode::Buttons(args) => {
            for item in &mut args.items {
                // ButtonItem display text comes from item.text per the
                // shortcode shape (crates/moss-core/src/ast/shortcode.rs).
                // Button URLs are authored markdown targets, not wikilinks.
                let text = item.text.clone();
                f(&mut item.url, &text, false);
            }
        }
        Shortcode::Gallery(_) => {
            // Gallery items use src URLs (image-kind), not link URLs.
            // No link-walk action.
        }
        Shortcode::Hero(args) => {
            for block in &mut args.overlay {
                walk_links_in_block(block, f);
            }
        }
        Shortcode::Grid(args) => {
            for cell_blocks in &mut args.cells {
                for block in cell_blocks {
                    walk_links_in_block(block, f);
                }
            }
        }
    }
}

fn walk_links_in_inline<F>(inline: &mut Inline, f: &mut F)
where
    F: FnMut(&mut Url, &str, bool),
{
    match inline {
        Inline::Link {
            url,
            children,
            is_wikilink,
            ..
        } => {
            // display_text = concatenated plain text of the children.
            // Matches markdown_links::rewrite_line, which uses the raw
            // text between `[` and `]` (no rendering, just the literal).
            let display = gather_text_inlines(children);
            f(url, &display, *is_wikilink);
            // Descend so nested Links (rare in CommonMark but possible
            // via parser quirks) get visited too.
            for nested in children {
                walk_links_in_inline(nested, f);
            }
        }
        Inline::Image { .. } => {
            // Image src is a Url but it's image-kind — handled by phase 1.
        }
        Inline::Emphasis(children) | Inline::Strong(children) | Inline::Strikethrough(children) => {
            for nested in children {
                walk_links_in_inline(nested, f);
            }
        }
        Inline::Text(_)
        | Inline::Code(_)
        | Inline::LineBreak
        | Inline::FootnoteRef(_)
        | Inline::TaskMarker(_)
        | Inline::Other(_) => {}
    }
}

/// Concatenate the plain-text content of a list of inlines, mirroring
/// pulldown-cmark's behavior of treating link text as a verbatim string.
/// Used to populate `OutgoingLink::display_text`.
fn gather_text_inlines(inlines: &[Inline]) -> String {
    let mut s = String::new();
    for inline in inlines {
        gather_text_inline(inline, &mut s);
    }
    s
}

fn gather_text_inline(inline: &Inline, out: &mut String) {
    match inline {
        Inline::Text(t) => out.push_str(t),
        Inline::Code(c) => out.push_str(c),
        Inline::Emphasis(children) | Inline::Strong(children) | Inline::Strikethrough(children) => {
            for nested in children {
                gather_text_inline(nested, out);
            }
        }
        Inline::Link { children, .. } => {
            for nested in children {
                gather_text_inline(nested, out);
            }
        }
        Inline::Image { alt, .. } => out.push_str(alt),
        Inline::LineBreak => out.push('\n'),
        // A marker is a pointer, not prose: it must not leak into a
        // description, a slug, or a numeric-column probe. A task checkbox is
        // the same — `- [x] Ship it` should summarize as "Ship it".
        Inline::FootnoteRef(_) | Inline::TaskMarker(_) | Inline::Other(_) => {}
    }
}

/// Concatenate the plain-text content of a list of blocks. Used by
/// Block::LinkCard arm to populate the OutgoingLink::display_text.
fn gather_text_blocks(blocks: &[Block]) -> String {
    let mut s = String::new();
    for block in blocks {
        gather_text_block(block, &mut s);
    }
    s
}

fn gather_text_block(block: &Block, out: &mut String) {
    match block {
        Block::Heading { children, .. } | Block::Paragraph(children) => {
            for inline in children {
                gather_text_inline(inline, out);
            }
        }
        Block::Figure { image, caption, .. } => {
            if let Inline::Image { alt, .. } = image {
                out.push_str(alt);
            }
            if let Some(cap) = caption {
                for inline in cap {
                    gather_text_inline(inline, out);
                }
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "resolve_urls_tests.rs"]
mod tests;
