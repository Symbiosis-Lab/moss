---
title: Component classes
uid: 0da56c25
weight: 42
translationKey: docs-reference-components
description: The stable .moss-* class names emitted by moss's auto-generated components. Target these in .moss/theme/style.css.
---

Class names moss emits on auto-generated components. Target them directly in `.moss/theme/style.css`.

<!-- auto:start:components -->
| Class | Kind | Description |
|---|---|---|
| `.moss-cards` | container | Auto-generated listing of child pages. The single canonical container; layout density on `data-layout` (`grid` for cover-led tiles, `list` for cover+excerpt rows, `minimal` for text-only year-grouped indexes). Wrapped in `.moss-cards-container` to scope CSS container queries. |
| `.moss-cards-container` | container | Outer wrapper around `.moss-cards` that carries `container-type: inline-size` so the grid can use `@container` queries instead of viewport `@media` queries. Layout-agnostic — wraps any `data-layout` variant. |
| `.moss-summary-layout` | container | Retired: the additional co-class on `.moss-cards[data-layout="list"]` had no matching rules in the default CSS once `children_style: summary` collapsed into the list-layout block, and its lingering emission broke themes that hid the class (e.g. SoCiviC's `.moss/theme/style.css` keyed `display: none` on it, erasing folder-embed listings). Theme authors targeting summary listings should use `.moss-cards[data-layout="list"]` directly. |
| `.moss-cards-grid` | container | Retired in Phase 1c — collapsed into `.moss-cards[data-layout=grid]`. |
| `.moss-cards-list` | container | Retired in Phase 1c — collapsed into `.moss-cards[data-layout=list]`. |
| `.moss-cards-minimal-year-group` | container | Year-grouped section in minimal card layout (e.g. blog index). Modifier `--summary` collapses past years. |
| `.moss-cards-minimal-year-group--summary` | container | BEM modifier on `.moss-cards-minimal-year-group`. Applied to year groups that should render in collapsed summary form (e.g. past years on a blog index). |
| `.moss-card` | instance | v1 collapsed shape — single canonical instance class inside `.moss-cards`. Layout-specific styling targets `.moss-cards[data-layout=X] .moss-card`. Tag is `<a>` for ordinary cards and `<div>` for linkblog cards (`[data-linkblog]`). |
| `.moss-card-cover` | instance | Cover media slot inside `.moss-card`. Gets `.moss-card-no-cover` modifier when no image is present. |
| `.moss-card-no-cover` | instance | Modifier applied to `.moss-card-cover` when no cover media is available. |
| `.moss-card-content` | instance | Text content slot inside a grid-layout `.moss-card` (kicker + title + meta). |
| `.moss-card-row` | instance | Row wrapper inside a list-layout `.moss-card` holding body + cover side-by-side. |
| `.moss-card-body` | instance | Text body slot of a list-layout `.moss-card`. |
| `.moss-card-head` | instance | Header row of a `.moss-card-body` (title + kicker + meta). |
| `.moss-card-title` | instance | Title inside `.moss-card`. |
| `.moss-card-meta` | instance | Type-aware metadata slot (date for articles, count for folders, domain for links). Renders ABOVE the title in horizontal mode — filling the kicker position when the explicit `kicker` slot is unset, per `docs/design-system/preview-cards.md:22-30`. To the right of the title in vertical CJK mode (the horizontal kicker position transposed). Meta IS the visual kicker, with the same uppercase overline treatment. |
| `.moss-card-kicker` | instance | Eyebrow / overline above the title inside `.moss-card`. |
| `.moss-card-permalink` | instance | Author's-archive link mark (★, U+2605) emitted INSIDE `.moss-card-kicker` for linkblog cards (those whose child page has `external_url:`). The card title links to the external canonical (publisher); the `★` links to the local archival copy at the page's slug. Reads as part of the kicker line — "Publisher · Year ★". Semantically distinct from Daring-Fireball's linkblog ★ (which marks discussion permalink alongside commentary) — here the local copy is the same content preserved for resilience and stable bylines, not added commentary. Putting `<a>★</a>` inside the kicker is valid because linkblog cards emit `<div class="moss-card" data-linkblog>` (not `<a>`) as the outer element — see the `data-linkblog` attribute described on `.moss-card`. |
| `.moss-card-title-link` | instance | Anchor wrapping the `.moss-card-title` `<h3>` on linkblog cards. Ordinary cards have the whole-card `<a class="moss-card">` as the link target — but linkblog cards switch the outer to `<div>` so the kicker can host a nested `★` anchor, which means the title needs its own anchor to stay clickable. Same canonical-URL target as the other inner anchors (`moss-card-cover-link`, `moss-card-description-link`). |
| `.moss-card-cover-link` | instance | Anchor wrapping the `.moss-card-cover` on linkblog cards — same role as `.moss-card-title-link` but for the cover image / media. Targets the canonical (external) URL. |
| `.moss-card-description-link` | instance | Anchor wrapping the `.moss-card-description` on linkblog cards — same role as `.moss-card-title-link` but for the description excerpt. Targets the canonical (external) URL. |
| `.moss-card-description` | instance | Excerpt / description paragraph inside a `.moss-card` — below the title in both grid- and list-layout cards. |
| `.moss-card-count` | instance | Tertiary subtitle line showing `N articles` for a folder card. Renders only on non-date listings when the folder card has no `description` to display. |
| `.moss-embed-more` | instance | Trailing "More →" link on a truncated children listing (emitted when `children_limit` caps the embed); links to the folder's full index. Rendered as a sibling immediately after `.moss-cards-container`, so it sits outside the listing's flex `gap` and binds to the list via its own `margin-top` (see docs/architecture/ui-design/spacing.md). |
| `.moss-card-grid` | instance | Retired in Phase 1c — collapsed into `.moss-card` (with parent `.moss-cards[data-layout=grid]`). |
| `.moss-card-grid-cover` | instance | Retired in Phase 1c — collapsed into `.moss-card-cover`. |
| `.moss-card-grid-no-cover` | instance | Retired in Phase 1c — collapsed into `.moss-card-no-cover`. |
| `.moss-card-grid-content` | instance | Retired in Phase 1c — collapsed into `.moss-card-content`. |
| `.moss-card-grid-kicker` | instance | Retired in Phase 1c — collapsed into `.moss-card-kicker`. |
| `.moss-card-grid-title` | instance | Retired in Phase 1c — collapsed into `.moss-card-title`. |
| `.moss-card-grid-meta` | instance | Retired in Phase 1c — collapsed into `.moss-card-meta`. |
| `.moss-card-list` | instance | Retired in Phase 1c — collapsed into `.moss-card` (with parent `.moss-cards[data-layout=list]`). |
| `.moss-card-list-row` | instance | Retired in Phase 1c — collapsed into `.moss-card-row`. |
| `.moss-card-list-cover` | instance | Retired in Phase 1c — collapsed into `.moss-card-cover`. |
| `.moss-card-list-body` | instance | Retired in Phase 1c — collapsed into `.moss-card-body`. |
| `.moss-card-list-head` | instance | Retired in Phase 1c — collapsed into `.moss-card-head`. |
| `.moss-card-list-kicker` | instance | Retired in Phase 1c — collapsed into `.moss-card-kicker`. |
| `.moss-card-list-title` | instance | Retired in Phase 1c — collapsed into `.moss-card-title`. |
| `.moss-card-list-meta` | instance | Retired in Phase 1c — collapsed into `.moss-card-meta`. |
| `.moss-card-list-description` | instance | Retired in Phase 1c — collapsed into `.moss-card-description`. |
| `.moss-card-minimal` | instance | Retired in Phase 1c — collapsed into `.moss-card` (with parent `.moss-cards[data-layout=minimal]`). |
| `.moss-folder-item` | instance | Modifier on `.moss-card-minimal` for folder-type entries in minimal listings. |
| `.moss-folder-title` | instance | Title text of a folder entry in minimal listings. |
| `.moss-folder-description` | instance | Description paragraph of a folder entry in minimal listings. |
| `.moss-folder-link` | instance | Modifier on `.moss-prefix-link` for folder-type links in minimal listings. |
| `.moss-prefix-link` | instance | Link with a prefix span (date or icon) and a title span. Used inside minimal cards. |
| `.moss-prefix-link-prefix` | instance | Prefix slot of a prefix-link (typically a date). |
| `.moss-prefix-link-title` | instance | Title slot of a prefix-link. |
| `.moss-prefix-link-suffix` | instance | Optional trailing slot of a prefix-link. |
| `.moss-callout` | standalone | Obsidian-style callout. The Obsidian-compat `.callout` class is co-emitted; type lives on `data-type` (v1). |
| `.callout` | standalone | Obsidian-compat class co-emitted on every callout for theme parity. Type lives on `data-type` (v1). |
| `.callout-title` | instance | Title row of a callout. |
| `.callout-content` | instance | Body container of a callout. |
| `.callout-note` | instance | Retired in Phase 1c — type lives on `.callout[data-type=note]`. |
| `.callout-info` | instance | Retired in Phase 1c — type lives on `.callout[data-type=info]`. |
| `.callout-tip` | instance | Retired in Phase 1c — type lives on `.callout[data-type=tip]`. |
| `.callout-warning` | instance | Retired in Phase 1c — type lives on `.callout[data-type=warning]`. |
| `.callout-pending` | instance | Retired in Phase 1c — type lives on `.callout[data-type=pending]`. |
| `.moss-embed` | standalone | Base class on every typed embed. Kind on `data-type` (v1). Ambient video: add `data-loop` via `![[clip.mp4\|loop]]`. `.moss-embed-audio` / `-video` / `-pdf` / `-notebook` / `-table` / `-iframe` / `-3d` retired in Phase 1c. |
| `.moss-embed-audio` | instance | Retired in Phase 1c — collapsed to `.moss-embed[data-type=audio]`. |
| `.moss-embed-video` | instance | Retired in Phase 1c — collapsed to `.moss-embed[data-type=video]`. |
| `.moss-embed-pdf` | instance | Retired in Phase 1c — collapsed to `.moss-embed[data-type=pdf]`. |
| `.moss-embed-iframe` | instance | Retired in Phase 1c — collapsed to `.moss-embed[data-type=iframe]`. |
| `.moss-embed-notebook` | instance | Retired in Phase 1c — collapsed to `.moss-embed[data-type=notebook]`. |
| `.moss-embed-ipynb` | instance | Alias of `.moss-embed-notebook`; consolidation pending. |
| `.moss-embed-table` | instance | Retired in Phase 1c — collapsed to `.moss-embed[data-type=table]`. |
| `.moss-embed-3d` | instance | Retired in Phase 1c — collapsed to `.moss-embed[data-type=3d]`. |
| `.moss-embed-error` | instance | Error state for embeds whose target cannot be resolved. |
| `.moss-embed-missing` | instance | Fallback rendered when a folder-list embed (`![[journal/]]`) targets a folder that does not exist or cannot be resolved. Distinct from `.moss-embed-error` (file/wikilink resolution failure) — this one is specifically the folder-listing path. |
| `.moss-hero` | standalone | Hero banner section at the top of a page (cover image + title). v1 adds `data-width` for author-controlled sizing. |
| `.moss-hero-content` | instance | Text content slot inside `.moss-hero`. |
| `.moss-image` | standalone | Wrapper around an inline `&lt;img> for sizing and figure semantics. `data-width` carries a named width token (body\|wide\|page\|screen); a content-relative width is instead emitted as inline `style="width:NN%"` (set by the editor drag-resize), which also forces the inner image to fill that percent box. Images narrower than the content column center horizontally. |
| `.moss-align-left` | standalone | Floats an image to the left of body text (editorial runaround). Defaults max-width to 50% on desktop, collapses to full-width below 48rem. CSS `:has()` escalates the float to a wrapping `<figure class="moss-image">` or `<picture>` when present. Mirrors WordPress's `alignleft` convention. |
| `.moss-align-right` | standalone | Floats an image to the right of body text (editorial runaround). Symmetric counterpart to `.moss-align-left`. Mirrors WordPress's `alignright` convention. |
| `.moss-article-title` | instance | Article-page H1 title emitted from frontmatter. |
| `.moss-heading-anchor` | instance | Clickable permalink appended inside every author-written body heading that carries a slug id; links to the heading's `#`-fragment. The auto-injected `moss-article-title` H1 is emitted separately and gets no anchor. |
| `.moss-grid` | container | Generic grid container (used by profiles, link previews, etc.). Modifier classes: `profiles`, `featured`, `no-cards`, `summary`. `summary` REPLACES this container rather than decorating it: the fence emits `.moss-cards-container > .moss-cards[data-layout="list"]` instead, so a `.moss-grid` never reaches the page and a column count is dropped — `scroll` is dropped the same way when both are written on one fence. v1 adds `data-width` (P9); a later release adds `data-scroll`. |
| `.moss-grid-card` | instance | Card instance inside `.moss-grid`. Today emits sibling classes `link-card` / `friend-card` / `no-cards`; v1 collapses to `data-kind`. |
| `.moss-gallery` | container | Image gallery container. v1 adds `data-width` (P9). |
| `.moss-gallery-item` | instance | Single image entry inside `.moss-gallery`. |
| `.moss-buttons` | container | Container for a row of `.moss-btn` buttons. v1: the inverted variant is on `data-style="inverted"`. |
| `.moss-btn` | standalone | Generic button primitive. Role on `data-role` (v1). |
| `.moss-btn__label` | instance | Label span inside `.moss-btn`. |
| `.moss-btn__check` | instance | Success checkmark slot inside `.moss-btn`. |
| `.moss-btn__spinner` | instance | Loading spinner slot inside `.moss-btn`. |
| `.moss-subscribe` | standalone | Newsletter subscribe block (auto-injected into footer when email channel configured). |
| `.moss-subscribe-form` | instance | Form element inside `.moss-subscribe`. |
| `.moss-btn-slot` | instance | Fixed-width slot wrapping a form's submit button; used by both the subscribe and comment forms to prevent layout shift across idle/loading/success states. |
| `.moss-subscribe-status` | instance | Status message shown after submit (success/error). |
| `.moss-subscribe-status__icon` | instance | Icon slot inside `.moss-subscribe-status`. |
| `.moss-subscribe-landing` | standalone | Standalone subscribe landing page surface (larger variant). |
| `.moss-series-nav` | standalone | Series navigation bar (prev/next/collection) on series pages. |
| `.moss-series-nav-links` | instance | Row holding prev/next links in series nav. |
| `.moss-series-nav-link` | instance | Individual link inside series nav. Modifiers: `moss-series-nav-prev`, `moss-series-nav-next`, `empty` (placeholder). |
| `.moss-series-nav-prev` | instance | Previous-page modifier on a series nav link. |
| `.moss-series-nav-next` | instance | Next-page modifier on a series nav link. |
| `.moss-series-nav-arrow` | instance | Arrow glyph inside a series-nav link. |
| `.moss-series-nav-title` | instance | Title text of the destination page in a series-nav link. |
| `.moss-series-nav-collection` | instance | Collection-listing slot in series nav (sibling pages). |
| `.moss-series-nav-collection-row` | instance | Row inside the collection listing of series nav. |
| `.moss-collection-cover` | standalone | Header surface on a collection landing page. |
| `.moss-collection-cover-row` | instance | Row inside `.moss-collection-cover`. |
| `.moss-collection-cover-body` | instance | Body content slot inside `.moss-collection-cover`. |
| `.moss-input` | standalone | Generic form input primitive. |
| `.moss-field` | container | Form field group (label + input). Modifier `--inline` for horizontal layout. |
| `.moss-label` | instance | Label primitive for `.moss-field`. Modifier `--small` for compact form. |
| `.moss-link` | standalone | Inline-link primitive (resets `<button>` chrome too). Use `--subtle` for muted variant. |
| `.moss-field--inline` | instance | BEM modifier on `.moss-field` for horizontal label+input layout (used by settings UI primitives). |
| `.moss-label--small` | instance | BEM modifier on `.moss-label` for compact form (used by services settings rows). |
| `.moss-info-grid` | container | Two-column aligned label+value rows (CSS grid with `display: contents` children). Used by the deployment settings panel; ships in the default theme so authors can reuse the layout. |
| `.moss-row` | container | Horizontal flex row of equal-flex `.moss-field` children. Form-row layout helper shipped in the default theme. |
| `.moss-input-feedback` | instance | Auto-save status hint slot under `.moss-field`. Three state modifiers: `--success`, `--error`, `--fade-out`. |
| `.moss-input-feedback--success` | instance | Success state modifier on `.moss-input-feedback`. |
| `.moss-input-feedback--error` | instance | Error state modifier on `.moss-input-feedback`. |
| `.moss-input-feedback--fade-out` | instance | Transient fade-out modifier on `.moss-input-feedback` (applied after a success message to dismiss it). |
| `.moss-comments` | standalone | Comments surface (per-site SQLite backend or Artalk legacy). |
| `.moss-service-inactive` | instance | Co-class applied to `.moss-comments` and `.moss-subscribe-form` when the backing service is not configured. Hidden by default in published sites and revealed inside the preview chrome so authors can see the inactive surface during editing. |
| `.moss-preview-popup` | chrome | Floating link-preview popover injected at `document.body` level by the runtime `preview.js`. Fetches `/_moss/previews.json` and renders a hover card with title, description, and excerpt for internal links. |
| `.moss-preview-title` | instance | Title slot inside `.moss-preview-popup`. |
| `.moss-preview-desc` | instance | Description slot inside `.moss-preview-popup` (from frontmatter `description`). |
| `.moss-preview-text` | instance | Excerpt slot inside `.moss-preview-popup` (auto-extracted from the linked article body). |
| `.moss-colophon` | chrome | Footer colophon credit appended by moss. |
| `.moss-colophon-icon` | instance | Icon slot inside `.moss-colophon`. |
| `.moss-shell-frame` | chrome | App-shell frame surface (preview chrome). |
| `.moss-mobile-frame` | chrome | Runtime marker the preview bridge adds to `<html>` when the shell is in mobile device-preview mode; drops the titlebar-clearance padding (the phone frame sits below the titlebar). |
| `.main-nav` | chrome | Top site navigation bar. Legacy non-`moss-` prefix kept for theme parity. |
| `.moss-child-section-divider` | instance | Divider rule between auto-generated child sections. |
| `.moss-unknown-shortcode` | standalone | Fallback emitted when a shortcode tag is not recognised by any plugin. |
| `.moss-hl-keyword` | instance | Syntax-highlight token: keyword. |
| `.moss-hl-string` | instance | Syntax-highlight token: string literal. |
| `.moss-hl-comment` | instance | Syntax-highlight token: comment. |
| `.moss-hl-function` | instance | Syntax-highlight token: function name. |
| `.moss-hl-type` | instance | Syntax-highlight token: type name. |
| `.moss-hl-number` | instance | Syntax-highlight token: numeric literal. |
| `.moss-hl-operator` | instance | Syntax-highlight token: operator. |
| `.moss-hl-builtin` | instance | Syntax-highlight token: builtin identifier. |
| `.moss-hl-tag` | instance | Syntax-highlight token: markup tag name. |
| `.moss-hl-attr` | instance | Syntax-highlight token: attribute name. |
| `.moss-hl-meta` | instance | Syntax-highlight token: meta/annotation. |
| `.moss-hl-addition-bg` | instance | Syntax-highlight diff token: added-line background. |
| `.moss-hl-deletion` | instance | Syntax-highlight diff token: removed-line text. |
| `.moss-hl-deletion-bg` | instance | Syntax-highlight diff token: removed-line background. |
| `.moss-recent` | container | Auto-generated list of recent posts emitted by the `:::recent` shortcode. Sorted newest-first; date and description slots are filled per child. No default CSS in the bundled theme — theme authors style it freely. |
| `.moss-recent__date` | instance | Per-entry date slot inside `.moss-recent` (BEM child). Format is `YYYY-MM-DD`, derived from frontmatter `date`. Empty string when the post lacks a parseable date. |
| `.moss-recent__desc` | instance | Per-entry description slot inside `.moss-recent` (BEM child). Sourced from frontmatter `description`; empty when unset. |
| `.moss-ambient-video` | standalone | JS-injected wrapper around a `video[data-loop]` element. Provides the positioning context for `.moss-ambient-toggle` and the `[data-paused]` state hook. Not emitted by the Rust synthesizer — ambient-video.ts creates it at init. |
| `.moss-ambient-toggle` | instance | Chrome-free pause/play toggle button for ambient loop videos. Injected by ambient-video.ts. Keyboard-focusable; `aria-label` toggles between "Pause video" and "Play video". Visible on hover/focus of `.moss-ambient-video` and always visible when `[data-paused]`. Satisfies WCAG 2.2.2 Level A (Pause, Stop, Hide). |
<!-- auto:end:components -->
