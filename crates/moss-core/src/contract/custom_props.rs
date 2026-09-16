//! Theming escape hatches — Source 2b of the federated contract.
//!
//! Two tables that do not fit the class-keyed [`super::components::COMPONENTS`]
//! table, kept here rather than bolted onto it.
//!
//! Why they are separate: 167 component entries would each need an empty array
//! that says nothing, and several of these hooks have no single owning class at
//! all — `--moss-nav-width` resolves against the `<body>`-level content width,
//! `--moss-escape` is set by `[data-width]` on any block, and `data-page` lives
//! on `<body>`, which carries no `moss-*` class. Inventing a `moss-body` entry
//! to give it a home would have added an orphan contract entry for a class moss
//! never emits — precisely what teaches agents to target dead selectors (#777).
//!
//! ## Adding a hook
//!
//! 1. Read it from a stylesheet as `var(--moss-foo, <fallback>)`.
//! 2. Add a [`CustomProp`] here, with `default` copied **verbatim** from the
//!    call site. A declared hook with an invented default is worse than an
//!    undeclared one: an agent reasons from the wrong starting point and has no
//!    way to tell.
//! 3. `cargo test --test components_sync_test` — `every_escape_hatch_is_declared`
//!    fails on a read nothing declares, and `every_declared_custom_prop_is_read`
//!    fails on a declaration nothing reads.

/// A CSS custom property a theme may set to reconfigure a component.
///
/// These are read by moss's stylesheets as `var(--moss-foo, <fallback>)` and
/// are deliberately **never declared** — that is exactly what makes them opt-in
/// escape hatches rather than design tokens. A token has a value in `:root` and
/// cascades site-wide; one of these has no value until a theme sets it, and it
/// is set *on a component or a scope* to change that component.
///
/// The distinction matters because it is the whole theming API in practice.
/// Audited 2026-08-03: the two most heavily customized moss sites (okagaki, 在場)
/// overrode **zero** design tokens between them and set six of these. None was
/// discoverable — not in `moss describe --json`, not in any published doc — so
/// okagaki hand-fought the hero height caps that `--moss-hero-max-height` exists
/// to lift, across three selectors and a 12-line comment.
pub struct CustomProp {
    /// Property name including the leading dashes (e.g. `"--moss-hero-max-height"`).
    pub name: &'static str,
    /// Class of the component whose rules read it, or a scope selector when no
    /// single class owns it (e.g. `"body"`). Free-form: this is documentation,
    /// not a foreign key.
    pub owner: &'static str,
    /// The fallback moss's own CSS uses when the theme does not set it.
    /// Taken verbatim from the `var()` call site — never invented.
    pub default: &'static str,
    /// What setting it does, and when you would want to.
    pub description: &'static str,
}

/// Every escape-hatch custom property moss's stylesheets read.
///
/// Kept as its own table rather than a field on [`ComponentEntry`] for two
/// reasons: 167 entries would each need a `custom_props: &[]` that says
/// nothing, and several of these are not owned by a single class anyway
/// (`--moss-nav-width` is read against the `<body>`-level content width;
/// `--moss-escape` is set by `[data-width]` on any block).
///
/// Enforced by `every_escape_hatch_is_declared` in
/// `src-tauri/tests/components_sync_test.rs`: a `var(--moss-*, …)` read that no
/// entry here declares fails the build. That test is the point of the table —
/// a hook nothing declares is a hook no agent can find.
pub const CUSTOM_PROPS: &[CustomProp] = &[
    CustomProp {
        name: "--moss-hero-max-height",
        owner: "moss-hero",
        default: "70vh",
        description: "Cap on hero media height on desktop. Set `none` for a hero that fills its container. Note the wrapper has its own cap — `.moss-hero { max-height: min(80vh, 800px) }` reads the same property, so setting it once lifts both.",
    },
    CustomProp {
        name: "--moss-hero-object-position",
        owner: "moss-hero",
        default: "top",
        description: "Crop anchor for hero media, which is `object-fit: cover`. The default anchors the top, which suits landscapes; use `center` for portraits and faces.",
    },
    CustomProp {
        name: "--moss-nav-island-display",
        owner: "moss-nav-island",
        default: "block",
        description: "Forces the floating nav island off from CSS, media queries included — the page then behaves as it did before ADR-049: the masthead scrolls away and nothing replaces it. Not the main switch: the island is opt-in and ships off, so `[site].floating_nav = true` (the Settings → Services toggle) is what turns it on in the first place, and this property is for a theme that wants it off at some widths and not others. Appearance is otherwise tuned through the island's contract-registered classes; its measure already tracks `--moss-nav-width`/`--moss-content-width`, so widening the nav widens the island with it.",
    },
    CustomProp {
        name: "--moss-sidenote-reserve",
        owner: "body",
        default: "0px",
        description: "Width of the right-hand gutter reserved for margin sidenotes. It is the note column's measure, not how far the page moves: core derives `--moss-sidenote-inset` from it in site.css and pads <body> by that, so the page gives up only the width the window is short of fitting the gutter — nothing at all from about 1316px up, where a centred column already has margin enough — and every centred block moves half of what it gives up. The sidenotes stylesheet (shipped only on sites with footnotes) sets the reserve at wide viewports to a measured clamp, on every page except the front page, full-width pages and sidebar pages — those keep the bottom-sheet presentation at any width. A theme can override the clamp to widen or narrow the note column, and the shift follows it; setting it on a site without footnotes reserves a gutter nothing fills.",
    },
    CustomProp {
        name: "--moss-hint-x",
        owner: "[data-tooltip]",
        default: "0px",
        description: "Horizontal offset of a hover hint's pill from its host's start edge. Written per-element at runtime by theme.js (hint-place.ts), which measures the pill on hover/focus entry and clamps it into the viewport so a hint can never crop at a screen edge. Not a theme hook: a hand-set value is overwritten on the next hover.",
    },
    CustomProp {
        name: "--moss-hint-max-w",
        owner: "[data-tooltip]",
        default: "calc(100vw - 24px)",
        description: "Widest a hover hint's pill may get before it wraps. Written per-element at runtime by theme.js (hint-place.ts) alongside `--moss-hint-x`, because the CSS fallback's `100vw` counts the scrollbar gutter as usable space and no CSS length can subtract it. Not a theme hook: a hand-set value is overwritten on the next hover.",
    },
    CustomProp {
        name: "--moss-grid-ratio",
        owner: "moss-grid",
        default: "repeat(N, minmax(0, 1fr))",
        description: "Track widths for a `:::grid`, as a `grid-template-columns` value. moss sets it on the element when the author writes a ratio (`:::grid 2 1:2` → `2fr 1fr`); the fallback is the even split for whatever `data-columns` says, and a ratio-less grid with no `data-columns` falls back to `initial`. It is a property rather than an inline `grid-template-columns` on purpose: an inline declaration beats every stylesheet rule, including the mobile collapse, so a ratio grid stayed multi-column on a phone. A theme setting this by hand overrides the author's ratio at every width — the mobile collapse still wins, because that rule does not read the property.",
    },
    CustomProp {
        name: "--moss-grid-image-ratio",
        owner: "moss-grid-card",
        default: "1 / 1",
        description: "Aspect ratio of images inside a `:::grid`. Set to the source art's own ratio when the image is a designed artifact whose edges carry meaning (a poster, a titled tile) rather than a photograph.",
    },
    CustomProp {
        name: "--moss-grid-image-radius",
        owner: "moss-grid-card",
        default: "8px",
        description: "Corner radius of grid images. `50%` makes circular portraits; `0` suits art that has its own designed corners.",
    },
    CustomProp {
        name: "--moss-grid-image-fit",
        owner: "moss-grid-card",
        default: "cover",
        description: "`object-fit` for grid images. `contain` letterboxes onto the surface colour instead of cropping — the right choice for typographic work, where a crop costs words rather than scenery.",
    },
    CustomProp {
        name: "--moss-card-cover-ratio",
        owner: "moss-card-cover",
        default: "4 / 3",
        description: "Aspect ratio of card cover images. Same reasoning as `--moss-grid-image-ratio`, for `:::cards` rather than `:::grid`.",
    },
    CustomProp {
        name: "--moss-card-cover-fit",
        owner: "moss-card-cover",
        default: "cover",
        description: "`object-fit` for card covers. `contain` for artwork whose edges carry meaning; `cover` stays right for photography.",
    },
    CustomProp {
        name: "--moss-card-min",
        owner: "moss-cards",
        default: "280px",
        description: "Minimum column width in the auto-filled card grid. Lower it for denser grids of short items, raise it to force fewer, wider cards.",
    },
    CustomProp {
        name: "--moss-cover-color",
        owner: "moss-card",
        default: "var(--moss-bg, var(--moss-color-bg, #fff))",
        description: "Background behind card content when the card carries `data-cover-color`. moss sets this per-card from the cover image's dominant colour; a theme can override it to opt out of the extracted tint. Also set on a `.moss-grid-card` whose cell opens with an image — there moss only publishes the colour and paints nothing, so a hand-built cell can wear the same band as a card.",
    },
    CustomProp {
        name: "--moss-bg",
        owner: "moss-card",
        default: "var(--moss-color-bg, #fff)",
        description: "Fallback background in the `--moss-cover-color` chain, for a scope that wants a different neutral than the site background without redefining the `--moss-color-bg` token.",
    },
    CustomProp {
        name: "--moss-nav-width",
        owner: "main-nav",
        default: "var(--moss-content-width)",
        description: "Width of the header nav's inner row. Unset (the default) the nav tracks the `<body>`-level content width, so it stays aligned with the article column through every `content_width` preset. Set it only to deliberately break that alignment.",
    },
    CustomProp {
        name: "--moss-escape",
        owner: "[data-width]",
        default: "100%",
        description: "Width a `data-width` block escapes to. moss sets it per keyword (`wide`, `page`, `screen`); set it directly for a width the keywords do not cover. Always clamped by `min(…, 100cqw)`, so a narrow viewport stays safe.",
    },
    CustomProp {
        name: "--moss-success",
        owner: "moss-input-feedback",
        default: "#10b981",
        description: "Colour of a success message under a form field. Deliberately not a token: it is one accent moss does not want to spend a site-wide variable on.",
    },
    CustomProp {
        name: "--moss-error",
        owner: "moss-input-feedback",
        default: "#c85450",
        description: "Colour of an error message under a form field, and of comment-thread error states.",
    },
    CustomProp {
        name: "--moss-radius-md",
        owner: "moss-subscribe",
        default: "0.5rem",
        description: "Corner radius of the subscribe card. Set to `0` for a square-cornered form that matches a flat theme.",
    },
    CustomProp {
        name: "--moss-mark-paint",
        owner: "moss-mark",
        default: "var(--moss-mark-ink)",
        description: "What the mark's black half is painted with. Unset, it takes the ink for the ground it is on — black on paper, a held-back near-white on dark, where a solid mass at full white out-weighs the name beside it — so a surface that wants the mark inked sets nothing. Set it to quiet the mark or give it a colour of its own; the colophon sets `currentColor`, which rests the mark in the credit's own grey and takes the ink back on hover. Set `--moss-mark-drop-paint` with it: a grey mark beside a green drop is a broken mark, not a quiet one.",
    },
    CustomProp {
        name: "--moss-mark-drop-paint",
        owner: "moss-mark",
        default: "var(--moss-mark-drop)",
        description: "The same for the mark's green drop, the satellite the eye weighs. Unset, it takes the ground's drop colour (`#6b8e4e` on paper, lifted to `#8aba6a` on dark). Reach for this pair rather than `opacity` whenever a surface wants the mark quieter: opacity bleaches the green to a sage grey while the black half merely fades, and the two-tone reading is the design.",
    },
    CustomProp {
        name: "--moss-mark-fade",
        owner: "moss-mark",
        default: "0s",
        description: "How long the mark takes to change colour. The default snaps, which is what a surface that never recolours it wants. Give it a duration where the paint changes on hover or focus, so the mark fades rather than snaps — the colophon sets 240ms, the duration its wording fades out on.",
    },
];

/// A `data-*` attribute moss emits on an element that carries no `moss-*` class.
///
/// `<body data-page="home">` is the case that forced this to exist. It is a
/// first-class part of the contract — the only thing that tells a stylesheet
/// which page it is on, and okagaki's entire front page hangs off it — but
/// `<body>` has no class, so declaring it would have meant inventing a
/// `moss-body` entry for a class moss never emits. That is the orphan-entry
/// problem (#777) in miniature: a contract that names selectors moss does not
/// produce teaches agents to target dead ones.
pub struct ScopeAttr {
    /// CSS selector for the element carrying it (e.g. `"body"`).
    pub selector: &'static str,
    /// Attribute name including the `data-` prefix.
    pub name: &'static str,
    /// Allowed values. Empty means free-form.
    pub values: &'static [&'static str],
    /// What it marks, and what to scope to it.
    pub description: &'static str,
}

/// Structural attributes on classless elements. See [`ScopeAttr`].
pub const SCOPE_ATTRS: &[ScopeAttr] = &[
    ScopeAttr {
        selector: "body",
        name: "data-page",
        values: &["home"],
        description: "Present as `home` on the site's front page only. Scope front-page-only rules to `body[data-page=\"home\"]` rather than to something merely unique to your homepage today — a hero that fills the screen, a suppressed footer, a different nav treatment.",
    },
    ScopeAttr {
        selector: "html",
        name: "data-theme",
        values: &["light", "dark"],
        description: "The resolved colour scheme, set on `<html>` before the first paint by an inline script, from the reader's stored choice or their OS preference. So `[data-theme=\"dark\"]` alone is sufficient for dark-mode rules — do not also write an `@media (prefers-color-scheme: dark)` block, which ignores the toggle and will disagree with it.",
    },
    ScopeAttr {
        selector: "article > [data-width]",
        name: "data-width",
        values: &["body", "wide", "page", "screen"],
        description: "Set by block shortcodes to escape the text column. The width resolves through `--moss-escape`, clamped by `min(…, 100cqw)` so a narrow viewport stays safe.",
    },
    ScopeAttr {
        selector: "body",
        name: "data-typesetting",
        values: &["vertical"],
        description: "Present as `vertical` when the page is set in vertical CJK writing mode, from `[site] typesetting` in `.moss/config.toml` or a page's `typesetting:` frontmatter. Absent means horizontal. It reorients roughly 50 rules — nav, article flow, scroll direction — so a theme for a vertical site scopes to `body[data-typesetting=\"vertical\"]` rather than reinventing the mode.",
    },
    ScopeAttr {
        selector: "body",
        name: "data-content-width",
        values: &["wide", "full"],
        description: "Present when a page widens its column via `content_width:` frontmatter; absent at the default reading measure. This is what `--moss-content-width` — and therefore `--moss-nav-width`, which tracks it — resolves against, so it is the hook for a layout that should respond to the preset rather than to a fixed width.",
    },
];
