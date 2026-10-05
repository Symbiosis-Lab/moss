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
  syntheticBusyImage,
  syntheticTestClip,
} from "./scratch-site";

const CONFIG_TOML = `schema_version = 5

[site]
lang = "en"
`;

/**
 * The same config plus the floating nav island, for the one gate that measures
 * it. The island has been opt-in since a 2026-08-30 amendment, so a
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

// ── Header hit areas + font-trigger focus ring ───────────────────────────────
// Five under-44px header icon buttons (`.nav-theme-btn`, `.nav-search-btn`,
// `.moss-nav-island-sections`, `.font-trigger`, `.mobile-menu-button`) each
// need an invisible hit area reaching 44px without moving the visible pill,
// and `.font-trigger`/`.font-pill button` need the same `:focus-visible` ring
// every other header control gets. This fixture turns all five on at once:
// `search = true` for the search button, `floating_nav = true` plus a
// breadcrumb trail plus two `##` sections for the island and its sections
// button, `nav: true` pages for the hamburger, and a `date:` on the deep
// article for the reading-size control (`.font-trigger` only renders on an
// article page with a date — see `article_date_line_html` in
// build/render/html.rs).
export const HEADER_HIT_AREAS_GATE: ScratchSiteSpec = {
  name: "header-hit-areas-gate",
  files: {
    // `breadcrumb: true` here is the site-wide enable (see the comment on
    // NAV_TOGGLE_CLUSTER_GATE above); the deep article below is what gives
    // it an actual trail to fold.
    "index.md": `---
title: Header Hit Areas
uid: "hha00101"
breadcrumb: true
---

# Header Hit Areas

Scratch site for the header hit-area and focus-ring render gate.
`,
    // Second language: without it, a single-language site puts
    // .nav-search-btn and .nav-theme-btn directly adjacent in .nav-icons —
    // which the gate also has to cover (the worst case for their shared
    // hit-area split), so this fixture is read with .nav-lang-toggle
    // PRESENT and the direct-adjacency case is reasoned about from the fixed
    // 4px .nav-icons gap instead of re-fixturing it.
    "index.zh-hans.md": `---
title: 头部命中区域
uid: "hha00102"
---

# 头部命中区域

用于头部命中区域与焦点环渲染门测试的测试站点。
`,
    // `nav: true` pages populate .nav-links, which is what makes
    // build/components/nav.rs emit .mobile-menu-button at all (has_nav_items).
    "About.md": `---
title: About
uid: "hha00103"
nav: true
---

# About
`,
    "Notes.md": `---
title: Notes
uid: "hha00104"
nav: true
---

# Notes
`,
    // Nested under a nav: true-free folder so the breadcrumb trail has a
    // middle segment, with a date (font-trigger) and two `##` sections (the
    // island only ever shows on a page with a real contents table).
    "Field Reports/index.md": `---
title: Field Reports
uid: "hha00105"
---

# Field Reports
`,
    "Field Reports/Long Form Dispatch.md": `---
title: Long Form Dispatch
uid: "hha00106"
date: 2026-01-15
---

# Long Form Dispatch

Enough text that the page's total scrollHeight clears a 900px viewport by
well over the 400px the gate scrolls down before scrolling back up — nav-
island.ts only reveals the island on a genuine upward scroll, so a page short
enough that scrollTo(400) is a no-op never triggers it.

Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.

Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.

Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque laudantium, totam rem aperiam, eaque ipsa quae ab illo inventore veritatis et quasi architecto beatae vitae dicta sunt explicabo.

## Section Two

More text, so the sections popover has a second entry worth opening.

Nemo enim ipsam voluptatem quia voluptas sit aspernatur aut odit aut fugit, sed quia consequuntur magni dolores eos qui ratione voluptatem sequi nesciunt. Neque porro quisquam est, qui dolorem ipsum quia dolor sit amet, consectetur, adipisci velit.

Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Quis autem vel eum iure reprehenderit qui in ea voluptate velit esse quam nihil molestiae consequatur.

## Section Three

nav-island.ts (\`It()\`) only builds the sections list from the first heading
LEVEL that has two or more IDs, and the page's own \`# Long Form Dispatch\`
title is not one of them — a single \`##\` here reads as a one-entry contents
table and the island suppresses itself (\`r.length < 2\`, removing
\`data-shown\` outright rather than ever setting it \`false\`). A third section
is what makes this fixture's island show at all.

Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque laudantium, totam rem aperiam, eaque ipsa quae ab illo inventore veritatis et quasi architecto beatae vitae dicta sunt explicabo.
`,
    ".moss/config.toml": `schema_version = 5

[site]
lang = "en"
search = true
floating_nav = true
`,
    ".moss/theme/style.css": null,
  },
};

// ── Grid mobile collapse + grid-cell image parity ────────────────────────────
// Three grids on one page, each answering a question only a laid-out engine
// can answer:
//
//   1. a plain `:::grid 3` — collapses to ONE track below 768px;
//   2. a ratio `:::grid 2 1:2` — collapses too. The ratio rides as
//      `--moss-grid-ratio` (an inline `grid-template-columns` would outrank
//      every stylesheet rule and block the collapse), and the desktop
//      assertion (2:1 track widths) proves the variable still drives the
//      layout;
//   3. a two-cell grid whose cells hold the SAME image written two ways — a
//      wikilink embed and a markdown image. One authorial intent, so the two
//      cells must present the same box.
//
// `implicit_figure = false`: the setting must apply to both `![alt](tile.svg)`
// and `![[tile.svg]]`, or one cell keeps a `<figure class="moss-image">` the
// other lacks and every theme rule keyed on `.moss-image` reaches only one
// cell of the pair.
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
// An internal whole-cell image link renders as `a.moss-grid-card[data-kind=
// "link"]` wrapping a `<figure class="moss-image">`; nothing stretches that
// figure on its own, so a lazy image's `sizes="auto, …"` sizes the source
// from the figure's own shrunk-to-content width unless
// `.moss-grid-card :is(.moss-image, picture, img) { inline-size: 100% }`
// (site.css) holds. That needs a real `<figure class="moss-image">` around
// the image, which only exists with moss's default `implicit_figure` (unset
// — true here), unlike GRID_MOBILE_COLLAPSE_GATE above, which turns it off to
// test the plain-`<p>` shape instead — this gate cannot reuse that site. An
// external whole-cell image link takes a different path entirely: it renders as `a.moss-card[data-external]`, and its cover fills
// via `.moss-card-cover`'s flex-stretch + `aspect-ratio` box instead of the
// `.moss-grid-card` rule — both shapes are exercised by this gate's fixture.
// Both writing modes matter: under vertical typesetting the inline axis is
// the box's physical HEIGHT, and the base rule the internal shape has to
// outrank sets a PHYSICAL `height: auto`, which is the inline axis there.
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

// Ten cells over three visible columns: enough that the row genuinely
// scrolls, and — before `.moss-grid[data-scroll]` gained `position:
// relative` — enough that a visually-hidden figcaption (see
// SCROLL_OVERFLOW_THEME_CSS below) landing at its static position against
// the initial containing block could reach far past the viewport, dragging
// the whole document wide with it.
const SCROLL_OVERFLOW_CELLS = Array.from(
  { length: 10 },
  (_, i) => `[![Card ${i + 1}](tile.svg)](https://example.org/card-${i + 1}/)`,
).join("\n+++\n");

// The standard visually-hidden pattern a site theme uses to keep a caption
// readable to assistive tech while removing it from view — scoped to scroll
// rows because that is where a card caption is common, not because the bug
// is scroll-row-specific. Without the fix below this escapes the row's
// `overflow-x: auto` (its containing block is outside the row), which is
// exactly what scroll-row-overflow.spec-level assertions below catch.
const SCROLL_OVERFLOW_THEME_CSS = `.moss-grid[data-scroll] figcaption {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip-path: inset(50%);
}
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
[![第一篇文章](tile.svg)](https://example.org/)
+++
[![第二篇文章：占位標題](tile.svg)](/about/)
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
[![第一篇文章](tile.svg)](https://example.org/)
+++
[![第二篇文章：占位標題](tile.svg)](/about/)
:::

![人形物體載浮載沉](tile.svg)
`,
    "About.md": `---
title: About
uid: "gcis0103"
---

# About
`,
    // A scroll row mixing every direct-child card shape `.moss-grid[data-scroll]`
    // has to size: an internal whole-cell image link (`a.moss-grid-card
    // [data-kind="link"]`), an external whole-cell image link (`a.moss-card
    // [data-external]`, bbb3c0a5's unified shell), a bare link to a page in
    // this build (converted to `a.moss-card` by `apply_collection_cards` — no
    // `.moss-grid-card` wrapper at all, see grid_cells.rs), and a bare
    // external link (also `a.moss-card[data-external]` since bbb3c0a5 —
    // `render_external_card` overrides even a non-whole-cell external link
    // with the unified shell). Four cells over three columns so the row
    // actually scrolls (`GridShortcode::scrolls`) rather than rendering as a
    // plain grid.
    "scroll-row.md": `---
title: Scroll Row Mixed Cards
uid: "gcis0104"
---

:::grid 3 {scroll label="Related"}
[![第一篇文章](tile.svg)](https://example.org/)
+++
[![第二篇文章：占位標題](tile.svg)](/about/)
+++
[About](/about/)
+++
[Elsewhere](https://example.org/elsewhere/)
:::
`,
    // A scroll row long enough to overflow, on a page with nothing else on
    // it: SCROLL_OVERFLOW_THEME_CSS visually hides every card caption, so
    // this page's own document width should never depend on how many cards
    // scroll past.
    "scroll-overflow.md": `---
title: Scroll Row Overflow
uid: "gcis0105"
---

# Scroll Row Overflow

:::grid 3 {scroll}
${SCROLL_OVERFLOW_CELLS}
:::
`,
    // Same row, vertical typesetting: the row's own scroll axis transposes
    // to the block axis (site/vertical.css), but a caption escaping to the
    // initial containing block still lands in physical page coordinates, so
    // the page's own scroll axis (still physically horizontal — see
    // site/vertical.css's scroll-row comment) is the one at risk here too.
    "scroll-overflow-vertical.md": `---
title: Scroll Row Overflow Vertical
uid: "gcis0106"
typesetting: vertical
---

# Scroll Row Overflow Vertical

:::grid 3 {scroll}
${SCROLL_OVERFLOW_CELLS}
:::
`,
    // Owner's rule: a `{scroll}` row whose cells already fit its column
    // count (3 cells over 3 columns here) renders like the plain grid on a
    // wide screen and only becomes a slideshow once the viewport narrows —
    // it must not be pinned to one or the other. Three plain-text cells
    // (no images) keep the geometry the render-gate assertions read off
    // (equal card widths, row scroll metrics) independent of image decode.
    // More cells than scroll-row.ts's MAX_DOT_COUNT (10), so the row gets
    // the sliding (data-indicator="dynamic") dots with their edge sizes.
    "scroll-dots-dynamic.md": `---
title: Scroll Dots Dynamic
uid: "gcis0110"
---

:::grid 3 {scroll label="Many"}
${Array.from({ length: 14 }, (_, i) => `[![Card ${i + 1}](tile.svg)](https://example.org/many-${i + 1}/)`).join("\n+++\n")}
:::
`,
    "scroll-row-fits.md": `---
title: Scroll Row Fits
uid: "gcis0107"
---

:::grid 3 {scroll label="Related"}
One
+++
Two
+++
Three
:::
`,
    // Same row, vertical typesetting: the fits/doesn't-fit switch has to
    // hold on the transposed (block) axis too, at the SAME viewport-width
    // breakpoint site/vertical.css restates rather than a height-based one
    // (see that file's own comment on why it borrows site.css's breakpoint
    // number here).
    "scroll-row-fits-vertical.md": `---
title: Scroll Row Fits Vertical
uid: "gcis0108"
typesetting: vertical
---

:::grid 3 {scroll label="Related"}
One
+++
Two
+++
Three
:::
`,
    ".moss/config.toml": CONFIG_TOML,
    // A theme is what actually surfaces this bug (see
    // SCROLL_OVERFLOW_THEME_CSS above): a real caption-hiding rule, not
    // moss's own defaults, is what leaves a scroll row's captions positioned
    // against the page instead of the row.
    ".moss/theme/style.css": SCROLL_OVERFLOW_THEME_CSS,
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
// A landscape plate for the vertical pages: under vertical-rl the plate's
// physical width is its block size, so a wide image is the shape a squeezed
// section cuts into.
const HERO_LANDSCAPE_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="2400" height="1771" viewBox="0 0 2400 1771"><rect width="2400" height="1771" fill="#357"/><circle cx="120" cy="885" r="80" fill="#fc3"/></svg>
`;
// Enough columns that body's flex row overflows the viewport — the condition
// under which a shrinkable hero gives up its width (a real vertical-writing site, 2026-09-23).
const VERTICAL_BODY = Array.from({ length: 12 }, () =>
  "這是一段用來填滿版面的測試文字。每一段的長度大致相同，足以讓直排的欄位向左延伸好幾列。",
).join("\n\n");

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
    "scroll.svg": HERO_LANDSCAPE_SVG,
    "vertical-plate.md": `---
title: 直排圖版
uid: "hcg00103"
typesetting: vertical
---

:::hero {.plate}
![[scroll.svg]]
:::

${VERTICAL_BODY}
`,
    "vertical-plate-captioned.md": `---
title: 直排圖說
uid: "hcg00104"
typesetting: vertical
---

:::hero {.plate caption="圖版說明文字，直排於圖旁"}
![[scroll.svg]]
:::

${VERTICAL_BODY}
`,
    ".moss/config.toml": CONFIG_TOML,
    // No user theme: this gate is about moss's own defaults.
    ".moss/theme/style.css": null,
  },
};

// ── A rotating hero can be paused ────────────────────────────────────────────
// A multi-image hero crossfades by itself, so WCAG 2.2.2 wants a pause control.
// Whether it pauses the slides, resumes while it still holds focus, and can be
// tapped on a phone (the overlay panel is full width at the block end there and
// once covered it) are all questions about painted layout and the cascade.
// Served by playwright/hero-pause.config.ts.
export const HERO_PAUSE_GATE: ScratchSiteSpec = {
  name: "hero-pause-gate",
  files: {
    "a.svg": HERO_LANDSCAPE_SVG,
    "b.svg": HERO_PORTRAIT_SVG,
    "index.md": `---
title: Rotating hero
uid: "hpg00101"
---

:::hero {mobile=overlay}
![[a.svg]]
![[b.svg]]
# Two pictures

Some words over the pictures, with [a link](https://example.com/a).
:::

Body text.
`,
    "align-end.md": `---
title: Rotating hero, panel at the end
uid: "hpg00102"
---

:::hero {align=end}
![[a.svg]]
![[b.svg]]
# Two pictures

Short.
:::

Body text.
`,
    ".moss/config.toml": CONFIG_TOML,
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
//
// Two pages carry the same `## Introduction` section: the home page, whose
// headings lost the anchor entirely, and `page/`, an ordinary non-home page
// where an author-written `##` heading still gets one. The permalink-
// selection tests run against `page/`, where the anchor exists to be
// selected; the home page's own `h2` is asserted anchor-less separately.
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
    "page/index.md": `---
title: A non-home page
uid: "hag00102"
---

## Introduction

Body text under the section heading.
`,
    ".moss/config.toml": CONFIG_TOML,
    ".moss/theme/style.css": null,
  },
};

// ── The quote card actually paints the page's cover ──────────────────────────
// A reader selects a sentence, taps Share, and gets a PNG whose top 140pt is
// the article's own photograph. A jsdom test that builds the cover element
// itself asserts the author's belief about moss's markup rather than moss's
// markup, and jsdom can neither fetch, decode nor paint.
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

// ── Footnote `:target` landing ──
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
// `##` sections — the island shows only where
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
    // on a site whose articles happen to cite sources. The front page is
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

// ── Content-width escape: `data-width` bands in both writing modes ──────────
// A width token breaks a block out of the reading measure. Where the band
// lands, and whether the text after it clears it, is only visible once an
// engine lays the page out — horizontally, right-to-left, and under
// vertical-rl, where the inline axis runs down the page.
//
// The plate is portrait so its figcaption (horizontal-tb even in a vertical
// page) sits beside it rather than after it. `doc.pdf` is only a box to size:
// the percent case needs an embed kind whose placement carries a size. A
// float shrinks to fit rather than stretching to its margins, so `float-*.md`
// are the cases where the band's own size, not its margins, sets the width.
// `raw.md` is hand-written HTML, the one way a token and a percent still meet
// on one element now that the renderer drops the token for a size.
// Served by playwright/content-width-escape.config.ts.
const ESCAPE_PLATE_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="400" height="800" viewBox="0 0 400 800"><rect width="400" height="800" fill="#357"/></svg>
`;
const ESCAPE_PDF = `%PDF-1.1
1 0 obj<<>>endobj
trailer<<>>
%%EOF
`;
const escapePage = (token: string, vertical: boolean) =>
  vertical
    ? `---
title: 縱 ${token}
typesetting: vertical
---

![縱排圖說|${token}](plate.svg)

第一段。

第二段。
`
    : `---
title: Band ${token}
---

![A caption|${token}](plate.svg)

The first paragraph after the figure.

The second paragraph.
`;

export const CONTENT_WIDTH_ESCAPE_GATE: ScratchSiteSpec = {
  name: "content-width-escape-gate",
  files: {
    ".moss/config.toml": CONFIG_TOML,
    ".moss/theme/style.css": null,
    "plate.svg": ESCAPE_PLATE_SVG,
    "doc.pdf": ESCAPE_PDF,
    "index.md": `---
title: Home
---

Home.
`,
    ...Object.fromEntries(
      ["body", "wide", "page", "screen"].flatMap((t) => [
        [`h-${t}.md`, escapePage(t, false)],
        [`v-${t}.md`, escapePage(t, true)],
      ]),
    ),
    "sized.md": `---
title: Sized
---

![[doc.pdf|A caption|wide|40%]]

The first paragraph after the embed.
`,
    ...Object.fromEntries(
      ["left", "right"].map((side) => [
        `float-${side}.md`,
        `---
title: Floated ${side}
---

![A caption|wide|align-${side}](plate.svg)

The first paragraph after the figure.
`,
      ]),
    ),
    "raw.md": `---
title: Hand-written
---

<figure class="moss-image" data-width="wide" style="width:40%"><img src="/plate.svg" alt=""><figcaption>Hand-written</figcaption></figure>

The first paragraph after the figure.
`,
  },
};

// ── Bare inline video takes its own shape, not a forced 16:9 box ────────────
// `clip.mp4` is 640x540 (DAR 32:27, not 16:9) — a landscape-but-not-widescreen
// shape close to the 1280x1080 clip that motivated this gate, scaled down so
// the fixture stays a few KB. Embedded as a bare `![[clip.mp4]]` wikilink on
// its own line, which moss emits as a direct `<video>` child of `<article>`
// (site.css's `article video` selector, not the `.video-figure` or `article
// figure` paths, which already leave a video's natural size alone). Served
// by playwright/video-embed-shape.config.ts.
//
// A function, not a plain `export const` like every other gate above: every
// gate's config imports from this one module, so a top-level
// `syntheticTestClip()` call here would shell out to ffmpeg — and require it
// on PATH — just from importing gate-sites.ts, even for a config that has
// nothing to do with video. Calling it only inside the one config that needs
// it keeps that cost (and that prerequisite) scoped to this gate alone.
export function videoEmbedShapeGate(): ScratchSiteSpec {
  return {
    name: "video-embed-shape-gate",
    files: {
      "clip.mp4": syntheticTestClip(640, 540),
      "index.md": `---
title: Video Embed Shape Gate
uid: "vsh001a"
---

# Video Embed Shape Test

![[clip.mp4]]
`,
      ".moss/config.toml": CONFIG_TOML,
    },
  };
}

// ── Place-map palette reaches the built page ─────────────────────────────────
// The Rust snapshot suite proves the SVG's `var(--moss-place-*, #fallback)`
// call sites exist; it cannot prove the browser resolves them to the
// *approved* colour rather than silently riding the fallback because
// site.css never defined the token — that needs a real cascade. `location:`
// on a coastal city plus the site-level `locator = "align-right"` default
// (the same key the terms/places design record uses) is enough to make moss
// emit a locator automatically, no embed syntax needed. Served by
// playwright/place-map-tokens.config.ts.
export const PLACE_MAP_TOKENS_GATE: ScratchSiteSpec = {
  name: "place-map-tokens-gate",
  files: {
    "index.md": `---
title: Place Map Tokens Gate
uid: "pmt001a"
location: "Lisbon"
---

# Place Map Tokens Test

A coastal locator, for reading the water layer's resolved colour.
`,
    "taveuni.md": `---
title: Taveuni
uid: "pmt002b"
location: "Taveuni"
---

A place on the antimeridian, where the world map's edge falls.
`,
    ".moss/config.toml": `schema_version = 6

[site]
lang = "en"
locator = "align-right"

[terms.places]
type = "place"
fields = ["location"]
`,
    ".moss/places.toml": `["Lisbon"]
lat = 38.722
lng = -9.139
precision = "city"

["Taveuni"]
lat = -16.85
lng = 179.95
precision = "exact"
`,
  },
};

// ── route: true draws a line, badges and leaders ─────────────────────────────
// Four stops in travel order: Harbor and Harbor Overlook share one
// coordinate, so the badge-offset rule fires and badge 2 carries a leader
// back to it; Inland Reach is region precision, for the hollow badge; Far
// Point is a plain fourth stop. The Rust snapshot suite pins the exact SVG
// bytes this produces; this gate asks what a real browser does with
// them — DOM order (line under markers), badge contrast in both themes, and
// the leader's presence — which no byte comparison can see. Served by
// playwright/place-map-route.config.ts.
export const PLACE_MAP_ROUTE_GATE: ScratchSiteSpec = {
  name: "place-map-route-gate",
  files: {
    "index.md": `---
title: Place Map Route Gate
uid: "pmr001a"
location: [Harbor, Harbor Overlook, Inland Reach, Far Point]
route: true
---

# Place Map Route Test

Four stops, two of them coincident, for reading the drawn route in a real browser.
`,
    ".moss/config.toml": `schema_version = 6

[site]
lang = "en"
locator = "align-right"

[terms.places]
type = "place"
fields = ["location"]
`,
    ".moss/places.toml": `["Harbor"]
lat = 35.0
lng = 135.0
precision = "city"

["Harbor Overlook"]
lat = 35.0
lng = 135.0
precision = "exact"

["Inland Reach"]
lat = 36.0
lng = 136.0
precision = "region"

["Far Point"]
lat = 34.0
lng = 134.5
precision = "city"
`,
  },
};

/**
 * The locator's position relative to the article body's first TEXT block (a
 * paragraph, list or blockquote) — never a heading or media. `heading-first.md`
 * opens with a `##` the locator must skip, staying full column width, with
 * the map's top instead tracking the paragraph that follows it.
 * `media-first.md` does the same with a figure. `paragraph-first.md`,
 * `list-first.md` and `quote-first.md` have no leading block to skip — the
 * map aligns with the body's very first element, as it always has, whatever
 * shape that element is. `no-text.md` has no paragraph, list or blockquote
 * anywhere in its body, so the locator has nothing to sit before and stays
 * at the front, above the (now full-width) heading.
 * Served by playwright/place-map-align.config.ts.
 */
export const PLACE_MAP_ALIGN_GATE: ScratchSiteSpec = {
  name: "place-map-align-gate",
  files: {
    "heading-first.md": `---
title: Heading First
uid: "pma001a"
location: "Lisbon"
---

## Where the story begins

Prose that should flow beside the locator, on its left, once the heading above it has cleared out of the way entirely.
`,
    "paragraph-first.md": `---
title: Paragraph First
uid: "pma002b"
location: "Lisbon"
---

Opening prose, directly after the masthead, with no heading before it.
`,
    "media-first.md": `---
title: Media First
uid: "pma003c"
location: "Lisbon"
---

![A photograph](photo.svg)

Prose that should flow beside the locator, once the photograph above it has cleared out of the way entirely.
`,
    "photo.svg": `<svg xmlns="http://www.w3.org/2000/svg" width="64" height="48"><rect width="64" height="48" fill="#6a7a9a"/></svg>
`,
    "no-text.md": `---
title: No Text
uid: "pma004d"
location: "Lisbon"
---

## Only a heading

***
`,
    "list-first.md": `---
title: List First
uid: "pma005e"
location: "Lisbon"
---

- Opening item, directly after the masthead, with no heading before it.
- A second item.
`,
    "quote-first.md": `---
title: Quote First
uid: "pma006f"
location: "Lisbon"
---

> Opening quote, directly after the masthead, with no heading before it.
`,
    // Vertical typesetting (page-level `typesetting:`): the locator is a
    // column-block in the flow, not a float. `vertical-article` has a title
    // and meta columns before the text; `vertical-front/` is a front page —
    // a short body, then folder cards and a listing.
    "vertical-article.md": `---
title: Quiet Harbours
uid: "pma007g"
author: Ines Moreau
date: 2026-03-14
location: "Lisbon"
typesetting: vertical
---

The ferry leaves before the light is fully up, and the first hour is only water and the sound of the engine.

A second paragraph keeps the column going, so the page has more than one block of text after the opening.
`,
    // The locator inside a themed wrapper that centres its children along the
    // column (a flex column with `align-items: center`): it must still start
    // at the columns' top.
    "vertical-plate.md": `---
title: Plate Page
uid: "pma013m"
location: "Lisbon"
typesetting: vertical
---

::: {.plate}
![A photograph](photo.svg)

Caption line under the photograph.
:::

Prose after the plate.
`,
    ".moss/theme/style.css": `body[data-typesetting="vertical"] .plate {
  display: flex;
  flex-direction: column;
  align-items: center;
}
`,
    "vertical-front/index.md": `---
title: Harbour Notes
uid: "pma008h"
location: "Lisbon"
typesetting: vertical
---

A short opening for the front page.
`,
    "vertical-front/north/index.md": `---
title: North Quay
uid: "pma009i"
---

Quay notes.
`,
    "vertical-front/south/index.md": `---
title: South Quay
uid: "pma010j"
---

More quay notes.
`,
    "vertical-front/tide-tables.md": `---
title: Tide Tables
uid: "pma011k"
---

Tables.
`,
    "vertical-front/lighthouse-log.md": `---
title: Lighthouse Log
uid: "pma012l"
---

A log.
`,
    ".moss/config.toml": `schema_version = 6

[site]
lang = "en"
locator = "align-right"

[terms.places]
type = "place"
fields = ["location"]
`,
    ".moss/places.toml": `["Lisbon"]
lat = 38.722
lng = -9.139
precision = "city"
`,
  },
};

// ── Preview server ↔ static build layout parity ──────────────────────────────
// One page combining every shape the preview server's own injected CSS
// (`PREVIEW_CHEAP_REFLOW_STYLE` in iframe_bridge.rs) can touch: a scroll row
// with enough cards that most start outside the row's own visible area, a
// wrapping (non-scroll) grid of images, standalone figures well below the
// fold, a tabular embed, a hero, and enough filler prose between them that
// the later media are genuinely off-screen at first paint. None of the
// existing gates combine all of these on one page — GRID_CARD_IMAGE_INLINE_
// SIZE_GATE's scroll rows are the closest relative, but its cards are a
// 40×30 probe size chosen to catch a different (inline-size) regression, and
// its pages carry no hero, wrapping grid, or embed alongside the row.
//
// 22 cards: the exact count the be18a723 regression measured (a 669px
// preview row against a 269px published one) — enough over 4 visible
// columns that most cards have never been laid out at first paint.
const PARITY_SQUARE_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="300" height="300" viewBox="0 0 300 300"><rect width="300" height="300" fill="#6a8caf"/></svg>
`;
const PARITY_FIGURE_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="800" height="450" viewBox="0 0 800 450"><rect width="800" height="450" fill="#8a6a4f"/><circle cx="640" cy="360" r="60" fill="#e8d9a0"/></svg>
`;
const PARITY_HERO_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="900" viewBox="0 0 1600 900"><rect width="1600" height="900" fill="#3a5a6a"/><circle cx="1360" cy="180" r="90" fill="#f2e4a0"/></svg>
`;

// Placeholder CJK prose (invented for this fixture, no real source) padding
// the page so later media start below the fold — this gate's whole point is
// content-visibility behaviour on off-screen media, which a short page could
// never exercise.
const PARITY_FILLER_LINE =
  "第一段。這是為了撐開版面高度而寫的占位文字，讓下面的圖片在頁面剛載入時還在畫面之外。";

function parityFiller(paragraphs: number): string {
  return Array.from({ length: paragraphs }, (_, i) => `${PARITY_FILLER_LINE}（第 ${i + 1} 段）`).join(
    "\n\n",
  );
}

const PARITY_SCROLL_ROW_CELLS = Array.from(
  { length: 22 },
  (_, i) => `![Card ${i + 1}](tile.svg)`,
).join("\n+++\n");

const PARITY_WRAP_GRID_CELLS = Array.from({ length: 6 }, (_, i) => `![Wrap cell ${i + 1}](tile.svg)`).join(
  "\n+++\n",
);

export const PREVIEW_PARITY_GATE: ScratchSiteSpec = {
  name: "preview-parity-gate",
  files: {
    "tile.svg": PARITY_SQUARE_SVG,
    "figure.svg": PARITY_FIGURE_SVG,
    "hero.svg": PARITY_HERO_SVG,
    // A tabular embed (`![[data.csv]]` → `.moss-embed.moss-embed-table`,
    // resolved by moss-core's csv_table renderer) — the fourth shape
    // PREVIEW_CHEAP_REFLOW_STYLE's selector names alongside figures, bare
    // images, and generic embeds.
    "data.csv": `label,value\nAlpha,12\nBeta,7\nGamma,19\nDelta,3\nEpsilon,25\n`,
    "index.md": `---
title: Preview Parity Gate
uid: "ppg001aa"
---

:::hero {image=hero.svg caption="Placeholder hero plate for the preview-parity gate"}
:::

${parityFiller(3)}

![A standalone figure near the top of the article](figure.svg)

${parityFiller(3)}

:::grid 3 {.no-cards}
${PARITY_WRAP_GRID_CELLS}
:::

${parityFiller(3)}

![[data.csv]]

${parityFiller(3)}

:::grid 4 {scroll label="Related"}
${PARITY_SCROLL_ROW_CELLS}
:::

${parityFiller(3)}

![A second standalone figure, well below the fold on first paint](figure.svg)

${parityFiller(3)}
`,
    ".moss/config.toml": CONFIG_TOML,
    // No user theme: this gate is about the preview server's own injected
    // CSS against moss's own defaults, not a cascade contract.
    ".moss/theme/style.css": null,
  },
};

// ── Lightbox close/nav ring, GitHub link shape ───────────────────────────────
// Two controls the first chrome-state audit's fixture never exercised:
//
//   - `.lightbox-close`/`.lightbox-nav` had no `:focus-visible` rule of
//     their own and fell back to the browser default — a bulky
//     rounded-rectangle box (measured in Chrome) around a bare glyph with no
//     background or radius of its own, nothing else in the lightbox's own
//     chrome language. Reaching it needs the real keyboard path: a media
//     item is a `tabindex="0"` `<figure>`, Enter opens it, and Tab from
//     there reaches the close button — a mouse click anywhere first (the
//     obvious shortcut) flips the page's input-modality heuristic and makes
//     every focus after it, even a programmatic one, resolve `:focus-visible`
//     false, which is a false negative about the control, not a finding
//     about it.
//   - `.github-link` carried no `border-radius`, so its hover fill painted a
//     hard-cornered square card behind a mark that reads as a circle (the
//     octocat glyph a real site supplies, mirrored here with a plain
//     placeholder circle so the fixture carries no third-party mark) — the
//     state-region-does-not-match-the-control-shape bug this whole audit
//     was written to catch, on the one header control moss ships as CSS
//     only (grep confirms no Rust call site emits `.github-link`; a real
//     site opts in with its own raw HTML, which is what this fixture does).
//
// Video-collection marker: an `<video>` embed immediately followed by
// `<!-- video-meta -->` becomes a media-collection page's own lightbox
// (media_collection.rs) — `videos/index.html`, served by
// playwright/lightbox-github-shapes.config.ts.
export function lightboxGithubShapesGate(): ScratchSiteSpec {
  return {
    name: "lightbox-github-shapes-gate",
    files: {
      "clip.mp4": syntheticTestClip(640, 360),
      "index.md": `---
title: Lightbox GitHub Shapes Gate
uid: "lgs001a"
---

# Lightbox / GitHub Link Shapes

A clip for the media-collection lightbox.

![[clip.mp4]]
<!-- video-meta: dispatch -->

A placeholder mark standing in for a real site's own GitHub logo — round,
so a square hover fill or focus ring behind it is the bug this gate exists
to catch.

<a class="github-link" href="https://example.org/repo" aria-label="Project repository"><svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" aria-hidden="true"><circle cx="10" cy="10" r="9" fill="currentColor"/></svg></a>
`,
      ".moss/config.toml": CONFIG_TOML,
      ".moss/theme/style.css": null,
    },
  };
}

// ── :::grid coverless-card quote treatment ───────────────────────────────────
// A hand-picked `:::grid` cell for a page with no cover, sitting beside
// siblings that DO have one, must read as a card rather than the bare
// `.moss-card-cover.moss-card-no-cover` placeholder box — the same upgrade
// `render_list_with_typesetting` already gives a coverless card in a mixed
// AUTO-generated listing (see `grid_card.rs::render_item`'s `list_has_covers`
// doc: the placeholder becomes a `data-cover="quote"` slot carrying the
// page's description, or its title with none). `apply_collection_cards`
// (grid_cells.rs) used to render every `:::grid` cell in isolation and never
// computed that flag, so the mixed-row upgrade never reached a hand-picked
// grid at all. Three wikilink cells, two covered, one not: the minimal mixed
// row. Served by playwright/grid-card-no-cover.config.ts.
const GRID_NO_COVER_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="400" height="300" viewBox="0 0 400 300"><rect width="400" height="300" fill="#5577aa"/></svg>
`;

export const GRID_CARD_NO_COVER_GATE: ScratchSiteSpec = {
  name: "grid-card-no-cover-gate",
  files: {
    "cover.svg": GRID_NO_COVER_SVG,
    "index.md": `---
title: Grid No Cover
uid: "gnc00101"
---

:::grid 3
[[Card One]]
+++
[[Card Two]]
+++
[[Card Three]]
:::
`,
    "Card One.md": `---
title: Card One
uid: "gnc00102"
cover: cover.svg
---

Body one.
`,
    // The one cell with no cover of its own — filename matches the
    // `[[wikilink]]` above verbatim (Obsidian-style fuzzy matching,
    // content_graph.rs), not a lookup against `title:` frontmatter.
    "Card Two.md": `---
title: Card Two
uid: "gnc00103"
---

Body two, no cover.
`,
    "Card Three.md": `---
title: Card Three
uid: "gnc00104"
cover: cover.svg
---

Body three.
`,
    ".moss/config.toml": CONFIG_TOML,
    ".moss/theme/style.css": null,
  },
};

// ── Footer touch targets: real footer.md vs. the generated fallback ──
//
// The default footer link list (`.footer-link`, generated when no footer.md
// exists) is not what a real site with an authored footer renders. footer.md
// goes through the ordinary markdown pipeline into plain, classless anchors —
// a markdown list becomes `<ul><li><a>…</a></li></ul>`, and a sentence
// becomes an inline `<a>` in running text — so a touch-target rule scoped to
// `.footer-link` alone reaches the fallback and misses exactly the reported
// case. Both shapes are exercised here, in one footer.md: a list (the "links
// inside a list item" case) and a sentence (the "links inside a sentence"
// case, where the fix must not turn the anchor into a block or force a line
// break).
export const TOUCH_TARGETS_FOOTER_MD_GATE: ScratchSiteSpec = {
  name: "touch-targets-footer-md-gate",
  files: {
    "index.md": `---
title: Footer Touch Targets (footer.md)
uid: "ttf00101"
---

# Footer Touch Targets

Scratch site for the footer render gate, with an authored footer.md.
`,
    "footer.md": `- [About](/about)
- [Contact](/contact)

Written by [an editor](/editor) for this scratch site.
`,
    ".moss/config.toml": CONFIG_TOML,
    ".moss/theme/style.css": null,
  },
};

// The generated fallback: no footer.md, one page opts into the default
// footer link list via \`footer: true\` frontmatter (nav.rs's own condition
// for populating it). Kept as its own site rather than a second case in the
// footer.md site above, since footer.md's presence — not a toggle — is what
// selects between the two renderers (footer.rs: an explicit \`slot:
// footer-left\` file always wins over footer.md, and footer.md is the only
// reserved name; there is no config flag that picks the fallback on a site
// that also has a footer.md).
export const TOUCH_TARGETS_FOOTER_FALLBACK_GATE: ScratchSiteSpec = {
  name: "touch-targets-footer-fallback-gate",
  files: {
    "index.md": `---
title: Footer Touch Targets (fallback)
uid: "ttf00201"
---

# Footer Touch Targets

Scratch site for the footer render gate, with no footer.md — the generated
default footer link list.
`,
    "About.md": `---
title: About
uid: "ttf00202"
footer: true
---

# About
`,
    ".moss/config.toml": CONFIG_TOML,
    ".moss/theme/style.css": null,
  },
};

// ── Places explorer ───────────────────────────────────────────────────────
// Twelve located works spread across three countries, including one
// coincident pair (two real points in the same small area, ~1km apart) that
// can never separate by zooming — the ring gate's own subject. Dates are
// strictly descending so the cards gate can assert row order without a tie.
// Served by playwright/places-explorer-{boot,camera,ring,cards}.config.ts.
const PLACES_EXPLORER_COVER_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="200" height="140" viewBox="0 0 200 140"><rect width="200" height="140" fill="#6a8caf"/></svg>
`;

// Covers of other shapes, so a card row whose images keep their natural
// heights shows up: one tall, one wide.
const PLACES_EXPLORER_TALL_COVER_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="60" height="240" viewBox="0 0 60 240"><rect width="60" height="240" fill="#af8c6a"/></svg>
`;
const PLACES_EXPLORER_WIDE_COVER_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="400" height="60" viewBox="0 0 400 60"><rect width="400" height="60" fill="#8caf6a"/></svg>
`;

function placesExplorerWork(uid: string, title: string, location: string, date: string, cover = "cover.svg", byline = "Field notes", author?: string): string {
  const authorLine = author ? `\nauthor: "${author}"` : "";
  return `---
title: ${title}
uid: "${uid}"
location: "${location}"
date: ${date}${authorLine}
byline: "${byline}"
description: "A short note from ${title}."
cover: ${cover}
---

# ${title}

A short note from ${title}.
`;
}

/**
 * A located work with NO byline, cover, date or description — the sparse
 * shape `places_data.rs` omits those four keys from the wire for
 * (`#[serde(skip_serializing_if = ...)]`), which once crashed the
 * explorer's first render (`work.byline.length` on an `undefined` byline)
 * before any card painted. Exists so the boot gate below has a real work
 * this bug would have failed on, not a fixture only exercising the
 * fully-authored path every other `placesExplorerWork` entry takes.
 */
function placesExplorerSparseWork(uid: string, title: string, location: string): string {
  return `---
title: ${title}
uid: "${uid}"
location: "${location}"
---

# ${title}
`;
}

export const PLACES_EXPLORER_GATE: ScratchSiteSpec = {
  name: "places-explorer-gate",
  files: {
    "cover.svg": PLACES_EXPLORER_COVER_SVG,
    "cover-tall.svg": PLACES_EXPLORER_TALL_COVER_SVG,
    "cover-wide.svg": PLACES_EXPLORER_WIDE_COVER_SVG,
    "places/index.md": `---
title: Places
---

Every work this site locates, gathered on one map.
`,
    // A style:map embed (task 6, "the embed's own build side"): a non-root
    // place, so this figure gets the embed-hydration handshake rather than
    // the root's own in-place explorer upgrade.
    "lisbon-overview.md": `---
title: Lisbon overview
---

![[/places/lisbon/|style:map]]
`,
    // The hero (task 6, step 10): style:map with the existing \`screen\`
    // placement, as the page's first block — a dedicated page, not
    // places/index.md itself, so this gate never depends on whatever that
    // page's own heading does. Targets a sub-place (Lisbon), not the bare
    // namespace root: a root-targeted \`style:map\` embed resolves its
    // listing through the SAME folder-children selector an ordinary
    // folder embed uses (\`select_children_by_slug\`), which finds no
    // direct children for the bare root in this fixture (every located
    // work claims its OWN place key via \`also_in\`, never the namespace
    // root itself) — a pre-existing gap outside this task's scope, noted
    // in the report rather than fixed here.
    "hero.md": `---
title: World
---

![[/places/lisbon/|style:map|screen]]

The hero above is Lisbon's own map, full-bleed.
`,
    // One footnote anywhere makes the side-note stylesheet ship site-wide, and
    // with it body's `padding-right: var(--moss-sidenote-inset)` between 1216
    // and 1316px: the full-width map must still reach the window's right edge.
    "notes.md": `---
title: Notes
---

A claim with a source.[^1]

[^1]: The source.
`,
    "lisbon-walk.md": placesExplorerWork("pex001aa", "Lisbon Walk", "Lisbon", "2024-06-10", "cover.svg", "Photographs: Cy", "Ana"),
    "lisbon-harbor-light.md": placesExplorerWork("pex002bb", "Lisbon Harbor Light", "Lisbon Harbor", "2024-06-05", "cover-tall.svg"),
    "porto-steps.md": placesExplorerWork("pex003cc", "Porto Steps", "Porto", "2024-05-20", "cover-wide.svg"),
    "coimbra-library.md": placesExplorerWork("pex004dd", "Coimbra Library", "Coimbra", "2024-05-01"),
    "portugal-overview.md": placesExplorerWork("pex013mm", "Portugal Overview", "Portugal", "2024-07-01", "cover.svg", "Field notes", "Alexandria Papadopoulos Konstantinou"),
    "kyoto-garden.md": placesExplorerWork("pex005ee", "Kyoto Garden", "Kyoto", "2024-04-15"),
    "osaka-market.md": placesExplorerWork("pex006ff", "Osaka Market", "Osaka", "2024-04-01"),
    "tokyo-crossing.md": placesExplorerWork("pex007gg", "Tokyo Crossing", "Tokyo", "2024-03-10"),
    "nara-deer-park.md": placesExplorerWork("pex008hh", "Nara Deer Park", "Nara", "2024-02-20"),
    "lima-coastline.md": placesExplorerWork("pex009ii", "Lima Coastline", "Lima", "2024-01-15"),
    "cusco-terraces.md": placesExplorerWork("pex010jj", "Cusco Terraces", "Cusco", "2023-12-01"),
    "arequipa-volcano.md": placesExplorerWork("pex011kk", "Arequipa Volcano", "Arequipa", "2023-11-10"),
    "iquitos-river.md": placesExplorerWork("pex012ll", "Iquitos River", "Iquitos", "2023-10-01"),
    // Its own location, not a reuse of another work's: `places-explorer-ring.spec.ts`
    // relies on Lisbon's pair being the fixture's ONE coincident 2-count
    // cluster — reusing a located work's exact coordinates here would quietly
    // mint a second one.
    "faro-notes.md": placesExplorerSparseWork("pex014nn", "Faro Notes", "Faro"),
    // Two places far from every other work and a few tens of km apart: at the
    // article's own framing they sit closer than the cluster distance, and
    // still get one marker each.
    "fjord-crossing.md": `---
title: Fjord Crossing
uid: "pex015oo"
location:
  - "Bergen"
  - "Os"
date: 2023-09-01
description: "A short note from the fjords."
---

# Fjord Crossing

A short note from the fjords.
`,
    ".moss/config.toml": `schema_version = 6

[site]
lang = "en"
locator = "align-right"

[terms.places]
type = "place"
fields = ["location"]
`,
    ".moss/places.toml": `["Bergen"]
lat = 60.3913
lng = 5.3221
precision = "city"

["Os"]
lat = 60.19
lng = 5.47
precision = "region"

["Lisbon"]
lat = 38.722
lng = -9.139
precision = "exact"

["Lisbon Harbor"]
lat = 38.715
lng = -9.145
precision = "exact"

["Portugal"]
lat = 37.0139
lng = -7.9303
precision = "country"

["Porto"]
lat = 41.1579
lng = -8.6291
precision = "city"
parent = "Portugal"

["Coimbra"]
lat = 40.2033
lng = -8.4103
precision = "city"
parent = "Portugal"

["Kyoto"]
lat = 35.0116
lng = 135.7681
precision = "city"

["Osaka"]
lat = 34.6937
lng = 135.5023
precision = "city"

["Tokyo"]
lat = 35.6762
lng = 139.6503
precision = "city"

["Nara"]
lat = 34.6851
lng = 135.8048
precision = "city"

["Lima"]
lat = -12.0464
lng = -77.0428
precision = "city"

["Cusco"]
lat = -13.5319
lng = -71.9675
precision = "city"

["Arequipa"]
lat = -16.4090
lng = -71.5375
precision = "city"

["Iquitos"]
lat = -3.7437
lng = -73.2516
precision = "city"

# Reykjavik's real coordinate under the "Faro" key, not Faro's own — a
# gazetteer edit elsewhere (the "Portugal" country entry above) landed
# almost exactly on Faro's real-world point, quietly minting a second
# coincident 2-count cluster the ring gate's own "ONE coincident pair"
# assumption did not expect. Keeping the real coordinate under this key
# again would only reintroduce that risk the moment some other entry
# nearby moves; a continent away is immune to it by construction.
["Faro"]
lat = 64.1466
lng = -21.9426
precision = "city"
`,
    ".moss/theme/style.css": null,
  },
};

// ── Places explorer labels ────────────────────────────────────────────────
// Two works, each placed at a REAL Natural Earth populated-place coordinate
// (confirmed against the embedded pack: Tokyo 35.687N 139.749E rank 0, Osaka
// 34.752N 135.458E rank 1) under its OWN gazetteer name. "Tokyo Shibuya"
// names the same city Natural Earth's own "Tokyo" label does (own-place
// rule: a sub-area name still counts as its city) — the positive case.
// "Namba District" sits at Osaka's own coordinate but names an unrelated
// place — the negative case the design calls out by name (a marker merely
// covering a label's point must drop it, never wear its name). The two
// points are ~400km apart, far enough that a camera centred on either one
// alone never has the other's marker anywhere near its own label's
// coincidence radius. Served by playwright/places-explorer-labels.config.ts.
export const PLACES_EXPLORER_LABELS_GATE: ScratchSiteSpec = {
  name: "places-explorer-labels-gate",
  files: {
    "cover.svg": PLACES_EXPLORER_COVER_SVG,
    "places/index.md": `---
title: Places
---

Every work this site locates, gathered on one map.
`,
    "tokyo-crossing.md": placesExplorerWork("pel001aa", "Tokyo Crossing", "Tokyo Shibuya", "2024-06-10"),
    "kawasaki-waterfront.md": placesExplorerWork("pel002bb", "Kawasaki Waterfront", "Namba District", "2024-05-01"),
    ".moss/config.toml": `schema_version = 6

[site]
lang = "en"

[terms.places]
type = "place"
fields = ["location"]
`,
    ".moss/places.toml": `["Tokyo Shibuya"]
lat = 35.687
lng = 139.749
precision = "exact"

["Namba District"]
lat = 34.752
lng = 135.458
precision = "exact"
`,
    ".moss/theme/style.css": null,
  },
};

// ── Places explorer: a realistic root ─────────────────────────────────────
// Four shapes a short, English, authored-root fixture (PLACES_EXPLORER_GATE
// above) never exercises, one richer site rather than four narrow ones (the
// standing "keep the number of fixture sites small" rule):
//   1. A SYNTHETIC root — no "places/index.md" anywhere, the places
//      namespace declared only in `.moss/config.toml` and reached only
//      through works' own `location:` fields.
//   2. Long, multi-link markdown bylines — several contributors, each a
//      `[Name](url)` link, the shape a real credits line is actually
//      authored in, not a bare string.
//   3. A three-level hierarchy (Japan -> Kansai -> Kyoto) whose MIDDLE rung
//      (Kansai, a region) has no work of its own — only Kyoto, a city
//      under it, does — and whose TOP rung (Japan) has no gazetteer row at
//      all, a name-only grouping level reachable solely by Kansai's own
//      `parent =` reference.
//   4. The whole site in zh-Hant, the language bucket the label layer's
//      CJK rung draws from. Tokyo and Osaka sit at their REAL Natural
//      Earth coordinates (confirmed against the embedded pack, same as
//      PLACES_EXPLORER_LABELS_GATE above) so the labels assertion exercises
//      the real pack, not a stub.
// Served by playwright/places-explorer-real.config.ts.
function placesExplorerRealWork(uid: string, title: string, location: string, date: string, bylineBlock: string, cover?: string): string {
  const coverLine = cover ? `\ncover: ${cover}` : "";
  return `---
title: ${title}
uid: "${uid}"
location: "${location}"
date: ${date}
byline: |
${bylineBlock}${coverLine}
---

# ${title}

A short note from ${title}.
`;
}

export const PLACES_EXPLORER_REAL_GATE: ScratchSiteSpec = {
  name: "places-explorer-real-gate",
  files: {
    "cover.svg": PLACES_EXPLORER_COVER_SVG,
    // A raster cover — `syntheticBusyImage` produces a real PNG, not an
    // SVG. Every other covered fixture in this file (PLACES_EXPLORER_GATE
    // included) uses an SVG cover, which `resolve_cover` always resolved
    // correctly: the raster `.webp`-guessing bug this fixture pins never
    // had a PNG/JPEG cover anywhere in the suite to fail on.
    "cover.png": syntheticBusyImage(200, 140),
    // No "places/index.md" — defect 1's own shape: the root is synthesized
    // from the term index alone.
    //
    // The title is long CJK text, not "Kyoto Garden" — the collapsed card's
    // two-line clamp needs a title that actually overflows it to pin the
    // clamp/unclamp split; a short English title never would have.
    "kyoto-garden.md": placesExplorerRealWork(
      "per001aa",
      "京都一座隱藏在竹林深處的古老庭園與其悠長的歷史故事",
      "Kyoto",
      "2024-04-15",
      "  作者　[黃毛](/people/huang-mao/)\n  編輯　[蘇美智](/people/su-meizhi/)\n  首發媒體　[遠聲媒體](https://example.org/a)\n",
      "cover.png",
    ),
    "tokyo-crossing.md": placesExplorerRealWork(
      "per002bb",
      "Tokyo Crossing",
      "Tokyo",
      "2024-04-01",
      "  作者　[周一](/people/zhou-yi/)\n",
    ),
    "osaka-market.md": placesExplorerRealWork(
      "per003cc",
      "Osaka Market",
      "Osaka",
      "2024-03-10",
      "  作者　[林二](/people/lin-er/)\n",
    ),
    // `footer: true` is this fixture's only footer-worthy content (no
    // footer.md, no RSS) — without it `strip_empty_footer` removes the
    // `<footer>` element outright, and the synthetic root's own "the map's
    // bottom edge meets the footer's top with no gap" gate (fix 1, design
    // decision 7) would have nothing to measure against. `nav: false`
    // keeps it out of the main nav — a bare top-level page there flips
    // `has_content_folders`, which changed `places/kyoto/`'s own breadcrumb
    // mode and broke an unrelated, pre-existing assertion the first time
    // this page was added without it.
    "about.md": `---
title: About
uid: "per004dd"
footer: true
nav: false
---

# About
`,
    ".moss/config.toml": `schema_version = 6

[site]
lang = "zh-hant"

[terms.places]
type = "place"
fields = ["location"]
title = "地點"
`,
    ".moss/places.toml": `["Kyoto"]
lat = 35.0116
lng = 135.7681
precision = "city"
parent = "Kansai"

["Kansai"]
lat = 34.75
lng = 135.5
precision = "region"
parent = "Japan"

# Japan itself has NO row here — only Kansai's "parent = Japan" link names
# it. This is fix 2's own shape: a grouping level with no gazetteer entry
# at all, reachable solely by reference, which used to leave Kyoto/Kansai
# stuck at the top level instead of nested under it.

# Real Natural Earth populated-place coordinates (confirmed against the
# embedded pack — see PLACES_EXPLORER_LABELS_GATE above), own gazetteer
# names, no parent: the labels assertion needs real pack entries at these
# exact points to exist, not a fixture-only name.
["Tokyo"]
lat = 35.687
lng = 139.749
precision = "exact"

["Osaka"]
lat = 34.752
lng = 135.458
precision = "exact"
`,
    ".moss/theme/style.css": null,
  },
};

// ── Overlay text needs a panel, and a hero grows to fit it ───────────────────
// A real decoded image this time, not a hand-set `data-hero-tone` — the panel
// colour comes from `color_extract::panel_background`, which reads the
// image's own scan-cached dominant colour, so a probe page (a given
// attribute, no real pixels) would test nothing about the thing that can
// actually go wrong: a tint computed from the wrong image, or a panel too
// transparent to carry the text it sits on. `busy.png` is a black grid on
// yellow spanning every corner, so no crop finds calm ground for the text —
// the exact failure the panel exists to survive. The paragraph is long
// enough, and `mobile=overlay` narrow enough, to overflow the old fixed-height
// clipped frame; that overflow is the second defect this gate pins.
// Served by playwright/hero-overlay-legibility.config.ts.
export const HERO_OVERLAY_LEGIBILITY_GATE: ScratchSiteSpec = {
  name: "hero-overlay-legibility-gate",
  files: {
    "busy.png": syntheticBusyImage(1600, 900),
    "index.md": `---
title: Hero overlay legibility gate
uid: "hol00101"
---

:::hero {image=busy.png mobile=overlay}
# Overlay on a busy backdrop

This hero demonstrates why overlay text needs its own backing panel rather
than sitting directly on a photograph. A busy image carries drawn lines in
every corner, so no amount of careful cropping finds calm ground for the
words to rest on. The panel behind this paragraph is tinted from the image
itself, so it reads as part of the picture rather than a grey box dropped on
top of it, and it stays readable in both light and dark mode whatever the
photo beneath happens to be. The box beneath this text grows tall enough to
hold every line of it without clipping any of them off, however long the
paragraph runs or however narrow the column gets on a phone. Read more
about [the fix](https://example.com/hero-fix).
:::

Body text below the hero.
`,
    // A short overlay on an align=end hero: covers two things the long
    // overlay above can't. (1) The panel hugs its own content rather than
    // the hero's full height -- a long overlay's panel could still cover
    // most of the frame by genuinely NEEDING that much height for its
    // words, so only a short one proves the panel isn't stretching to fill
    // space it doesn't need. (2) align=end moves the panel to the
    // inline-end edge, for a subject the crop can't move out from under it.
    "align-end.md": `---
title: Hero align end gate
uid: "hol00102"
---

:::hero {image=busy.png align=end}
# Short heading

One line of overlay text.
:::

Body text below the hero.
`,
    // Image-only: no overlay text, so the scrim never paints at all (search
    // data-page="home" -- no, search ":has()" above) -- the ground truth
    // for "undarkened pixel" the align=end scrim-direction test compares
    // against, same image and same crop so only the scrim differs.
    "plain.md": `---
title: Hero plain gate
uid: "hol00103"
---

:::hero {image=busy.png}
:::

Body text below the hero.
`,
    ".moss/config.toml": CONFIG_TOML,
    // A full-bleed, opaque, body-level layer -- the shape a real theme's
    // background treatment takes. .moss-hero's grow-to-fit media sits at
    // z-index: -1 so it paints behind the hero's own (in-flow) overlay
    // text; without isolation: isolate on .moss-hero containing that,
    // -1 is relative to the WHOLE page, not just the hero, and this layer
    // (at the default stacking level, effectively 0) would paint over it.
    ".moss/theme/style.css": `body::before { content: ""; position: fixed; inset: 0; background: #fff; z-index: 0; }`,
  },
};

/**
 * A horizontal site whose places root holds exactly one work: a located home
 * page with a chapter and no cover. The initial frame has nothing to compare
 * the lone marker against, so it must still keep it clear of the card row and
 * the chip, and no card may open before a click. Served by
 * playwright/places-explorer-single.config.ts.
 */
export const PLACES_EXPLORER_SINGLE_GATE: ScratchSiteSpec = {
  name: "places-explorer-single-gate",
  files: {
    "places/index.md": `---
title: Places
---

Every work this site locates, gathered on one map.
`,
    "voyage/voyage.md": `---
title: The Long Voyage
uid: "pes001aa"
location: "Cambridge"
date: 2024-06-10
---

A work with chapters and no description of its own.
`,
    "voyage/first-chapter.md": `---
title: First Chapter
uid: "pes002bb"
---

The first chapter.
`,
    ".moss/config.toml": `schema_version = 6

[site]
lang = "en"
locator = "align-right"

[terms.places]
type = "place"
fields = ["location"]
`,
    ".moss/places.toml": `["Cambridge"]
lat = 52.205
lng = 0.119
precision = "city"
`,
    ".moss/theme/style.css": null,
  },
};

/**
 * A vertical-typesetting site with a places explorer and one located page.
 * The map is a horizontal widget: under `[site] typesetting = "vertical"` the
 * explorer page's figure and the locator's hydrated embed page must still be
 * as wide as the window, and the embed's map must paint its whole box.
 * Served by playwright/places-explorer-vertical.config.ts.
 */
export const PLACES_EXPLORER_VERTICAL_GATE: ScratchSiteSpec = {
  name: "places-explorer-vertical-gate",
  files: {
    "places/index.md": `---
title: Places
---

Every work this site locates, gathered on one map.
`,
    "harbour.md": `---
title: Harbour Morning
uid: "pev001aa"
location: "Lisbon"
date: 2024-06-10
author: Ines Moreau
---

The ferry leaves before the light is fully up.
`,
    ".moss/config.toml": `schema_version = 6

[site]
lang = "en"
locator = "align-right"
typesetting = "vertical"

[terms.places]
type = "place"
fields = ["location"]
`,
    ".moss/places.toml": `["Lisbon"]
lat = 38.722
lng = -9.139
precision = "exact"
`,
    ".moss/theme/style.css": null,
  },
};
