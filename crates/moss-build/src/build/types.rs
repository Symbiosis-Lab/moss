//! Build-local types: parsed document model, series metadata, and source file metadata.
//!
//! These types are consumed exclusively within the build pipeline and are co-located
//! here per the codebase restructure plan (docs/archive/2026-04-24-codebase-restructure-continuation-plan.md Task 5).
//!
//! Consumers import these directly from `crate::build::types` — the old
//! `crate::types` back-compat re-exports were removed 2026-08-12 (M5a landing 3).

use serde::{Deserialize, Serialize};
use specta::Type;

// SeriesField moved to moss-core (ADR-018). Re-exported for backward compat.
pub use moss_core::frontmatter_typed::SeriesField;

/// One media reference that resolves to nothing.
///
/// Broken is a fact about the SOURCE, and this is the build's verdict on it:
/// the render pass keeps the resolver's own `MissingAsset` diagnostics and
/// hands them out on `SiteBuildResult`, so a reference with no file behind it
/// travels with the build that found it rather than being re-derived by a
/// second scan that is free to disagree. An asset that is merely still
/// encoding has a source file and never appears here. The publish gate and the
/// command that shows the author which files to fix are the app half, in
/// `crate::missing_media`.
///
/// Both fields are the author's own strings, not resolved paths — `reference`
/// especially, because it is what they will search their document for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct MissingMedia {
    /// Markdown file holding the reference, relative to the site root.
    pub source_path: String,
    /// The reference exactly as the author typed it.
    pub reference: String,
}

impl MissingMedia {
    /// Every blocking reference in a run of resolve diagnostics.
    ///
    /// `DiagnosticKind::MissingAsset` is the one kind a publish is refused
    /// over; everything else in the same stream is advisory and logged.
    pub fn from_diagnostics<'a>(
        diagnostics: impl IntoIterator<Item = &'a moss_core::resolve::Diagnostic>,
    ) -> Vec<Self> {
        diagnostics
            .into_iter()
            .filter(|d| d.kind == moss_core::resolve::DiagnosticKind::MissingAsset)
            .map(|d| MissingMedia {
                source_path: d.source_path.clone(),
                reference: d.reference.clone(),
            })
            .collect()
    }
}

/// Parsed markdown document with frontmatter and content.
///
/// Represents a processed markdown file ready for HTML generation.
/// Enhanced data model following Jekyll/Hugo patterns for consistent site generation.
/// References:
/// - https://jekyllrb.com/docs/variables/
/// - https://gohugo.io/variables/page/
/// Most fields are `Option<T>`, `String`, or `Vec<T>` whose `Default` is
/// well-defined. Use `..Default::default()` in tests and synthesizers that
/// only care about a subset of fields, instead of spelling every field
/// every time. The single canonical full-literal constructor lives in
/// `pipeline::process_markdown_file` — that's where missing fields should
/// fail compilation when a new field is added (forcing a real decision).
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct ParsedDocument {
    /// Document title.
    ///
    /// For ARTICLE pages: always equals `label` (frontmatter `title:` if non-
    /// empty, else filename) — per `docs/reference/title-rendering.md`.
    ///
    /// For INDEX/folder pages: frontmatter `title:` if non-empty, else the
    /// filename/folder name (`filename_text`). NEVER sourced from body content
    /// (Obsidian-match, 2026-05-30). A no-`title:` index.md therefore feeds
    /// `homepage_title` with the folder name (or, for the root home where the
    /// stem is an index-stem, `homepage_title` is filtered to None and
    /// `LayoutConfig::site_name` falls back to the folder name).
    pub title: String,
    /// Raw markdown content without frontmatter
    pub content: String,
    /// HTML content generated from markdown
    pub html_content: String,
    /// Relative URL path for the generated page
    pub url_path: String,
    /// Publication date if specified in frontmatter
    pub date: Option<String>,
    /// Estimated reading time in minutes
    pub reading_time: u32,
    /// URL-safe slug identifier (auto-generated from title/filename)
    /// Following Hugo slug conventions
    pub slug: String,
    /// Complete URL with depth-aware path generation
    /// Following Jekyll permalink patterns
    pub permalink: String,
    /// Plain-text label used in site chrome (nav, breadcrumb, cards, tabs, RSS).
    /// Never rendered as a heading — that's body H1's job.
    /// Source priority: frontmatter `title` → title-cased filename
    /// (folder name for index/self-named notes). Never sourced from body content.
    pub label: String,
    /// Navigation weight for ordering (lower numbers = higher priority)
    pub weight: Option<i32>,
    /// Analytics configuration for privacy-focused analytics
    pub analytics: Option<crate::build::markdown::AnalyticsConfig>,
    /// Site logo image path (from homepage frontmatter, rendered in nav)
    #[serde(skip)]
    #[specta(skip)]
    pub logo: Option<String>,
    /// Cover image URL for collections
    pub cover: Option<String>,
    /// Explicit cover type override ("video", "iframe", or "image")
    pub cover_type: Option<String>,
    /// Media items marked for collection pages (photography, video, interactive)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[specta(skip)]
    pub media_items: Vec<crate::build::media_collection::MediaItem>,
    /// Whether to show in navigation
    pub nav: Option<bool>,
    /// Whether source file is at root level (no '/' in path)
    /// Used for auto-navigation: root-level non-index files default to nav
    pub is_root_level: bool,
    /// True iff this document is its folder's home via the `home: true`
    /// frontmatter marker rather than its filename — i.e. it won the home
    /// slot in [`crate::build::scan::page_map::compute_home_overrides`]
    /// (issue #587). This is the ONE centralized home-override signal: the
    /// markdown pipeline derives it once (from the page_map URL shape) and
    /// every consumer reads it from here instead of re-deriving from
    /// `translation_key == "home"`. Render code uses it to suppress the
    /// folder-title `<h1>` on a language home (e.g. `en/Liu Guo.md` →
    /// `en/index.html`), which is the language-specific site root, not a
    /// generic folder listing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[specta(skip)]
    pub is_home_override: bool,
    /// Draft: rendered and published at its direct URL, but hidden from all
    /// listings, feeds, sitemap, and navigation (and marked `noindex`).
    pub draft: Option<bool>,
    /// Listed: when `Some(false)`, hidden from moss's auto-generated index
    /// surfaces (feeds/lists/sitemap) via `is_listable()`, but still a public
    /// page — indexable, share-cardable, reachable (see `is_public_page`).
    /// `None` ⇒ listed. Orthogonal to `draft` (which adds `noindex` + drops
    /// the share card).
    pub listed: Option<bool>,
    /// When set, this page's rendered HTML is emitted into the named slot
    /// instead of (or in addition to) being a page.
    ///
    /// Recognized values: see `crate::build::slots::Slot`. Author-targetable:
    /// `"footer-left"`. Other values produce a build warning and are no-op.
    ///
    /// Authors who use the reserved filename `footer.md` at site root do
    /// NOT need this field — the slot is inferred from the filename
    /// (auto-targets `footer-left`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
    /// True if this document fills a layout slot rather than rendering as a
    /// standalone page (e.g., `footer.md`). Mirrors
    /// [`moss_core::ast::Document::slot_only`] on the production type so
    /// page-emission, native-slot collection, and auto-H1 injection can
    /// branch on a single structural flag.
    ///
    /// Set at parse time by `process_markdown_file` from the reserved-name
    /// convention (`crate::build::footer::is_excluded_from_pages`). Authors
    /// may target additional slots via the `slot:` frontmatter field above,
    /// but reserved-name detection is the canonical signal.
    ///
    /// Lands in PR7b of the typed-AST migration (moss#599); replaces the
    /// filesystem-scan + on-demand re-render at `build/footer.rs::render_footer_pages_from_disk`
    /// (deleted in the same PR) by letting `footer.md` flow through the
    /// normal parse pipeline with this flag set, and the page-emission
    /// loop in `build/render/blocking.rs` skip it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[specta(skip)]
    pub slot_only: bool,
    /// Page description for SEO and list previews
    pub description: Option<String>,
    /// Byline rows from frontmatter `byline:` — the credit lines a reader
    /// sees under the page title (article or folder index alike), already
    /// normalized to one row per line
    /// by `moss_core::frontmatter_union::normalize_byline`. Empty when the
    /// field is absent. Rendered by `build::render::byline`; nothing else
    /// reads it, and it is deliberately absent from JSON-LD and RSS (a
    /// display string carries no reliable role information — see the field's
    /// doc comment in `frontmatter_typed.rs`).
    #[serde(skip)]
    #[specta(skip)]
    pub byline: Vec<String>,
    /// Colophon rows from frontmatter `colophon:` — the same credit rows as
    /// `byline`, rendered at the foot of the page instead of the head.
    /// Same normalization, same "display only, never structured data" rule.
    #[serde(skip)]
    #[specta(skip)]
    pub colophon: Vec<String>,
    /// Content tags for organization
    pub tags: Option<Vec<String>>,
    /// The `tags:` frontmatter list BEFORE inline `#hashtags` merged in.
    /// Only this list derives tag term pages (`build::terms`): a frontmatter
    /// tag is deliberate cataloguing; an inline hashtag is prose, and
    /// deriving pages from prose shipped junk pages on the first real vault.
    /// `tags` (the merged set) stays the source for article:tag metadata.
    #[serde(skip)]
    #[specta(skip)]
    pub fm_tags: Option<Vec<String>>,
    /// Author names from frontmatter `author:`, already normalized to one
    /// name per entry (a single string stays one verbatim entry) by
    /// `moss_core::frontmatter_union::normalize_name_list`. Empty when the
    /// field is absent. Feeds the term derivation (`build::terms`) and the
    /// byline author-link pass; unlike `byline` this IS a machine claim.
    #[serde(skip)]
    #[specta(skip)]
    pub author: Vec<String>,
    /// Term-page claim from `author_page:` — this page is the author page
    /// for the claimed name (see `moss_core::terms::TermClaim`). Resolved by
    /// `build::terms::derive_terms` into [`Self::term_listing`] on the
    /// winning claimer; the render layer reads that, never this.
    #[serde(skip)]
    #[specta(skip)]
    pub author_page: Option<moss_core::terms::TermClaim>,
    /// Term-page claim from `tag_page:` — same as `author_page`, `tags/` namespace.
    #[serde(skip)]
    #[specta(skip)]
    pub tag_page: Option<moss_core::terms::TermClaim>,
    /// The term pseudo-folder key (`authors/<slug>`, `tags/<slug>`) whose
    /// member listing this page hosts. Set by `build::terms::derive_terms`
    /// on the page that WINS a term claim — and only when the author didn't
    /// route `children` explicitly (their routing wins) — so `render/html.rs`
    /// and the ADR-044 listing digest read one resolved field instead of
    /// re-running claim resolution.
    #[serde(skip)]
    #[specta(skip)]
    pub term_listing: Option<String>,
    /// Whether to render child pages below content.
    /// Accepts bool or wikilink in frontmatter (e.g. `children: "[[News]]"`).
    /// true = render children, false = hide, None = default to true.
    pub children: Option<bool>,
    /// Wikilink reference for targeted children (e.g. "[[News]]").
    /// When set, renders the target folder's articles inline instead of
    /// the page's own direct children.
    pub children_source: Option<String>,
    /// Wikilink to folder whose children appear in sidebar (e.g. "[[News]]")
    pub sidebar: Option<String>,
    /// How child pages are rendered: "list" (default), "card".
    ///
    /// Wrapped in [`moss_core::Resolved`] so the renderer can tell
    /// author intent (frontmatter / cascade) apart from auto-detected
    /// defaults. Author intent survives axis-driven overrides.
    pub children_style: Option<moss_core::Resolved<String>>,
    /// How children are grouped: "year" (default for list), "none" (default for card).
    ///
    /// Wrapped in [`moss_core::Resolved`] so the renderer can tell
    /// author intent apart from auto-detected defaults — explicit
    /// `children_group: year` survives a non-Date sort axis (the
    /// liu-guo regression). Only auto-detected values are overridden.
    pub children_group: Option<moss_core::Resolved<String>>,
    /// What children to include: "direct" (default), "all" descendants
    pub children_depth: Option<String>,
    /// Where to render the children feed: "body" (default) or "sidebar".
    pub children_in: Option<String>,
    /// Cap the children feed at N items. Truncation appends "More →".
    pub children_limit: Option<u32>,
    /// Wikilink to a page the truncated "More →" link should point at,
    /// instead of the listing's own folder — the folder_embed self-listing
    /// suppression (a "More →" back to the page you're already on) is
    /// pointless otherwise.
    pub children_more: Option<String>,
    /// Listing filter: "only" keeps pages that have a cover. Applied after
    /// flattening and before `children_limit`. A folder entry is covered
    /// when ITS OWN index page (the same `ParsedDocument`) has a cover —
    /// no special-casing needed, `cover` is already that page's field.
    pub children_covers: Option<String>,
    /// Internal: marks documents whose `children_*` fields came from the deprecated
    /// `sidebar:` alias. Sidebar callsite uses this to apply the legacy default-3
    /// limit on cross-ref. Skip-serialize so it never round-trips into stored frontmatter.
    #[serde(skip_serializing, default)]
    pub from_sidebar_alias: Option<bool>,
    /// Series declaration: bool for weight-based, Vec<String> for explicit wikilink order
    pub series: Option<SeriesField>,
    /// Override site-wide breadcrumb setting
    pub breadcrumb: Option<bool>,
    /// Override site-wide footer setting
    pub footer: Option<bool>,
    /// Footer alignment: "left" (default) or "right"
    #[serde(default)]
    pub footer_align: Option<String>,
    /// Frontmatter values to cascade to all descendants
    ///
    /// `BTreeMap`, not `HashMap`: this field is part of the `Debug` string
    /// `PageFacade` hashes (moss#922) for the incremental-build facade
    /// diff. `HashMap`'s per-process-random hasher makes its `Debug`
    /// iteration order — and thus the facade hash — differ between build
    /// invocations for byte-identical content; `BTreeMap` iterates in
    /// sorted-key order regardless of process.
    #[serde(skip)]
    #[specta(skip)]
    pub cascade: Option<std::collections::BTreeMap<String, serde_yaml::Value>>,
    /// Folder paths where this article also appears in lists
    pub also_in: Option<Vec<String>>,
    /// Whether to show comments on this page (default: true)
    pub comments: Option<bool>,
    /// Document language (resolved from frontmatter > filename suffix > auto-detect > site default)
    #[serde(skip)]
    #[specta(skip)]
    pub lang: crate::i18n::Language,
    /// The document's own BCP-47 tag when it declares a language moss
    /// recognizes but has no interface for (`ja`, `fr`, `de`, …).
    ///
    /// `None` — the common case — means the document's language is one of the
    /// three [`crate::i18n::Language`] variants, whose canonical tag comes from
    /// `Language::as_bcp47_attr()`. This field exists because `lang` above
    /// answers "which interface strings" and `<html lang>` asks "what language
    /// is this text", and those diverge for a `ja/` tree: the chrome is
    /// honestly the site default's, but the content is Japanese and used to say
    /// otherwise (#977). Set by `i18n::declared_lang_tag`.
    #[serde(skip)]
    #[specta(skip)]
    pub lang_tag: Option<String>,
    /// Clean filename stem without language suffix (for translation linking)
    #[serde(skip)]
    #[specta(skip)]
    pub clean_stem: String,
    /// Translation key for linking arbitrary files as translations
    #[serde(skip)]
    #[specta(skip)]
    pub translation_key: Option<String>,
    /// Links to translated versions of this document
    #[serde(skip)]
    #[specta(skip)]
    pub translations: Vec<crate::i18n::link::TranslationLink>,
    /// Hero section HTML (extracted from :::hero block)
    /// Placed outside <main> at template level for full-width display
    #[serde(skip)]
    #[specta(skip)]
    pub hero_html: Option<String>,
    /// Hero image URL — populated at hoisting time from `HeroShortcode.image`
    /// (post-resolution). `None` when the hero had no `image=` attribute
    /// (text-only overlay) or the page has no hero. Feeds the homepage-hero
    /// cover-cascade rung in `cover.rs`. Invariant: never re-derive from
    /// `hero_html`; `None` means "the hero genuinely had no image."
    #[serde(skip)]
    #[specta(skip)]
    pub hero_image_url: Option<String>,
    /// First markdown-origin `![]()` image in the document body
    /// (post-resolution, ready to use as `<img src>`). Captured at
    /// parse time by `transform_events`. Feeds the body-image rung of
    /// `cover.rs::resolve_cover_chain` so the cover fallback no longer
    /// requires a regex post-pass over rendered HTML (Step 5 of the
    /// structural-html-emission migration; see
    /// `docs/reference/structural-html-emission.md`).
    ///
    /// Intentionally excludes raw HTML `<img>` tags embedded in markdown
    /// source (pulldown-cmark passes them as `Event::Html`, not
    /// `Tag::Image` — they're a documented carve-out) and shortcode-
    /// emitted images (`:::hero` etc., which have their own cascade
    /// rungs). Authors who want a raw `<img>` or a hero image as cover
    /// should use frontmatter `cover:` or the hero cascade.
    #[serde(skip)]
    #[specta(skip)]
    pub body_cover_path: Option<String>,
    /// Hero overlay text — the opening prose of `HeroShortcode.overlay_markdown`
    /// after running through `meta::extract_description`: its first paragraph,
    /// or that paragraph plus the next where the first is only a fragment (an
    /// unpunctuated headline over a subhead reads as one line here). `None`
    /// when the overlay was empty or contained only headings/shortcodes/code-blocks.
    /// Feeds the description fallback chain in `meta.rs`. Invariant: same
    /// extractor as the body-paragraph rung; never re-extract from `hero_html`.
    #[serde(skip)]
    #[specta(skip)]
    pub hero_overlay_text: Option<String>,
    /// Per-page feature-presence flags. See `PageFeatures`.
    #[serde(skip)]
    #[specta(skip)]
    pub features: PageFeatures,
    /// Kind of page this document represents.
    ///
    /// Set once at ingestion (for markdown files, in `markdown::process_markdown_file`)
    /// or at synthesis time (for per-asset pages, in `render.rs`). All downstream
    /// code classifies pages by reading this field — do not infer kind from filename,
    /// extension, or other fields.
    ///
    /// See `moss/docs/reference/page-kinds.md`.
    #[serde(skip)]
    #[specta(skip)]
    pub kind: moss_core::PageKind,
    /// Stable note identity: 8 random hex characters, minted once on first
    /// build and written back to frontmatter. It is NOT derived from the path
    /// or the content — a lost uid cannot be recomputed, and it is the join key
    /// for the note's comment thread and its signed moderation events.
    pub uid: Option<String>,
    /// Typesetting direction: "horizontal" or "vertical" (from frontmatter)
    #[serde(skip)]
    #[specta(skip)]
    pub typesetting: Option<String>,
    /// Content width preset: "wide" or "full" (from frontmatter)
    #[serde(skip)]
    #[specta(skip)]
    pub content_width: Option<String>,
    /// Template layout override: "page" or "article" (from frontmatter)
    #[serde(skip)]
    #[specta(skip)]
    pub layout: Option<String>,
    /// Original relative file path (e.g. "友链.md") for filesystem fallbacks like creation date
    #[serde(skip)]
    #[specta(skip)]
    pub source_path: Option<String>,
    /// Custom URL override from frontmatter (preserved for cascade resolution)
    #[serde(skip)]
    #[specta(skip)]
    pub url_override: Option<String>,
    /// Raw frontmatter as generic key-value pairs (preserves all fields including plugin-specific ones like `syndicated`)
    ///
    /// `BTreeMap`, not `HashMap` — see the doc comment on `cascade` above
    /// for why (moss#922 `PageFacade` determinism).
    #[serde(skip)]
    #[specta(skip)]
    pub raw_frontmatter: std::collections::BTreeMap<String, serde_json::Value>,
    /// Declared listing sort from frontmatter (folder index pages only;
    /// `None` on article pages).
    pub sort: Option<moss_core::sort::SortField>,
    /// Scan-time axis inference over the folder's DIRECT children only
    /// (not flattened descendants). Renderers must NOT read this field
    /// directly — use [`ParsedDocument::resolve_for_direct_children`]
    /// (cache hit) or [`ParsedDocument::resolve_for_flatten`]
    /// (re-computes on the flattened scope) to get the right value for
    /// what's about to be iterated.
    ///
    /// Populated by `build::scan::sort_inference::populate_direct_children_sorts`.
    pub(crate) direct_children_sort: Option<moss_core::sort::ResolvedSort>,
    /// `html_content` in the pieces the serializer emitted it in, plus the
    /// typed grid cells behind them (ADR-034).
    ///
    /// Rendering happens per page, before any page knows about the others, so
    /// two structural decisions have to wait for whole-build state: which grid
    /// cells link to a collection, and where a cover-bearing folder home
    /// releases its narrow column. The render phase makes them on the typed
    /// `Block`s recorded here instead of re-parsing `html_content` — the
    /// scanning that produced moss#903's char-boundary abort.
    ///
    /// `None` for documents synthesized outside `process_markdown_file`
    /// (tests, generated index pages); those fall back to `html_content`
    /// treated as one opaque segment, which is exactly the pre-ADR-034
    /// behavior minus the grid/cover enhancements they never needed.
    ///
    /// Not serialized: it is derived build state, and the frontend has no use
    /// for a second copy of the body.
    #[serde(skip)]
    #[specta(skip)]
    pub body_plan: Option<super::markdown::body_plan::BodyPlan>,
    /// Every wikilink/embed/markdown-link target this document resolved
    /// during parsing (`markdown/pipeline.rs`'s `outgoing_links`, both the
    /// wikilink-dispatch pass and the content-graph URL-resolve pass).
    /// `OutgoingLink.link_type` already discriminates plain links from
    /// `![[embed]]` transclusions (`LinkType::Embed`) — there is no separate
    /// "embed_deps" field to duplicate that split.
    ///
    /// Not read by anything yet (moss#922 Stage 3 — the future `DepGraph`,
    /// Stage 4, is the first consumer). Not serialized: it's derived build
    /// state the frontend has no use for, same rationale as `body_plan`.
    #[serde(skip)]
    #[specta(skip)]
    pub outgoing_links: Vec<moss_core::resolve::OutgoingLink>,
    /// `(target, immediate_embedder)` pairs for every `![[transclusion]]` the
    /// resolve phase spliced into this document's markdown, transitively
    /// (moss#922 Stage 7).
    ///
    /// Verbatim `ResolveResult::embed_deps` (`moss_core::resolve`). NOT the
    /// same relation as `outgoing_links`' `LinkType::Embed` entries: a
    /// transclusion is lowered to an HTML comment and replaced with the
    /// target's raw bytes *before* the AST dispatcher that populates
    /// `outgoing_links` runs, so the only page→page embed edges in the build
    /// are these.
    ///
    /// The second element is the file the marker was found IN, which for a
    /// nested chain is an intermediate file, not this document — see
    /// [`moss_core::dep_graph::DepGraph::with_embed_pairs`]. Consumers must go
    /// through `DepGraph::embed_closure`, never filter these pairs on this
    /// document's own path.
    ///
    /// Not serialized: derived build state, same rationale as `outgoing_links`.
    #[serde(skip)]
    #[specta(skip)]
    pub embed_deps: Vec<(String, String)>,
    /// Every media reference in this document that resolves to no file.
    ///
    /// The `DiagnosticKind::MissingAsset` diagnostics raised while this
    /// document was parsed — by the AST URL pass for `![alt](gone.png)`, by
    /// the wikilink dispatcher for `![[gone.jpg]]`. Carried on the document
    /// because both run inside `process_markdown_file`, out of reach of the
    /// render loop that assembles the build's verdict.
    ///
    /// Not serialized: derived build state, same rationale as `embed_deps`.
    /// The build's own copy — `SiteBuildResult::missing_media` — is what the
    /// publish gate reads.
    #[serde(skip)]
    #[specta(skip)]
    pub missing_media: Vec<MissingMedia>,
}

impl ParsedDocument {
    /// Rewrite the page body, keeping the typed plan and the flattened
    /// `html_content` in sync.
    ///
    /// `html_content` IS [`Self::body_plan`] flattened (ADR-034), and the render
    /// phase renders from the plan. So a pass that edited only the string would
    /// be silently discarded at render time, and a pass that edited only the
    /// plan would be invisible to the ~38 places that read the bytes (RSS,
    /// excerpts, search, syndication). Every body rewrite between parse and
    /// render goes through here so the two shapes cannot drift.
    ///
    /// `f` must rewrite within single elements (attribute values, a self-closing
    /// marker comment): it is applied per segment, so it cannot see across a
    /// top-level block boundary. Nothing that needs the whole body belongs in a
    /// string pass at all — it belongs in the render phase, on typed blocks.
    pub(crate) fn rewrite_body(&mut self, f: &dyn Fn(&str) -> String) {
        match &mut self.body_plan {
            Some(plan) => {
                plan.map_html(f);
                self.html_content = plan.to_html();
            }
            None => self.html_content = f(&self.html_content),
        }
    }

    /// Cache hit: return the scan-time inference over the folder's direct
    /// children. Use this when the renderer iterates direct children
    /// (`children_depth: direct`, or folder index with shallow listing,
    /// or series-nav over direct-sibling pages).
    ///
    /// Fallback axis is `Date` (matches the default-when-uncertain
    /// elsewhere in this module). This fallback is reached in two
    /// situations:
    ///
    /// 1. Test paths that construct `ParsedDocument` by hand without
    ///    seeding the cache.
    /// 2. Production lookup paths where `sort_source_doc` resolves to a
    ///    synthetic auto-index folder — those are constructed inline in
    ///    `render/blocking.rs` (around line 1084) AFTER
    ///    `populate_direct_children_sorts` has run, so their cache is
    ///    intentionally `None`. The dedicated auto-index render loop
    ///    (`render/blocking.rs:1331+`) calls `generate_children` with
    ///    `parent_sort_axis: None` for the same reason, and this
    ///    helper's Date fallback matches that convention.
    ///
    /// If a caller needs a different fallback (e.g. series-chrome
    /// historically wanted Title), it must set up the cache explicitly.
    pub fn resolve_for_direct_children(&self) -> moss_core::sort::ResolvedSort {
        self.direct_children_sort.clone().unwrap_or(moss_core::sort::ResolvedSort {
            axis: moss_core::sort::SortAxis::Date,
            explicit_order: None,
            series_default: false,
        })
    }

    /// Re-compute axis on a flattened render scope. Use this when the
    /// renderer flattens descendants (`children_depth: all`): the scan
    /// cache only reflects direct-children inference and misrepresents
    /// the corpus the reader sees.
    pub fn resolve_for_flatten(&self, scope: &[&ParsedDocument]) -> moss_core::sort::ResolvedSort {
        moss_core::sort::resolve_folder_sort(self, scope)
    }

    /// Whether this document appears on moss's auto-generated *listing*
    /// surfaces — page lists, folder embeds, year-grouped article lists,
    /// home/folder children, auto-index pages, RSS, llms.txt, sitemap, and the
    /// auto sidebar/sibling listings. False when **any** exclusion fires:
    ///
    /// 1. `draft == Some(true)` — work-in-progress: rendered and reachable at
    ///    its direct URL, but hidden from all listings and (via
    ///    `is_public_page`) marked `noindex` with no share card. (Supersedes
    ///    the former `unlisted` flag, removed 2026-06.)
    /// 2. `listed == Some(false)` — a finished page curated *off-feed*: hidden
    ///    from these listing surfaces but still a public page — indexable,
    ///    share-cardable, reachable (see `is_public_page`).
    /// 3. `slot_only == true` — layout chrome (e.g. `footer.md`) that fills a
    ///    layout slot rather than rendering as a page. Structurally detected by
    ///    `crate::build::footer::is_excluded_from_pages`; see
    ///    `ParsedDocument::slot_only` and PR7b (moss#599).
    ///
    /// All list-emitting sites call this helper so any future visibility flag
    /// is added in one place.
    ///
    /// **NOT** to be used for: nav/footer placement (explicit `nav:`/`footer:`,
    /// independent of this flag); page-emission gating (drafts and off-feed
    /// pages both render — emission only branches on `slot_only`); or
    /// `noindex`/share-card gating (use `is_public_page`).
    pub fn is_listable(&self) -> bool {
        self.draft != Some(true) && self.listed != Some(false) && !self.slot_only
    }

    /// Whether this is a real reader-facing page: indexable (no `noindex`) and
    /// eligible for an og/twitter share card + JSON-LD + QR + hover preview.
    /// Independent of *listing* membership — a `listed: false` page is still a
    /// public page. Only `draft` (work-in-progress) and `slot_only` (layout
    /// chrome) are excluded. Gates per-page public affordances, where
    /// `is_listable` gates multi-page listing surfaces.
    pub fn is_public_page(&self) -> bool {
        self.draft != Some(true) && !self.slot_only
    }
}

/// Per-page feature-presence flags.
///
/// Set during markdown processing as shortcodes/blocks fire. Aggregated by
/// the build feature pass to decide which feature-CSS modules and JS
/// bundles to embed. This is the prototype for the eventual `SiteFeatures`
/// registry described in `docs/reference/html-css-contract.md` (roadmap
/// step 8). When that lands, `PageFeatures` becomes a typed enum-set or
/// bitset; today it's a struct of `bool`s for legibility.
///
/// **Adding a new feature flag:** add a field here, set it where the feature
/// fires (typically a shortcode arm or markdown processor), and read it in
/// `build/features.rs` to gate CSS/JS injection. Do not add ad-hoc bools
/// elsewhere on `ParsedDocument` for this purpose.
#[derive(Debug, Clone, Default)]
pub struct PageFeatures {
    /// True if the page contains a `:::subscribe` inline shortcode block.
    /// When any page in a build has this set, the email feature pass injects
    /// `SUBSCRIBE_JS` + `email.css` even if `[channels.email]` is not
    /// configured.
    pub inline_subscribe: bool,
    /// True if the page contains a `:::apply` inline shortcode block.
    /// Causes the same `SUBSCRIBE_JS` + `email.css` injection as
    /// `inline_subscribe` (subscribe.ts hydrates both forms).
    pub inline_apply: bool,
    /// True if the page contains an Obsidian-style callout (`> [!note]`),
    /// nested ones included. Gates the `callouts` stylesheet partial via
    /// [`SiteAssets`]. Read from the typed AST
    /// (`moss_core::ast::has_callout_recursive`), never from emitted HTML.
    pub callouts: bool,
    /// True if the page defines at least one footnote (`[^label]` plus its
    /// `[^label]: ...` definition). Gates the sidenote runtime script via
    /// [`SiteAssets`]. Read from the typed AST
    /// (`moss_core::ast::footnotes::FootnoteIndex::build`), never from
    /// emitted HTML.
    pub footnotes: bool,
}

impl PageFeatures {
    /// Merge two feature sets, taking the OR of each flag. Used to aggregate
    /// per-page flags into a per-build summary.
    pub fn union(self, other: Self) -> Self {
        Self {
            inline_subscribe: self.inline_subscribe || other.inline_subscribe,
            inline_apply: self.inline_apply || other.inline_apply,
            callouts: self.callouts || other.callouts,
            footnotes: self.footnotes || other.footnotes,
        }
    }
}

/// Which optional asset partials this build ships — the site-level union of
/// every page's [`PageFeatures`] with the resolved site config.
///
/// **One asset set per build, never per page.** `emit::scripts`'
/// `shell_tags` records the reason for the JS half (a script set that
/// varies per page trips the preview morph-guard's `scriptsDiffer` full
/// reload); for CSS the same call plus content-hashed caching means one
/// `_moss/style.<hash>.css` shared by every page. A per-page stylesheet
/// would multiply the cache-key space by page count to save bytes it then
/// spends on cache misses.
///
/// Consumed by `build/emit/stylesheet.rs`, which decides from it which
/// partials to concatenate, and by `build/emit/scripts.rs`, which decides
/// from it which runtime `<script>` tags the shell carries. It reaches the
/// render paths as `LayoutConfig::assets`.
///
/// **Not** the key of the Stage 5b global invalidator. Changing the partial
/// set changes the stylesheet's content-hashed filename, which every already-
/// rendered page references — but so does editing `site.css` or `tokens.json`
/// or upgrading moss. That whole class is keyed on the emitted
/// `css_version`/`js_version` instead; see `FacadeCache::asset_versions`.
///
/// **Adding a partial:** add a field here, compute it in [`SiteAssets::of`],
/// add the `CssPartial` row in `build/emit/stylesheet.rs`, and drop the
/// stylesheet in `assets/css/site/`. The totality test there fails if the
/// row and the file disagree.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SiteAssets {
    /// Any page has a callout block.
    pub callouts: bool,
    /// Any page defines a footnote. Gates the sidenote runtime script — the
    /// one entry here that gates JS rather than a stylesheet partial, which
    /// is why it is named `has_footnotes` rather than `footnotes`: it reads
    /// at the `SITE_SCRIPTS` gate as a question, not as a partial name.
    pub has_footnotes: bool,
    /// `[site].link_preview` — hover link previews. Defaults ON.
    pub link_preview: bool,
    /// `[site].heading_anchors` — click-to-copy section permalinks. Defaults ON.
    pub heading_anchors: bool,
    /// `[site].math` — click-to-copy LaTeX on math nodes. Defaults ON.
    pub math: bool,
    /// This build generated at least one media collection page
    /// (`/photography`, `/videos`, `/experiments`), which is what
    /// `fullscreen.js` attaches to.
    pub media_pages: bool,
    /// The site ships the search overlay: `[site].search`, resolved once by the config phase
    /// (`LayoutConfig::with_search`) and read from `LayoutConfig::assets` by
    /// the nav button, the search-index emitter and `search.js`'s tag alike.
    /// All three must read this one value: a rawer one gives a reader a button
    /// — or a runtime — pointing at an index that was never written. Not a
    /// page fact:
    /// the overlay is site chrome, present on every page or none. Off by
    /// default, so most sites drop the whole 237-rule partial.
    pub search: bool,
    /// Any page renders in vertical typesetting — either the site default is
    /// `vertical` or some page's frontmatter sets it. Deliberately an
    /// over-approximation: a site defaulting to vertical whose every page
    /// overrides to horizontal still ships the partial, which costs bytes,
    /// where the reverse would cost a broken layout.
    pub vertical: bool,
    /// Some video on this site was encoded into an HLS ladder. Read from the
    /// asset registry rather than folded from `documents`: the registry is
    /// where the ladder's `<source>` also comes from, so the gate and the tag
    /// that needs it cannot disagree.
    pub video_ladder: bool,
}

impl SiteAssets {
    /// Compute the set from this build's parsed pages plus the site-level
    /// facts that are not page facts: the typesetting default
    /// (`LayoutConfig::typesetting`) and whether search is on.
    /// `from_config` carries the site-level flags — `link_preview`,
    /// `heading_anchors`, `math`, `media_pages`, `search` — which the caller
    /// sets **by name**. It is a `Self` rather than five `bool` parameters on
    /// purpose: five adjacent bools transpose silently at the call site and
    /// compile, and the failure mode is shipping `math-copy.js` on the flag
    /// that was meant to gate the search overlay. `callouts` and `vertical`
    /// are ignored — this function derives them from `pages` — so
    /// `..SiteAssets::default()` is the idiomatic tail.
    pub fn of(
        pages: &[ParsedDocument],
        site_typesetting: Option<&str>,
        from_config: Self,
    ) -> Self {
        // One pass, no per-page clone: `PageFeatures` is a handful of bools,
        // and this runs once per build over every page in the site.
        let mut out = Self {
            callouts: false,
            has_footnotes: false,
            vertical: site_typesetting == Some("vertical"),
            ..from_config
        };
        for page in pages {
            out.callouts |= page.features.callouts;
            out.vertical |= page.typesetting.as_deref() == Some("vertical");
            out.has_footnotes |= page.features.footnotes;
        }
        out
    }
}

/// Metadata for a source file, used for fast-path cache validation.
///
/// By storing file size and modification time alongside the hash,
/// we can skip expensive SHA-256 computation when the file metadata
/// hasn't changed.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct SourceMetadata {
    /// SHA-256 hash of the file content
    pub hash: String,
    /// File size in bytes
    pub size: u64,
    /// Modification time as Unix timestamp (seconds since epoch)
    pub mtime: u64,
    /// Sub-second component of the modification time, in nanoseconds.
    ///
    /// `None` when the writer could not read an mtime at all, or when the
    /// manifest predates this field (`#[serde(default)]` on read). The
    /// watcher gate's size+mtime fast path requires `Some` and an exact
    /// nanosecond match — a whole-second match alone cannot distinguish the
    /// hashed file from a same-size rewrite landing in the same second, so
    /// missing precision fails OPEN to hashing, never to suppression.
    #[serde(default)]
    pub mtime_nanos: Option<u32>,
    /// Inode change time (ctime) as Unix seconds, where the platform reports
    /// one (`None` on Windows and in manifests that predate the field).
    ///
    /// mtime can be forged from userland (`rsync -t`, `touch -r`,
    /// mtime-preserving editors); ctime cannot. A ctime disagreement never
    /// suppresses — it only demotes the size+mtime fast path to the hash
    /// tier, so false positives (a permission change bumps ctime) cost one
    /// hash and self-absorb. git's index does exactly this. See
    /// docs/archive/2026-08-18-watcher-reliability-architecture.md.
    #[serde(default)]
    pub ctime: Option<i64>,
    /// Inode number, where the platform reports one. Replace-via-rename (the
    /// atomic-save pattern) changes the inode even when size and mtime are
    /// preserved. Same fail-open rule as `ctime`: disagreement routes to the
    /// hash tier, absence changes nothing.
    #[serde(default)]
    pub inode: Option<u64>,
}

/// The forgery-resistant half of a stat record: (ctime seconds, inode).
///
/// `(None, None)` on platforms that report neither — every consumer must
/// fail open on `None` (compare only when both sides are `Some`).
pub fn stat_identity(md: &std::fs::Metadata) -> (Option<i64>, Option<u64>) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (Some(md.ctime()), Some(md.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = md;
        (None, None)
    }
}

impl moss_core::sort::SortableDoc for ParsedDocument {
    fn url_path(&self) -> &str { &self.url_path }
    fn date(&self) -> Option<&str> { self.date.as_deref() }
    fn weight(&self) -> Option<i32> { self.weight }
    fn declared_sort(&self) -> Option<&moss_core::sort::SortField> { self.sort.as_ref() }
    fn clean_stem(&self) -> &str { &self.clean_stem }
    fn is_folder_index(&self) -> bool { self.kind == moss_core::PageKind::Folder }
}

impl moss_core::sort::SortableLabel for ParsedDocument {
    fn label(&self) -> &str { &self.label }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsed_document_implements_sortable_doc() {
        use moss_core::sort::{SortableDoc, SortableLabel};
        let doc = ParsedDocument {
            url_path: "blog/post.html".to_string(),
            date: Some("2025-01-01".to_string()),
            weight: Some(10),
            clean_stem: "post".to_string(),
            label: "Post".to_string(),
            ..Default::default()
        };
        assert_eq!(doc.url_path(), "blog/post.html");
        assert_eq!(doc.date(), Some("2025-01-01"));
        assert_eq!(doc.weight(), Some(10));
        assert!(doc.declared_sort().is_none());
        assert_eq!(doc.clean_stem(), "post");
        assert_eq!(doc.label(), "Post");
    }

    #[test]
    fn listable_and_public_page_truth_table() {
        let mk = |draft: Option<bool>, listed: Option<bool>, slot_only: bool| {
            ParsedDocument { draft, listed, slot_only, ..Default::default() }
        };
        // (draft, listed, slot_only) -> (is_listable, is_public_page)
        // default page: listed + public
        let d = mk(None, None, false);
        assert_eq!((d.is_listable(), d.is_public_page()), (true, true));
        // listed: false -> off feeds, but still a public (indexable) page
        let d = mk(None, Some(false), false);
        assert_eq!((d.is_listable(), d.is_public_page()), (false, true));
        // listed: true explicit -> same as default
        let d = mk(None, Some(true), false);
        assert_eq!((d.is_listable(), d.is_public_page()), (true, true));
        // draft -> off feeds AND not a public page (noindex)
        let d = mk(Some(true), None, false);
        assert_eq!((d.is_listable(), d.is_public_page()), (false, false));
        // draft wins even if listed: true
        let d = mk(Some(true), Some(true), false);
        assert_eq!((d.is_listable(), d.is_public_page()), (false, false));
        // draft wins even if listed: false (both exclusions agree)
        let d = mk(Some(true), Some(false), false);
        assert_eq!((d.is_listable(), d.is_public_page()), (false, false));
        // slot_only chrome: neither
        let d = mk(None, None, true);
        assert_eq!((d.is_listable(), d.is_public_page()), (false, false));
    }

    /// One page carrying a footnote is enough to ship the sidenote script,
    /// and no page carrying one means it is never emitted — the asset set is
    /// per BUILD, not per page, so this OR is the whole gate.
    #[test]
    fn site_assets_of_aggregates_has_footnotes_across_pages() {
        let with_footnote = ParsedDocument {
            features: PageFeatures { footnotes: true, ..Default::default() },
            ..Default::default()
        };
        let without = ParsedDocument::default();

        let assets = SiteAssets::of(&[without.clone(), with_footnote], None, SiteAssets::default());
        assert!(assets.has_footnotes, "one footnote page must set the flag");

        let none = SiteAssets::of(&[without], None, SiteAssets::default());
        assert!(!none.has_footnotes, "no footnote page must leave it clear");
    }
}

#[cfg(test)]
mod series_field_tests {
    use super::*;

    #[test]
    fn series_field_deserializes_from_bool() {
        let yaml = "true";
        let field: SeriesField = serde_yaml::from_str(yaml).unwrap();
        match field {
            SeriesField::Flag(true) => {}
            _ => panic!("Expected Flag(true), got {:?}", field),
        }
    }

    #[test]
    fn series_field_deserializes_from_list() {
        let yaml = r#"
- "[[Chapter One]]"
- "[[Chapter Two]]"
- "[[Chapter Three]]"
"#;
        let field: SeriesField = serde_yaml::from_str(yaml).unwrap();
        match field {
            SeriesField::Ordered(list) => {
                assert_eq!(list.len(), 3);
                assert_eq!(list[0], "[[Chapter One]]");
            }
            _ => panic!("Expected Ordered, got {:?}", field),
        }
    }

    #[test]
    fn series_field_false() {
        let yaml = "false";
        let field: SeriesField = serde_yaml::from_str(yaml).unwrap();
        match field {
            SeriesField::Flag(false) => {}
            _ => panic!("Expected Flag(false), got {:?}", field),
        }
    }

    #[test]
    fn series_field_empty_list() {
        let yaml = "[]";
        let field: SeriesField = serde_yaml::from_str(yaml).unwrap();
        match field {
            SeriesField::Ordered(list) => assert!(list.is_empty()),
            _ => panic!("Expected Ordered([]), got {:?}", field),
        }
    }
}
