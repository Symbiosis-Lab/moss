/**
 * The scratch sites every cascade / theme render gate builds.
 *
 * These are fixtures — markdown, config, and a few lines of user CSS — and
 * nothing else; `scratch-site.ts` owns scaffolding, building, and resolving the
 * output directory. Each `playwright/*.config.ts` imports one spec from here
 * and calls `buildScratchSite` at parse time.
 *
 * They live together because the interesting content of each is three lines of
 * CSS, and reading them side by side is how you see what each gate isolates.
 * Previously each sat in its own ~150-line `*-global-setup.ts` under 130 lines
 * of identical scaffolding; the `ui-accent-seam` fixture had additionally been
 * pasted a second time into its config, and the two copies had already drifted.
 *
 * Ported set: only the gates whose spec lives under `tests/render-gates/site/`
 * or `tests/render-gates/cascade/` and is actually wired into
 * `scripts/render-gates.sh`. The app/editor/preview gates, and a few site
 * gates not yet wired anywhere, stay in the private desktop repo.
 */
import {
  ScratchSiteSpec,
  findLinkHref,
  requireLinkHref,
} from "./scratch-site";

const CONFIG_TOML = `schema_version = 5

[site]
lang = "en"
`;

/**
 * The same config plus the floating nav island, for the one gate that measures
 * it. The island has been opt-in since ADR-049 §1's 2026-08-30 amendment, so a
 * site that says nothing gets none — and the footnote gate's forward-jump case
 * is specifically about landing clear of the island.
 */
const CONFIG_TOML_WITH_ISLAND = `${CONFIG_TOML}floating_nav = true
`;

const MOSS_CSS = /\/_moss\/style\.[a-f0-9]+\.css/;
const THEME_CSS = /\/_moss\/theme\/style\.[a-f0-9]+\.css/;

// ── Task 2.1: @layer cascade contract ────────────────────────────────────────
// A token override and a plain selector override, both in the user's theme
// sheet, which moss links with layer="themes". Both must beat moss's own
// layers. Served by playwright/customization-cascade.config.ts.
export const CASCADE_GATE: ScratchSiteSpec = {
  name: "cascade-gate",
  files: {
    "index.md": `---
title: Cascade Test Site
uid: "c8a5de01"
---

# Cascade Contract Test

This page verifies the @layer cascade contract.
`,
    // A second page, so the site has a real .main-nav link to probe.
    "About.md": `---
title: About
uid: "c8a5de02"
---

# About

About page.
`,
    ".moss/config.toml": CONFIG_TOML,
    ".moss/theme/style.css": `:root {
  --moss-color-bg: #abcdef;
}

.main-nav a {
  color: rgb(255, 0, 0);
}
`,
  },
};

// ── Native feature CSS in @layer plugins ─────────────────────────────────────
// Comments on, so the build emits the comments feature sheet in
// `@layer plugins`; the user's `.comment-author` rule is in `@layer themes` and
// must win. Served by playwright/comments-cascade.config.ts.
export const COMMENTS_CASCADE_GATE: ScratchSiteSpec = {
  name: "comments-cascade-gate",
  files: {
    "index.md": `---
title: Comments Cascade Test
---

# Comments Cascade Test

This page verifies the native feature CSS @layer contract.
`,
    // moss needs an article with a uid to generate the comment section —
    // without one no per-page comment form is injected, and there is nothing
    // for the gate to probe.
    "hello.md": `---
title: Hello
uid: "casc0001"
date: 2026-01-01
---

Hello world.
`,
    ".moss/config.toml": `schema_version = 5

[site]
lang = "en"

[services.comments]
enabled = true
`,
    // Domain required so resolved_server_url returns the moss-operated
    // endpoint rather than an empty string (which skips comment injection).
    ".moss/state.toml": `[deployment]
domain = "example.com"
deploy_method = "moss"
`,
    ".moss/theme/style.css": `.comment-author {
  color: rgb(0, 128, 0);
}
`,
  },
};

// ── Task 2.4: dark tokens win by layer order, not specificity ────────────────
// The override below uses `[data-theme="dark"]` WITHOUT a `:root` prefix — and
// so does the tokens-layer dark block moss emits. That equality is the whole
// point: with identical specificity only layer order can decide, so an author
// win proves `themes` > `tokens`. Adding `:root` here would let specificity
// mask layer order and the gate would stop testing anything.
// Served by playwright/dark-layer-order.config.ts.
export const DARK_LAYER_ORDER_GATE: ScratchSiteSpec = {
  name: "dark-layer-order-gate",
  files: {
    "index.md": `---
title: Dark Layer Order Gate
uid: "dlo24a01"
---

# Dark Layer Order Test

Scratch site for Task 2.4 render gate.
`,
    ".moss/config.toml": CONFIG_TOML,
    ".moss/theme/style.css": `[data-theme="dark"] {
  --moss-color-bg: #111111;
}
`,
  },
};

// ── Heading weight token ─────────────────────────────────────────────────────
// 440 cannot be confused with any hardcoded default (480/500/600), so a
// matching computed weight proves the token actually controls the property.
// Served by playwright/heading-weight.config.ts.
export const HEADING_WEIGHT_GATE: ScratchSiteSpec = {
  name: "heading-weight-gate",
  files: {
    "index.md": `---
title: Heading Weight Test
uid: "hw001a"
---

Welcome to the heading weight test site.
`,
    // A standalone article (non-index) gets an auto-injected
    // .moss-article-title. No body `# heading`, so the pipeline injects one
    // from the YAML title.
    "token-heading.md": `---
title: Token Heading
uid: "hw002a"
---

## An In-Content H2

Body text beneath the auto-injected article title.
`,
    ".moss/config.toml": CONFIG_TOML,
    ".moss/theme/style.css": `:root {
  --moss-font-heading-weight: 440;
}
`,
  },
};

// ── Task 2.3: pre-paint data-theme ───────────────────────────────────────────
// The user CSS carries ONLY a `[data-theme="dark"]` selector and NO
// `@media (prefers-color-scheme: dark)` block. That absence is the assertion:
// with colorScheme:"dark" and empty localStorage, the dark background can only
// appear if the pre-paint inline <script> set `data-theme` on <html> before the
// stylesheet was parsed. Remove the script and the gate goes red.
// Served by playwright/pre-paint-dark.config.ts.
export const PRE_PAINT_DARK_GATE: ScratchSiteSpec = {
  name: "pre-paint-dark-gate",
  files: {
    "index.md": `---
title: Pre-paint Dark Gate
uid: "ppd23a01"
---

# Pre-paint Dark Theme Test

Scratch site for Task 2.3 render gate.
`,
    ".moss/config.toml": CONFIG_TOML,
    ".moss/theme/style.css": `:root[data-theme="dark"] {
  --moss-color-bg: #111111;
}
`,
  },
};

// ── Task 2.2: minimize !important ────────────────────────────────────────────
// Both user rules below used to need `!important` to win. They now sit in
// `@layer themes`, which beats `@layer shortcodes` on layer order alone — so
// the gate is that they still win with no `!important` anywhere.
//
// The probe page exists because the assertions need markup moss would not emit
// on its own. Served by playwright/no-important-cascade.config.ts.
export const NO_IMPORTANT_GATE: ScratchSiteSpec = {
  name: "no-important-gate",
  files: {
    "index.md": `---
title: No-Important Gate Site
uid: "nigate01"
---

# No-Important Gate

Scratch site for Task 2.2 render gate.
`,
    "About.md": `---
title: About
uid: "nigate02"
---

# About

About page.
`,
    ".moss/config.toml": CONFIG_TOML,
    ".moss/theme/style.css": `article footer .moss-grid {
  grid-template-columns: 1fr 1fr;
}

.read-more {
  color: rgb(0, 0, 255);
}
`,
  },
  probe: (indexHtml) => ({
    fileName: "gate-test.html",
    html: `<!DOCTYPE html>
<html>
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <!-- moss site CSS — inside @layer shortcodes -->
  <link rel="stylesheet" href="${requireLinkHref(indexHtml, MOSS_CSS, "the moss stylesheet")}">
  <!-- user theme CSS — loaded with layer="themes" (themes > shortcodes) -->
  <link rel="stylesheet" href="${requireLinkHref(indexHtml, THEME_CSS, "the theme stylesheet")}" layer="themes">
</head>
<body>
  <article>
    <footer>
      <!-- At 390px the moss mobile rule collapses this to 1fr (no !important).
           The user rule in @layer themes must still win with 2 tracks. -->
      <div class="moss-grid" data-columns="3" id="test-grid">
        <div class="moss-grid-card">Card A</div>
        <div class="moss-grid-card">Card B</div>
        <div class="moss-grid-card">Card C</div>
      </div>
    </footer>
    <!-- Previously needed !important to beat \`article a { color }\`. -->
    <a class="read-more" id="test-read-more" href="#">Read more &rarr;</a>
  </article>
</body>
</html>`,
  }),
};

// ── Task 2.5: the --moss-color-ui-accent seam ────────────────────────────────
// Two sites, because the seam is only visible as a difference between them:
//
//   default   no theme CSS → --moss-color-ui-accent falls back to
//             var(--moss-color-accent); both probes read rgb(45, 90, 45).
//   override  theme CSS points ui-accent at var(--moss-color-text); the chrome
//             probe moves to rgb(44, 40, 37) while the content probe stays
//             rgb(45, 90, 45).
//
// The override site's *unchanged* content probe is the actual assertion: chrome
// and content accents are independently overridable, and only chrome follows
// ui-accent. Served by playwright/ui-accent-seam.config.ts.
//
// Inline styles in the probe, so it does not depend on any class name in the
// built CSS — we read back the resolved value of the custom properties
// themselves. The theme link carries layer="themes" so the :root override
// is live.
const uiAccentProbe = (indexHtml: string) => {
  const themeCss = findLinkHref(indexHtml, THEME_CSS); // absent on the default site by design
  return {
    fileName: "ui-accent-seam-probe.html",
    html: `<!DOCTYPE html>
<html>
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <link rel="stylesheet" href="${requireLinkHref(indexHtml, MOSS_CSS, "the moss stylesheet")}">${
    themeCss ? `\n  <link rel="stylesheet" href="${themeCss}" layer="themes">` : ""
  }
  <style>
    #chrome-probe { color: var(--moss-color-ui-accent); }
    #content-probe { color: var(--moss-color-accent); }
  </style>
</head>
<body>
  <!-- Chrome probe: nav/buttons/controls use --moss-color-ui-accent -->
  <div id="chrome-probe">chrome accent probe</div>
  <!-- Content probe: article links use --moss-color-accent directly -->
  <div id="content-probe">content accent probe</div>
</body>
</html>`,
  };
};

const uiAccentIndex = (label: string, uid: string): string => `---
title: UI Accent Seam ${label} Gate
uid: "${uid}"
---

# UI Accent Seam — ${label}

Scratch site for Task 2.5 render gate.
`;

export const UI_ACCENT_SEAM_DEFAULT: ScratchSiteSpec = {
  name: "ui-accent-seam-default",
  files: {
    "index.md": uiAccentIndex("Default", "uas25d01"),
    ".moss/config.toml": CONFIG_TOML,
    // null, not absent: target/test-tmp/ survives between runs, so an earlier
    // edit of this fixture that DID write theme CSS would otherwise leave the
    // "no override" site quietly overridden.
    ".moss/theme/style.css": null,
  },
  probe: uiAccentProbe,
};

export const UI_ACCENT_SEAM_OVERRIDE: ScratchSiteSpec = {
  name: "ui-accent-seam-override",
  files: {
    "index.md": uiAccentIndex("Override", "uas25o01"),
    ".moss/config.toml": CONFIG_TOML,
    ".moss/theme/style.css": `:root {
  --moss-color-ui-accent: var(--moss-color-text);
}
`,
  },
  probe: uiAccentProbe,
};

// ── Nav toggle cluster: one optical family ───────────────────────────────────
// A site with search on AND a translation, so `.nav-icons` carries all three
// toggles (search button, language toggle, theme button) plus a couple of nav
// links to sit beside.
// Served by playwright/nav-toggle-cluster.config.ts.
export const NAV_TOGGLE_CLUSTER_GATE: ScratchSiteSpec = {
  name: "nav-toggle-cluster-gate",
  files: {
    // `breadcrumb: true` on the homepage is the site-wide enable; the deep page
    // below is what turns it into a trail long enough to wrap nav-right.
    "index.md": `---
title: Nav Toggle Cluster
uid: "ntc00101"
breadcrumb: true
---

# Nav Toggle Cluster

Scratch site for the nav toggle-cluster render gate.
`,
    // The translation is what makes .nav-lang-toggle appear.
    "index.zh-hans.md": `---
title: 导航切换组
uid: "ntc00102"
---

# 导航切换组

导航切换组渲染门测试站点。
`,
    // Two more pages so .nav-links is non-empty and the cluster has a
    // neighbouring group to be separated from.
    "About.md": `---
title: About
uid: "ntc00103"
---

# About
`,
    "Notes.md": `---
title: Notes
uid: "ntc00104"
---

# Notes
`,
    // A page deep enough, with names long enough, that the breadcrumb fills
    // row 1 and pushes .nav-right onto row 2 at a DESKTOP width. That is the
    // case the wrapped-row assertion needs: the wrap has to be content-driven
    // (long trail), not breakpoint-driven, because the rules under test —
    // `.nav-right { flex: 1 1 auto }` + `.nav-icons { margin-inline-start: auto }`
    // — are unconditional defaults whose behaviour keys off whether nav-right
    // actually wrapped. They used to be inside `@media (max-width: 32rem)`,
    // which is exactly why a wide-screen overflow got the wrong treatment.
    // The folder index: middle breadcrumb segments link to it.
    "Programmes and Long Form Reporting/index.md": `---
title: Programmes and Long Form Reporting
uid: "ntc00105"
---

# Programmes and Long Form Reporting
`,
    "Programmes and Long Form Reporting/Field Notes From a Very Long Section Title.md": `---
title: Field Notes From a Very Long Section Title
uid: "ntc00106"
---

# Field Notes From a Very Long Section Title
`,
    ".moss/config.toml": `schema_version = 5

[site]
lang = "en"
search = true
`,
    ".moss/theme/style.css": null,
  },
};

// ── Grid mobile collapse + grid-cell image parity ────────────────────────────
// Three grids on one page, each answering a question only a laid-out engine
// can answer:
//
//   1. a plain `:::grid 3` — collapses to ONE track below 768px;
//   2. a ratio `:::grid 2 1:2` — collapses too. It used to carry an inline
//      `style="grid-template-columns:1fr 2fr"`, which outranks every rule in
//      every stylesheet, so the collapse could not reach it at any width. The
//      ratio now rides as `--moss-grid-ratio`, and the desktop assertion (2:1
//      track widths) is what proves the variable still drives the layout;
//   3. a two-cell grid whose cells hold the SAME image written two ways — a
//      wikilink embed and a markdown image. One authorial intent, so the two
//      cells must present the same box.
//
// `implicit_figure = false`, because that is the site setting the two spellings
// used to disagree about: moss honoured it for `![alt](tile.svg)` and ignored
// it for `![[tile.svg]]`, so the wikilink cell kept a `<figure
// class="moss-image">` the other cell did not have — and every theme rule
// keyed on `.moss-image` then reached one cell of the pair.
//
// `{.no-cards}` on the third grid: without it a cell that is a single internal
// link is replaced wholesale by a rendered collection card, and the image the
// gate measures is gone.
//
// A fourth question lives in the same fixture for the same reason the first
// three do — only a laid-out engine can answer it: `rows/` is an ordinary
// `children_style: grid` folder (three children, each with a cover, reusing
// `tile.svg`) whose listing must become phone ROWS — thumbnail left, title
// right — below `36rem`, and stay the normal cover-on-top column above it.
// docs/archive/2026-09-11-home-feed-cards-and-archive-link.md §2.
//
// A fifth, for the same reason: the `Shelf` section is a `:::grid 3` of
// wikilinks to covered pages — the cards a directive makes are a different
// component from a listing's — and it must become the same rows below `36rem`
// and stay columns above.
//
// An SVG, not a PNG: `ScratchSiteSpec.files` is text, and an SVG is text.
// Served by playwright/grid-mobile-collapse.config.ts.
const GRID_TILE_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="600" height="400" viewBox="0 0 600 400"><rect width="600" height="400" fill="#4a7"/></svg>
`;

export const GRID_MOBILE_COLLAPSE_GATE: ScratchSiteSpec = {
  name: "grid-mobile-collapse-gate",
  files: {
    "tile.svg": GRID_TILE_SVG,
    "index.md": `---
title: Grid Collapse
uid: "gmc00101"
---

# Grid Collapse

## Plain

:::grid 3
One
+++
Two
+++
Three
:::

## Ratio

:::grid 2 1:2
Narrow cell
+++
Wide cell
:::

## Image parity

:::grid 2 {.parity .no-cards}
[![[tile.svg]]](/about/)
+++
[![Tile](tile.svg)](/about/)
:::

## Shelf

:::grid 3
[[Shelf Alpha]]
+++
[[Shelf Beta]]
+++
[[Shelf Gamma]]
:::
`,
    "vertical/index.md": `---
title: Vertical Grid
uid: "gmc00110"
typesetting: vertical
---

:::grid 3
One
+++
Two
+++
Three
:::

:::grid 2 1:2
Narrow cell
+++
Wide cell
:::
`,
    "About.md": `---
title: About
uid: "gmc00102"
---

# About
`,
    "rows/index.md": `---
title: Rows
uid: "gmc00103"
children_style: grid
---
`,
    "rows/one.md": `---
title: One
uid: "gmc00104"
cover: ../tile.svg
---
`,
    "rows/two.md": `---
title: Two
uid: "gmc00105"
cover: ../tile.svg
---
`,
    "rows/three.md": `---
title: Three
uid: "gmc00106"
cover: ../tile.svg
---
`,
    // Lone-wikilink :::grid cells resolving to covered pages — the "shelf"
    // cards a home page builds with `:::grid 3` + `[[wikilinks]]`. grid_cells.rs
    // substitutes the SAME `<a class="moss-card">` markup `rows/` gets (see
    // the shared `.moss-cards[data-layout="grid"] .moss-card, .moss-grid
    // .moss-card` selector in site.css), so these three exist to prove the
    // phone-row treatment reaches that second component too.
    //
    // Filenames match the `[[wikilinks]]` above verbatim — wikilink
    // resolution is Obsidian-style fuzzy filename matching (content_graph.rs),
    // not a lookup against `title:` frontmatter.
    "Shelf Alpha.md": `---
title: Shelf Alpha
uid: "gmc00107"
cover: tile.svg
---
`,
    "Shelf Beta.md": `---
title: Shelf Beta
uid: "gmc00108"
cover: tile.svg
---
`,
    "Shelf Gamma.md": `---
title: Shelf Gamma
uid: "gmc00109"
cover: tile.svg
---
`,
    ".moss/config.toml": `schema_version = 5

[site]
lang = "en"
implicit_figure = false
`,
    // No user theme: this gate is about moss's own defaults.
    ".moss/theme/style.css": null,
  },
};

// ── A grid-card image fills the card's inline size ───────────────────────────
// An external-link cell renders as a flex column (`a.moss-grid-card
// .link-preview`) whose figure carries `margin-inline: auto`; nothing else
// stretches it, so a lazy image's `sizes="auto, …"` sizes the source from the
// figure's own shrunk-to-caption width unless
// `.moss-grid-card :is(.moss-image, picture, img) { inline-size: 100% }`
// (site.css) holds. That needs a real `<figure class="moss-image">` around
// the image, which only exists with moss's default `implicit_figure` (unset
// — true here), unlike GRID_MOBILE_COLLAPSE_GATE above, which turns it off to
// test the plain-`<p>` shape instead — this gate cannot reuse that site.
// Both writing modes matter: under vertical typesetting the inline axis is
// the box's physical HEIGHT, and the base rule this one has to outrank sets
// a PHYSICAL `height: auto`, which is the inline axis there.
// Served by playwright/grid-card-image-inline-size.config.ts.
//
// Deliberately tiny intrinsic size (40×30, an order of magnitude below any
// card): `article figure:not(.video-figure) img { max-width: 100% }` already
// caps an OVERSIZED image down to the card, masking this exact regression —
// the bug this gate guards is an image that never gets STRETCHED UP to fill
// a card bigger than it, which only a real `sizes="auto"` lazy fetch or (as
// here) a genuinely small source can exercise without depending on a lazy
// network fetch resolving inside the test.
const CARD_IMAGE_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30" viewBox="0 0 40 30"><rect width="40" height="30" fill="#a47"/></svg>
`;

export const GRID_CARD_IMAGE_INLINE_SIZE_GATE: ScratchSiteSpec = {
  name: "grid-card-image-inline-size-gate",
  files: {
    "tile.svg": CARD_IMAGE_SVG,
    "index.md": `---
title: Card Image Inline Size
uid: "gcis0101"
---

:::grid 2 {.no-cards}
[![被兩地驅逐的人](tile.svg)](https://example.org/)
+++
[![家鎖：家庭不只是你自己的事](tile.svg)](/about/)
:::
`,
    // CJK alt text on both cells, plus a standalone (non-grid) captioned
    // figure: the grid-card figcaption gate (below) needs real card
    // captions to assert `writing-mode` on, and the standalone figure is
    // the control — an ordinary article figure whose caption the vertical
    // exception in site/vertical.css still governs, unlike the grid-card
    // ones the fix carves out.
    "vertical/index.md": `---
title: Card Image Inline Size Vertical
uid: "gcis0102"
typesetting: vertical
---

:::grid 2 {.no-cards}
[![被兩地驅逐的人](tile.svg)](https://example.org/)
+++
[![家鎖：家庭不只是你自己的事](tile.svg)](/about/)
:::

![人形物體載浮載沉](tile.svg)
`,
    "About.md": `---
title: About
uid: "gcis0103"
---

# About
`,
    ".moss/config.toml": CONFIG_TOML,
    // No user theme: this gate is about moss's own defaults.
    ".moss/theme/style.css": null,
  },
};

// ── A hero that carries a caption ────────────────────────────────────────────
// Two claims no text assertion can settle. The caption must be READABLE — below
// the photograph, not printed across it and not swallowed by the section's
// `overflow: hidden`. And a hero whose caption names a subject must show the
// whole image, because `object-fit: cover` crops to fill and can cut the named
// subject out of the frame. Both are questions about what an engine paints.
//
// The image is deliberately portrait against a wide viewport: that is the shape
// where cover and contain disagree most, so a regression is a large measurable
// difference rather than a rounding error.
// Served by playwright/hero-caption.config.ts.
const HERO_PORTRAIT_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="400" height="800" viewBox="0 0 400 800"><rect width="400" height="800" fill="#357"/><circle cx="200" cy="740" r="40" fill="#fc3"/></svg>
`;

export const HERO_CAPTION_GATE: ScratchSiteSpec = {
  name: "hero-caption-gate",
  files: {
    "cover.svg": HERO_PORTRAIT_SVG,
    "captioned.md": `---
title: With a credit
uid: "hcg00101"
---

:::hero {image=cover.svg caption="Cover: the memorial wall at the monastery gate (photo: A. Photographer)"}
:::

Body text.
`,
    "plain.md": `---
title: Without one
uid: "hcg00102"
---

:::hero {image=cover.svg}
:::

Body text.
`,
    ".moss/config.toml": CONFIG_TOML,
    // No user theme: this gate is about moss's own defaults.
    ".moss/theme/style.css": null,
  },
};

// ── A pale hero flips its text instead of darkening its picture ──────────────
// moss reacts to a pale cover by setting `data-hero-tone="light"`. That used to
// mean "lay the scrim on thicker so white type survives" — 0.78 black at the
// bottom of the frame — which on a designed pastel artwork reads as a dirty
// band across the very thing the author chose. It now means the opposite: no
// scrim, dark type.
//
// Nothing about that is provable from emitted text. `content: none` versus
// `content: ""` on a `::before`, and which of four same-specificity rules wins
// a tie decided by source order, exist only once an engine has resolved the
// cascade — and jsdom implements no `@layer` precedence at all.
//
// A probe page rather than a built hero, because the tone attribute comes from
// decoding the cover's pixels and this gate is not about that half: the Rust
// test `hero_with_light_cover_gets_light_tone_attribute` already pins when the
// attribute is emitted. Here the attribute is a given and the question is what
// the browser paints under it. The three cases are the three layouts the rules
// have to tell apart — overlaid on desktop, overlaid on mobile, and stacked
// below the image on mobile, where the text is NOT on the picture and must
// keep its white-on-band treatment.
// Served by playwright/hero-tone.config.ts.
export const HERO_TONE_GATE: ScratchSiteSpec = {
  name: "hero-tone-gate",
  files: {
    "index.md": `---
title: Hero tone gate
uid: "htg00101"
---

Scratch site; the assertions run against the probe page.
`,
    ".moss/config.toml": CONFIG_TOML,
    // No user theme: this gate is about moss's own defaults.
    ".moss/theme/style.css": null,
  },
  probe: (indexHtml) => ({
    fileName: "hero-tone.html",
    html: `<!DOCTYPE html>
<html>
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <link rel="stylesheet" href="${requireLinkHref(indexHtml, MOSS_CSS, "the moss stylesheet")}">
</head>
<body>
  <!-- Pale cover, overlaid. No scrim; dark type. -->
  <section class="moss-hero" data-hero-tone="light" id="light-hero">
    <img src="data:image/gif;base64,R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==" alt="">
    <div class="moss-hero-content">
      <h2 id="light-heading">Pale cover heading</h2>
      <p id="light-para">Pale cover paragraph.</p>
    </div>
  </section>

  <!-- No tone attribute: moss's default. Scrim on, white type. -->
  <section class="moss-hero" id="dark-hero">
    <img src="data:image/gif;base64,R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==" alt="">
    <div class="moss-hero-content">
      <h2 id="dark-heading">Default heading</h2>
    </div>
  </section>

  <!-- Pale cover in mobile overlay mode: still on the picture, so still dark. -->
  <section class="moss-hero" data-hero-tone="light" data-mobile="overlay" id="light-overlay-hero">
    <img src="data:image/gif;base64,R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==" alt="">
    <div class="moss-hero-content">
      <h2 id="light-overlay-heading">Overlay heading</h2>
    </div>
  </section>

  <!-- Pale cover, stacked on mobile with a cover tint: text is BELOW the image
       on the darkened band, so it must stay white. -->
  <section class="moss-hero" data-hero-tone="light" data-cover-color
           style="--moss-cover-color: hsla(203, 73%, 14%, 1)" id="light-stacked-hero">
    <img src="data:image/gif;base64,R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==" alt="">
    <div class="moss-hero-content">
      <h2 id="light-stacked-heading">Stacked heading</h2>
    </div>
  </section>
</body>
</html>`,
  }),
};

// ── A heading's `#` is drawn, not written ────────────────────────────────────
// Selecting a heading and copying it must yield the heading, not "Heading#".
// That was the intent of a one-line `user-select: none` added 2026-05-31, and
// it half-worked for two months: Blink honours it when building the clipboard
// string, WebKit does not. Since moss's own preview is WebKit and Safari is
// WebKit, the people most likely to notice were the ones it never worked for.
//
// The fix moves the glyph out of the document into `::after`. Whether that is
// true is a question ONLY an engine can answer — `getSelection().toString()`
// is the engine's own serializer, and jsdom has no layout, no `::after`
// content, and no selection model to ask. Both engines run here because the
// bug was a disagreement BETWEEN engines; one of them would have passed all
// along.
//
// A built site rather than a probe page, because the second half of this gate
// is about what the emitter emits: a `:::grid` cell's heading is a card title
// and must carry no permalink at all. Served by
// playwright/heading-anchor.config.ts.
export const HEADING_ANCHOR_GATE: ScratchSiteSpec = {
  name: "heading-anchor-gate",
  files: {
    "index.md": `---
title: Heading anchor gate
uid: "hag00101"
---

## Introduction

Body text under the section heading.

:::grid 2
### Card title one

The cell's own text.
+++
### Card title two

The other cell's text.
:::
`,
    ".moss/config.toml": CONFIG_TOML,
    ".moss/theme/style.css": null,
  },
};

// ── The quote card actually paints the page's cover ──────────────────────────
// A reader selects a sentence, taps Share, and gets a PNG whose top 140pt is
// the article's own photograph. For two months it was a blank band instead:
// `findCoverSource()` looked for `.article-cover img`, a class moss has never
// emitted, and every test stayed green because the only test that covered it
// built that element itself in jsdom. It asserted the author's belief about
// moss's markup rather than moss's markup, and jsdom can neither fetch, decode
// nor paint, so nothing in that suite could have noticed.
//
// So the gate has to go all the way to pixels: select text, click Share, read
// the image the browser produced back through a canvas, and sample it. Only an
// engine can do any of that — a `<canvas>` with a real 2D context, a real image
// decode, and a real `drawImage`. jsdom's canvas is a stub that returns
// nothing, and a Rust test can only see the attribute, which is already pinned
// by `share_cover_attr_test.rs`.
//
// Three pages, one per rung of `share_cover_url`, with the SAME body text so
// their cards differ only by the strip:
//   /hero/    — a `:::hero` image           → strip is magenta
//   /covered/ — `cover:` frontmatter, no hero → strip is cyan
//   /plain/   — neither                     → NO strip, even though this page
//               still has an `og:image` (the generated 1200×630 title card).
// The two fill colours are different so a cross-wired lookup — drawing the
// other page's cover, or the og card — reads as the wrong colour rather than
// as a pass. Both are fully saturated and nothing like the card's warm paper
// background (#f5f0e6), so a painted strip is an unmistakable signal.
// Served by playwright/share-card.config.ts.
const HERO_MAGENTA_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="800" height="600" viewBox="0 0 800 600"><rect width="800" height="600" fill="#ff00ff"/></svg>
`;
const COVER_CYAN_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="800" height="600" viewBox="0 0 800 600"><rect width="800" height="600" fill="#00ffff"/></svg>
`;

// Identical on all three pages: the card's height is one of the assertions, so
// everything except the cover strip has to be the same. The middle paragraph is
// the one the gate selects; it is longer than 40 characters, which is the card's
// short/long mode switch, so all three take the same layout path.
const SHARE_CARD_BODY = `The first paragraph gives the card something to fade in above the quote.

A reader highlights this sentence and expects the card to carry the picture.

The last paragraph gives the card something to fade out below the quote.
`;

// A poem and the same words as prose. The card must set the poem on four
// lines and the prose on however many the width dictates — the difference is
// the only thing the height comparison measures.
const POEM_LINES = [
  "Cherry stones",
  "Winter morning",
  "Salt and paper",
  "Nobody answers",
  "The gate is open",
  "Rain on the roof",
  "One lamp burning",
  "Somebody sleeps",
];
const POEM = POEM_LINES.join("\n");
const POEM_AS_PROSE = POEM_LINES.join(" ");

export const SHARE_CARD_GATE: ScratchSiteSpec = {
  name: "share-card-gate",
  // The QR is only emitted for a site that has somewhere to point.
  siteUrl: "https://gate.test",
  files: {
    "hero-cover.svg": HERO_MAGENTA_SVG,
    "fm-cover.svg": COVER_CYAN_SVG,
    "hero.md": `---
title: Hero page
uid: "scg00101"
---

:::hero {image=hero-cover.svg}
:::

${SHARE_CARD_BODY}`,
    "covered.md": `---
title: Covered page
uid: "scg00102"
cover: fm-cover.svg
---

${SHARE_CARD_BODY}`,
    "plain.md": `---
title: Plain page
uid: "scg00103"
---

${SHARE_CARD_BODY}`,
    "poem.md": `---
title: Poem page
uid: "scg00104"
---

${POEM}
`,
    "prose.md": `---
title: Prose page
uid: "scg00105"
---

${POEM_AS_PROSE}
`,
    // A vertical page: the gate reads its card for the vertical layout —
    // portrait, the cover as a landscape band across the top, columns at the
    // right, a reading-end meta block, and a corner QR. Its own body, not
    // SHARE_CARD_BODY: the samples need Han and 「」 in the quote
    // (paragraph 1) and a run short enough for short mode (paragraph 2).
    // Paragraph 3 is orientation bait: 一 is a single horizontal stroke, so
    // its ink is wide-and-short when upright and narrow-and-tall the moment
    // something rotates the glyph — the shape the vertical-card render gate
    // samples for, rather than trusting column geometry alone to notice.
    // Paragraph 4 carries one of each vertical-punctuation treatment in a
    // single 8-character short quote: 「」 rotate, 印/字/曰/年 draw upright
    // and centred, ： must draw upright and centred too (no corner shift —
    // the bug that read as the colon overlapping 印), and 。 corner-shifts.
    "vertical.md": `---
title: 直書頁
uid: "scg00106"
cover: hero-cover.svg
typesetting: vertical
---

第一段落只是引語之前的鋪墊，測試不會選取它。

範例畫家畫魚，魚無水；畫鳥，鳥無枝。題曰「筆墨無多情意多，紙上仍是舊山河」。一八二〇年，畫家六十有五，居範例城墨石山房，以畫易米，人稱之曰石叟。

「石痕，墨跡也。」

一一一一一一一一一一一一一一一一一一一一一一一一一一一一一一一一一一一一一一一一一一一一

印：「字曰年」。
`,
    ".moss/config.toml": CONFIG_TOML,
    // No user theme: this gate is about moss's own defaults.
    ".moss/theme/style.css": null,
  },
};

// ── The narrow-viewport hamburger menu ───────────────────────────────────────
// Six nav pages, which is the point: the menu used to open to a fixed
// `max-height: 200px`, and because `.nav-links` inherits `flex-wrap: wrap` from
// the desktop rule, bounding a COLUMN flex container's height wraps it into
// extra COLUMNS rather than clipping it. Six links rendered as two columns of
// three in both engines. Three pages would not have shown it. Served by
// playwright/nav-mobile.config.ts.
export const NAV_MOBILE_GATE: ScratchSiteSpec = {
  name: "nav-mobile-gate",
  files: {
    "index.md": `---
title: Nav Mobile
uid: "nmg00101"
---

# Nav Mobile

Scratch site for the narrow-viewport nav render gate.
`,
    "About.md": `---
title: About
uid: "nmg00102"
nav: true
---

# About
`,
    "Writing.md": `---
title: Writing
uid: "nmg00103"
nav: true
---

# Writing
`,
    "Projects.md": `---
title: Projects
uid: "nmg00104"
nav: true
---

# Projects
`,
    "Photography.md": `---
title: Photography
uid: "nmg00105"
nav: true
---

# Photography
`,
    "Notes.md": `---
title: Notes
uid: "nmg00106"
nav: true
---

# Notes
`,
    "Contact.md": `---
title: Contact
uid: "nmg00107"
nav: true
---

# Contact
`,
    ".moss/config.toml": CONFIG_TOML,
    // No user theme: this gate is about moss's own defaults.
    ".moss/theme/style.css": null,
  },
};

// ── Footnote `:target` landing (docs/archive/2026-08-24-footnote-landing-fix-plan.md) ──
// Two footnotes are enough to prove both directions of the fix without the
// 152-note stress case (that belongs to the sidenote feature's own gates):
//   - forward, `#fn-1`: a reader following the superscript marker down to the
//     endnote list must land on a line the floating nav island does not cover.
//   - backward, `#fnref-1`: a reader following the endnote's return arrow (or
//     opening the page straight at that fragment, which the gate uses to
//     avoid depending on the island's own scroll-direction heuristic) must see
//     a visible highlight wash telling them which of several numbered lines
//     they returned to.
//
// Two pages, not one: `generate_nav_island` emits nothing at all for a page
// with no breadcrumb trail (island.rs), and a homepage never gets one
// (compute_breadcrumb_segments rule 3) even with `breadcrumb: true` set on
// it — that flag is the site-wide ENABLE, not a per-page trail. So the
// footnotes live on `/notes/`, one level under a `breadcrumb: true` home.
//
// Three things have to hold at once for the forward-jump test to have an
// island to observe, and all three are load-bearing here: the site opts in
// (`CONFIG_TOML_WITH_ISLAND`), the page has a trail, and the page carries two
// `##` sections — since ADR-049 §10 as amended the island shows only where
// there is a contents table to build. Do not thin the Notes page down to one
// section.
//
// A wide, flat banner for the home page: the hero question here is only where
// its right edge lands, so the art is one rectangle at a banner's aspect.
const FOOTNOTE_HERO_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="500" viewBox="0 0 1600 500"><rect width="1600" height="500" fill="#357"/></svg>
`;

// Served by playwright/footnote-target.config.ts.
export const FOOTNOTE_TARGET_GATE: ScratchSiteSpec = {
  name: "footnote-target-gate",
  files: {
    // The gutter is a SITE-wide stylesheet gate (`SiteAssets::has_footnotes`),
    // so a page with no notes of its own still gets whatever the reserve does
    // to the page box. The home page is that page, and it carries a hero
    // because that IS the reported shape: a front page of hero plus card grids
    // on a site whose articles happen to cite sources
    // (docs/archive/2026-08-30-sidenote-gutter-page-kind.md). The front page is
    // now scoped out of the reserve, so the escape itself is proven on
    // /banner/ below, which keeps one.
    "cover.svg": FOOTNOTE_HERO_SVG,
    "index.md": `---
title: Footnote Target Gate
uid: "ftg00101"
breadcrumb: true
---

:::hero {image=cover.svg}
:::

# Footnote Target Gate

Home page for the footnote \`:target\` landing render gate. It carries no
notes of its own — the composition page against which the gutter's sitewide
reach is measured.
`,
    "Notes.md": `---
title: Notes
uid: "ftg00102"
---

# Notes

The first claim carries a note[^1].

Two more sit a few words apart[^3], close enough[^4] that their margin
sidenotes would overlap if \`clear\` were not doing the stacking.

## Filler section

Enough body copy that the endnotes sit well below the fold, so a forward
jump to either one is a real scroll rather than a no-op.

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod
tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim
veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea
commodo consequat.

Duis aute irure dolor in reprehenderit in voluptate velit esse cillum
dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non
proident, sunt in culpa qui officia deserunt mollit anim id est laborum.

## Another section

More filler, same purpose: push the endnotes list far enough from the top
of the document that landing on one is an observable scroll.

Sed ut perspiciatis unde omnis iste natus error sit voluptatem
accusantium doloremque laudantium, totam rem aperiam, eaque ipsa quae ab
illo inventore veritatis et quasi architecto beatae vitae dicta sunt
explicabo.

A late claim near the foot of the document carries the second note[^2] —
far enough down that tapping its marker happens at real scroll depth, and
the backref jump home from note one is a genuinely upward travel.

[^1]: The first note's own text, a couple of lines: short enough that its
    margin copy sits beside the line that cites it and fits on one screen,
    so a marker click at wide width is genuinely travel-free.
[^3]: A short third note, cited close to the fourth.
[^4]: A fourth note, deliberately long enough to be more than one line in the
    margin so the stack it forces with the third is unambiguous: quis autem
    vel eum iure reprehenderit qui in ea voluptate velit esse quam nihil
    molestiae consequatur.
[^2]: The second note's own text, padded the same way so the document stays
    tall past the first note: at vero eos et accusamus et iusto odio
    dignissimos ducimus qui blanditiis praesentium voluptatum deleniti
    atque corrupti quos dolores et quas molestias excepturi sint
    occaecati cupiditate non provident similique sunt in culpa qui
    officia deserunt mollitia animi id est laborum et dolorum fuga et
    harum quidem rerum facilis est et expedita distinctio.
`,
    // The hero-escape page. It carries no notes and is not the home page, so
    // it is the one page of this site that has BOTH a full-bleed banner and a
    // reserved gutter — which is exactly the combination the escape has to
    // survive. The home page cannot stand in: it is scoped out of the reserve,
    // so its banner reaches the viewport edge whether the escape works or not.
    //
    // Its own page rather than a hero added to Notes.md, because a banner
    // there pushes every marker ~700px down the document and two tests above
    // depend on the first marker being on screen (a marker click at wide width
    // must cause no travel) and on the sheet's drag geometry at 1100px.
    "Banner.md": `---
title: Banner
uid: "ftg00103"
---

:::hero {image=cover.svg}
:::

# Banner

A page whose banner must reach the viewport's right edge even though the
sidenote gutter is reserved on it.
`,
    // The ordinary-page case. An ordinary reading page with no notes and no hero, on
    // a site whose stylesheet gate is on because some OTHER page cites a
    // source — which is what nearly every page of a footnoted site actually is,
    // and what no page of this fixture could stand for before this gate was
    // registered. The home page cannot: it is scoped out of the reserve
    // entirely, so it proves nothing about a page that keeps one. /banner/
    // cannot: its hero is the subject. Without this page a real site sat
    // 140px off-centre on nearly every one of its pages through two rounds of
    // gates that were green every time.
    "Plain.md": `---
title: Plain
uid: "ftg00104"
---

# Plain

A page that cites nothing. It must centre exactly as it would on a site with
no footnotes anywhere — the gutter is reserved for notes it does not have, and
a reserved gutter is not by itself a reason to move the page.
`,
    ".moss/config.toml": CONFIG_TOML_WITH_ISLAND,
    ".moss/theme/style.css": null,
  },
};

// ── Notebook contents actually load ──────────────────────────────────────────
// A site with one .ipynb, so the build downloads the pinned JupyterLite
// bundle, copies it to /jupyter/, patches jupyter-lite.json, and writes the
// api/contents/ index. The gate then boots the real JupyterLite app and
// asserts the notebook's cells render — the one place moss's config patch,
// moss's /jupyter/ serving layout, and the bundle exist together. A unit test
// over `patch_jupyterlite_config` cannot see this class of failure: the
// 0.7→0.8 breakage was that the patch was correct about every key it set and
// silent about one it did not (`contentsAllJsonFile`), and the bundle ships
// no contents at all, so a smoke test in jupyterlite-dist has nothing to
// open. Served by playwright/notebook-loads.config.ts.
export const NOTEBOOK_GATE: ScratchSiteSpec = {
  name: "notebook-gate",
  files: {
    "index.md": `---
title: Notebook Gate Site
uid: "ab5de901"
---

# Notebook Gate
`,
    "analysis.ipynb": `${JSON.stringify(
      {
        cells: [
          {
            cell_type: "markdown",
            metadata: {},
            source: [
              "# Notebook Gate\n",
              "\n",
              "moss-notebook-gate-sentinel: the contents index served this cell.\n",
            ],
          },
          {
            cell_type: "code",
            execution_count: null,
            metadata: {},
            outputs: [],
            source: ["print('unexecuted is fine — rendering is the claim')\n"],
          },
        ],
        metadata: {},
        nbformat: 4,
        nbformat_minor: 5,
      },
      null,
      1,
    )}\n`,
    ".moss/config.toml": CONFIG_TOML,
    ".moss/theme/style.css": null,
  },
};

// ── The reading scale is monotonic ───────────────────────────────────────────
// One article page carrying every level of the scale at once. The auto-injected
// `<h1 class="moss-article-title">` sits above the body's own `# H1` — moss
// injects it from the frontmatter title whether or not the body has one — so
// the masthead and the `#`..`####` ladder are measurable in a single render,
// and the spec's `article h1:not(.moss-article-title)` is what separates them.
//
// The site is built `lang = "en"`; the spec sets `documentElement.lang` to
// `zh-Hant` for the CJK half. The only thing that attribute drives is the
// `html[lang^="zh"]` rules in site.css, so toggling it at runtime exercises the
// real selector without a second site and a second server.
// Served by playwright/reading-scale-order.config.ts.
export const READING_SCALE_ORDER_GATE: ScratchSiteSpec = {
  name: "reading-scale-order-gate",
  files: {
    "index.md": `---
title: Reading Scale Order
uid: "rs0001aa"
---

Home page for the reading-scale ordering gate.
`,
    "ladder.md": `---
title: 標題 Masthead Level
uid: "rs0002aa"
---

# 大標 Level One

Body copy 內文 sets the baseline every heading level is measured against.

![圖說 Caption level](swatch.svg)

## 次標 Level Two

### 小標 Level Three

#### 更小標 Level Four

##### 再小標 Level Five

###### 最小標 Level Six

More body copy 內文.
`,
    // A real file, so the figure survives the asset pipeline.
    "swatch.svg": `<svg xmlns="http://www.w3.org/2000/svg" width="64" height="48"><rect width="64" height="48" fill="#6a9a5a"/></svg>
`,
    ".moss/config.toml": CONFIG_TOML,
    ".moss/theme/style.css": null,
  },
};
