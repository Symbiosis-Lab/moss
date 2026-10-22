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
//! never emits — precisely what teaches agents to target dead selectors.
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
/// Audited 2026-08-03: the two most heavily customized moss sites (a large live
/// site, a literary-award site) overrode **zero** design tokens between them
/// and set six of these. None was discoverable — not in `moss describe --json`,
/// not in any published doc — so the large live site hand-fought the hero
/// height caps that `--moss-hero-max-height` exists to lift, across three
/// selectors and a 12-line comment.
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
/// Enforced by `every_escape_hatch_is_declared` in this crate's own
/// `tests/components_sync_test.rs`: a `var(--moss-*, …)` read that no
/// entry here declares fails the build. That test is the point of the table —
/// a hook nothing declares is a hook no agent can find.
pub const CUSTOM_PROPS: &[CustomProp] = &[
    CustomProp {
        name: "--moss-hero-max-height",
        owner: "moss-hero",
        default: "70vb",
        description: "Cap on hero media block size on desktop (height horizontally, width under vertical typesetting). `none` removes the crop entirely rather than raising it — the image renders at its own intrinsic size instead of a taller band; to make the band taller, give this a longer length (e.g. `120vb`) instead. Note the wrapper has its own cap — `.moss-hero { max-block-size: min(80vb, 800px) }` reads the same property, so setting it once lifts both. It is also the minimum height of a `mobile=overlay` hero on a phone, so `100svh` makes that hero fill the first screen. On a closing picture it is the whole height of the picture's box, in place of the picture's own shape; the ink measure assumes the uncropped picture, so set `panel=always` on a closing picture you cap.",
    },
    CustomProp {
        name: "--moss-hero-content-width",
        owner: "moss-hero-content",
        default: "auto",
        description: "Width of the words block over a hero's picture, from tablet width up (a phone stacks or full-widths it). Unset, the block is as wide as its text, so a title over a wide poster can shrink to its longest word; give it a length or a percentage of the hero (e.g. `29%`, `24rem`) to use the poster's clear area. It also replaces the block's default cap of `max(600px, 35%)`, so a wider value is not clamped.",
    },
    CustomProp {
        name: "--moss-hero-progress",
        owner: "moss-hero-indicator-segment",
        default: "0",
        description: "How far the playing clip of a sequence hero has played, 0 to 1. Set by the script on each indicator segment from the video's time; the segment's bar fills by that fraction. Read-only for a theme: restyle the bar, do not set this.",
    },
    CustomProp {
        name: "--moss-hero-object-position",
        owner: "moss-hero",
        default: "top",
        description: "Crop anchor for hero media, which is `object-fit: cover`. The default anchors the top, which suits landscapes; use `center` for portraits and faces.",
    },
    CustomProp {
        name: "--moss-hero-tone-color",
        owner: "moss-hero",
        default: "#2c2825",
        description: "Text colour over a pale ('light'-toned) hero image, in both overlaid layouts (desktop, and mobile in `data-mobile=\"overlay\"`). Deliberately not `var(--moss-color-text)`: that token is `#2c2825` in light mode but `#d4cbba` in dark, while the photograph underneath does not repaint with the colour scheme, so the token's dark-mode value would put pale text back on the same pale image. Set this to tune the exact shade; it stays fixed across both colour schemes.",
    },
    CustomProp {
        name: "--moss-hero-panel-bg",
        owner: "moss-hero-content",
        default: "rgba(20, 16, 12, 0.82)",
        description: "Backing panel behind overlay text in both overlaid hero layouts (desktop, and mobile under `mobile=overlay`), so the text stays legible wherever a drawn line, lettering or a face in the photo falls under it. moss does not set this itself: it publishes a per-render panel colour as `--item-hero-panel-bg`, tinted from the image's own scan-cached dominant colour (`color_extract::panel_background`) and engineered so the panel's WCAG contrast against the overlay text holds even in the worst case of its own translucency — composited over pure black or pure white, which bounds every actual pixel underneath. This token, when a theme sets it (on `:root` or `.moss-hero`), takes precedence over that computed colour. The flat default here only applies when neither exists (no media manifest). A pale ('light'-toned) hero reads this against a different literal fallback, `rgba(250, 248, 244, 0.82)` — same property, the other side of the dark/light text split `--moss-hero-tone-color` makes.",
    },
    CustomProp {
        name: "--moss-hero-mobile-band",
        owner: "moss-hero",
        default: "min(45vb, 320px)",
        description: "Under `mobile=overlay`, the height of image reserved above the text panel on a narrow viewport. Without a floor here the panel could grow tall enough (a heading plus a long paragraph and a link) to cover the whole hero, leaving no picture at all — a photo the words sit on, not one they erase. Raise it for a subject that needs more room to read, or lower it toward `0px` for a short caption where the default reserve would look like wasted space.",
    },
    CustomProp {
        name: "--moss-nav-island-display",
        owner: "moss-nav-island",
        default: "block",
        description: "Forces the floating nav island off from CSS, media queries included — the page then behaves as it did before this feature shipped: the masthead scrolls away and nothing replaces it. Not the main switch: the island is opt-in and ships off, so `[site].floating_nav = true` (the Settings → Services toggle) is what turns it on in the first place, and this property is for a theme that wants it off at some widths and not others. Appearance is otherwise tuned through the island's contract-registered classes; its measure already tracks `--moss-nav-width`/`--moss-content-width`, so widening the nav widens the island with it.",
    },
    CustomProp {
        name: "--moss-sidenote-reserve",
        owner: "body",
        default: "0px",
        description: "Width of the right-hand gutter reserved for margin sidenotes. It is the note column's measure, not how far the page moves: core derives `--moss-sidenote-inset` from it in site.css and pads <body> by that, so the page gives up only the width the window is short of fitting the gutter — nothing at all from about 1316px up, where a centred column already has margin enough — and every centred block moves half of what it gives up. The sidenotes stylesheet (shipped only on sites with footnotes) sets the reserve at wide viewports to a measured clamp, on every page except the front page, full-width pages and sidebar pages — those keep the bottom-sheet presentation at any width. A theme can override the clamp to widen or narrow the note column, and the shift follows it; setting it on a site without footnotes reserves a gutter nothing fills.",
    },
    CustomProp {
        name: "--moss-eyebrow-separator",
        owner: ".moss-eyebrow",
        default: "\" · \"",
        description: "The text drawn between the fields of the eyebrow line above a page title, as a CSS string. The markup puts only a space between fields, which this string's own spaces absorb, so give it spaces on both sides: `\" / \"` for a slash, or `\" \"` for a plain space.",
    },
    CustomProp {
        name: "--moss-field-group-separator",
        owner: ".moss-field-group",
        default: "\" · \"",
        description: "The text drawn between the parts of a declared field group, as a CSS string. The markup puts one space between the parts, which this string's own spaces absorb, so give it spaces on both sides, as for `--moss-eyebrow-separator`. A part with no value is not in the markup, so it takes its separator with it.",
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
        description: "Track widths for a `:::grid`, as a `grid-template-columns` value. moss sets it on the element when the author writes a ratio (`:::grid 2 1:2` → `minmax(0, 1fr) minmax(0, 2fr)`: one part for the first column, two for the second); the fallback is the even split for whatever `data-columns` says, and a ratio-less grid with no `data-columns` falls back to `initial`. It is a property rather than an inline `grid-template-columns` on purpose: an inline declaration beats every stylesheet rule, so a theme rule could never override it if it arrived inline. A theme setting this custom property by hand overrides the author's ratio at every width, same as any other cascade value.",
    },
    CustomProp {
        name: "--moss-gallery-row-height",
        owner: "moss-gallery",
        default: "16rem",
        description: "Height of each row in a `:::gallery rows` (the justified layout). Every photo in a row is scaled to this height, so the row's widths fill the container and each photo keeps its proportions. Set it on the gallery or on an ancestor; a narrow screen (600px and under) overrides it with 9rem so a row holds about two photos.",
    },
    CustomProp {
        name: "--moss-gallery-gap",
        owner: "moss-gallery",
        default: "6px",
        description: "Space between photos in a `:::gallery rows` block, on both the row and the column axis. The grid layout keeps its own gap; this only applies to the justified rows.",
    },
    CustomProp {
        name: "--moss-grid-scroll-peek",
        owner: "moss-grid",
        default: "2.5rem",
        description: "Exact width of the next card's visible slice at the edge of a scrolling row (`:::grid N {scroll}`) — how much of it shows, not an approximation. moss solves the row's `grid-auto-columns` so `data-columns` cards fit fully and precisely this much of the next one peeks past the edge. Raise it for a more insistent hint, lower it toward `0` to hide the cue.",
    },
    CustomProp {
        name: "--moss-collage-gap",
        owner: "moss-grid",
        default: "6px",
        description: "Gap between the tiles of a collage (`:::grid {collage}`), the same in both directions at every width.",
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
        description: "Aspect ratio of card cover images. Same reasoning as `--moss-grid-image-ratio`, for the cards of a folder listing rather than the cells of a `:::grid`. `aspect-ratio` has no logical spelling, so vertical typesetting overrides this to the transposed `3 / 4` in `site/vertical.css` rather than expressing the ratio once.",
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
        description: "Minimum column width in the auto-filled card grid. Lower it for denser grids of short items, raise it to force fewer, wider cards. Set it on `:root` for every grid or on a grid for that one; with none set, the grid picks a width by its sort order.",
    },
    CustomProp {
        name: "--moss-cover-color",
        owner: "moss-card",
        default: "var(--moss-bg, var(--moss-color-bg, #fff))",
        description: "Background behind card content when the card carries `data-cover-color`. moss does not set this itself: it publishes each card's colour from the cover image's dominant colour as `--item-cover-color` and paints with `var(--moss-cover-color, var(--item-cover-color))`, so a theme that sets `--moss-cover-color` (on `:root`, or on `.moss-card`) wins over the extracted tint — set it to `transparent` to opt out of the tint entirely. The same `--item-cover-color` is published on a `.moss-grid-card` whose cell opens with an image, where moss paints nothing, so a hand-built cell can wear the same band with `background: var(--moss-cover-color, var(--item-cover-color))`.",
    },
    CustomProp {
        name: "--moss-cover-ink",
        owner: "moss-card",
        default: "#fff",
        description: "Text colour on a card's tinted content band (title, and the kicker, meta and description derived from it at 75%, 70% and 85% opacity), and on the text block of a hero stacked below its image on a phone. Pairs with `--moss-cover-color`: `--moss-cover-color: transparent; --moss-cover-ink: var(--moss-color-text)` gives plain cards on the page background with readable text and no class selectors. Set it on `:root`, or on `.moss-card` for one scope.",
    },
    CustomProp {
        name: "--moss-header-bg",
        owner: "body > header",
        default: "transparent",
        description: "Background of the site header's band. Unset, the header has none. A page whose opening hero says `header=over` keeps its header transparent over the photo whatever this is set to, so a theme that only sets this token never needs to know about `header=over`. Set it on `:root`, with its dark-mode pair under `:root[data-theme=\"dark\"]`.",
    },
    CustomProp {
        name: "--moss-header-ink",
        owner: "body > header",
        default: "var(--moss-color-text-secondary)",
        description: "Colour of the nav links in the header, and of the site name beside them (whose own default is `var(--moss-color-text)`). Hover and the current page keep the accent colour. The opened phone menu takes it too, unless `--moss-menu-ink` is set. Under `header=over` the links take the light colour of the photo whatever this is set to. Use it with `--moss-header-bg` so the text stays readable on the band.",
    },
    CustomProp {
        name: "--moss-header-rule",
        owner: "body > header",
        default: "var(--moss-border-light)",
        description: "Colour of the hairline under the header's navigation. Set it to `transparent` when `--moss-header-bg` gives the header a band, since the band's own edge already divides it from the page; the hairline would otherwise draw inside the band.",
    },
    CustomProp {
        name: "--moss-menu-bg",
        owner: ".nav-links",
        default: "var(--moss-header-bg, var(--moss-color-bg))",
        description: "Background of the opened phone menu, the full-screen sheet that replaces the nav links below 20rem (320px). It follows the header's band, so with `--moss-header-bg` set the sheet matches it and the header's own rule is the only line between them. Keep it opaque: the page behind must not show through.",
    },
    CustomProp {
        name: "--moss-menu-ink",
        owner: ".nav-links",
        default: "var(--moss-header-ink, var(--moss-color-text))",
        description: "Colour of the links in the opened phone menu. The current page keeps the interface accent colour. Set it with `--moss-menu-bg` so the links stay readable on the sheet.",
    },
    CustomProp {
        name: "--moss-menu-link-size",
        owner: ".nav-links",
        default: "1.875rem",
        description: "Font size of the links in the opened phone menu, set in the heading font. The links stay in one column, flush left on the page gutter, and a label too long for the width wraps.",
    },
    CustomProp {
        name: "--moss-footer-bg",
        owner: "footer.container",
        default: "transparent",
        description: "Background colour of the site footer's band, which runs the full page width although the footer's content stays in its column. It takes a colour, not a gradient or image. Unset, the footer has none. Set it on `:root`, with its dark-mode pair under `:root[data-theme=\"dark\"]`.",
    },
    CustomProp {
        name: "--moss-footer-ink",
        owner: "footer.container",
        default: "var(--moss-color-muted)",
        description: "Colour of the text, links and headings in the footer. Hover and focus keep the accent colour, and the colophon keeps its own. The footer's own text defaults to `var(--moss-color-muted)`, its authored links to `var(--moss-color-accent)` and its headings and generated link list to `var(--moss-color-text)`; setting this replaces all of them with one colour. Use it with `--moss-footer-bg` so the text stays readable on the band.",
    },
    CustomProp {
        name: "--moss-footer-rule",
        owner: "footer.container",
        default: "var(--moss-border-light)",
        description: "Colour of the hairline at the top of the footer and of any horizontal rule (`---`) inside it, which is a plain line in the footer rather than the three-dot ornament. Set it to `transparent` when `--moss-footer-bg` gives the footer a band, since the band's own edge already divides it from the page.",
    },
    CustomProp {
        name: "--moss-footer-end",
        owner: "footer.container",
        default: "var(--moss-space-2xl)",
        description: "The space at the very end of the page, below the footer's last content. moss's colophon (the \"Published with moss\" line) puts the same space above itself, and the footer keeps it when a theme hides the colophon. Set it to `0` when the footer ends on a picture that should touch the page end; the value is smaller on a phone.",
    },
    CustomProp {
        name: "--moss-bg",
        owner: "moss-card",
        default: "var(--moss-color-bg, #fff)",
        description: "Fallback background in the `--moss-cover-color` chain, for a scope that wants a different neutral than the site background without redefining the `--moss-color-bg` token.",
    },
    CustomProp {
        name: "--moss-sheet-away",
        owner: "moss-footnotes-lifted",
        default: "1",
        description: "How far the lifted footnote sheet sits from its docked position (1 = away, 0 = docked). Written per-element at runtime by sidenotes.ts as the sheet scrolls back into its slot, and removed on dismiss. Not a theme hook: a hand-set value is overwritten on the next scroll frame.",
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
        description: "Width a `data-width` block escapes to. moss sets it per keyword (`wide`, `page`, `screen`); set it directly for a width the keywords do not cover. Always clamped by `min(…, 100cqi)`, so a narrow viewport stays safe. Under vertical typesetting the tokens are inert: every band stays in the column, which is already the full measure.",
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
        description: "The same for the mark's green, the far-end ink the eye weighs. Unset, it takes the ground's drop colour (`#4f7031` on paper, lifted to `#a3d483` on dark). Reach for this pair rather than `opacity` whenever a surface wants the mark quieter: opacity bleaches the green to a sage grey while the black half merely fades, and the two-tone reading is the design.",
    },
    CustomProp {
        name: "--moss-mark-fade",
        owner: "moss-mark",
        default: "0s",
        description: "How long the mark takes to change colour. The default snaps, which is what a surface that never recolours it wants. Give it a duration where the paint changes on hover or focus, so the mark fades rather than snaps — the colophon sets 240ms, the duration its wording fades out on.",
    },
    CustomProp {
        name: "--moss-place-marker",
        owner: "moss-places-marker",
        default: "#2d5a2d",
        description: "Fill colour of an explorer marker's own dot, its cluster-count badge, and a bloomed ring's legs/dots. `.moss-place-map` already sets this to `var(--moss-color-accent, #2d5a2d)` for the static map's own pins (precision_rank's marker colour); the explorer's own JS-positioned markers read the same property, so recolouring one recolours both.",
    },
    CustomProp {
        name: "--moss-place-marker-casing",
        owner: "moss-places-marker",
        default: "#ffffff",
        description: "The ring/border colour around a marker's dot and a cluster badge — set to the page background so a dense cluster of markers still reads as separate dots rather than a solid blob.",
    },
    CustomProp {
        name: "--moss-place-water",
        owner: "moss-places-viewport",
        default: "#dbe7ea",
        description: "First-paint background of the explorer's own pannable viewport, before the inlined world SVG's own identically-coloured water layer has loaded — never itself set independently of `.moss-place-map`'s own `--moss-place-water`, which already carries the site's light/dark values.",
    },
    CustomProp {
        name: "--moss-place-tile-opacity",
        owner: "moss-places-tiles",
        default: "0",
        description: "Cross-fade opacity of the explorer's regional tile layer. Written per-frame by tiles.ts's `TileLayer.render` as the camera crosses the fade band short of the world layer's own detail ceiling — 0 outside the band, where `render` also clears every tile element from the DOM, so the property and the content agree. Not a theme hook: a hand-set value is overwritten on the next camera settle.",
    },
    CustomProp {
        name: "--moss-place-river-scale",
        owner: "[data-map-layer=\"rivers\"]",
        default: "1",
        description: "Scale applied to a river path's own baked stroke-width (`--river-w`, set once per path by raster.ts's `splitMapSvg`), so rivers keep a constant on-screen width as `applyCamera` zooms the world layer instead of thickening with it. Not a theme hook: a hand-set value is overwritten on the next camera settle.",
    },
    CustomProp {
        name: "--moss-section-color",
        owner: "body",
        default: "unset",
        description: "The accent colour of the section a page sits in, written by moss as an inline style on `<body>` (and on a listing card for such a page, on the card's root element) when a folder's home page declares `color:`; the nearest coloured ancestor folder wins. No moss style reads it, so nothing changes until a theme does: `var(--moss-section-color, var(--moss-color-accent))`. Not set on pages outside any coloured folder, so always give `var()` a fallback.",
    },
    CustomProp {
        name: "--moss-section-text",
        owner: "body",
        default: "unset",
        description: "A readable text colour for the section accent: the hue of `--moss-section-color`, adjusted until it reaches 4.5:1 contrast (WCAG 2) against moss's default page background, with one value for the light theme and one for the dark, switching with `data-theme` like moss's own colour tokens. A colour that already passes is used unchanged. moss derives it at build time and writes the two variants (`--moss-section-text-light`, `--moss-section-text-dark`) beside `--moss-section-color`, on `<body>` and on a listing card for the page; the default stylesheet picks the one for the current theme. Write `color: var(--moss-section-text, var(--moss-color-text))`. The guarantee is against moss's default `--moss-color-bg`: a theme that repaints the page background checks its own contrast. Not set on pages outside any coloured folder, so always give `var()` a fallback.",
    },
    CustomProp {
        name: "--moss-section-text-light",
        owner: "body",
        default: "var(--moss-color-text)",
        description: "The light-theme variant of `--moss-section-text`, written by moss as an inline style on `<body>` and on a listing card, beside `--moss-section-color`, when a folder's home page declares `color:`. The default stylesheet copies the variant for the current theme into `--moss-section-text`; a theme should read `--moss-section-text` instead of this.",
    },
    CustomProp {
        name: "--moss-section-text-dark",
        owner: "body",
        default: "var(--moss-color-text)",
        description: "The dark-theme variant of `--moss-section-text`, written by moss as an inline style on `<body>` and on a listing card, beside `--moss-section-color`, when a folder's home page declares `color:`. The default stylesheet copies the variant for the current theme into `--moss-section-text`; a theme should read `--moss-section-text` instead of this.",
    },
    CustomProp {
        name: "--moss-sidenote-inset",
        owner: "body",
        default: "0px",
        description: "How far the page gives up on the right for the sidenote gutter, derived by site.css from `--moss-sidenote-reserve` and applied as `<body>` padding. The places explorer's full-width figure adds it back so the map still reaches the window's edge. Not a theme hook: set `--moss-sidenote-reserve` instead.",
    },
    CustomProp {
        name: "--moss-contents-lane",
        owner: ".moss-contents-ruler",
        default: "0px",
        description: "How wide each link of the contents ruler's invisible hit strip is on a page with `contents: pictures`: from the dash to the gutter before the text column, so the pointer can move from a dash toward its title without leaving the strip and never rests over the text. Set by the site runtime (contents-ruler.ts) from the measured layout. Not a theme hook.",
    },
    CustomProp {
        name: "--moss-contents-reach",
        owner: ".moss-contents-ruler",
        default: "0px",
        description: "How far each hit strip of an open `contents: pictures` ruler reaches from the dash: to the far edge of the drawn titles, so the pointer can move onto a title and click it. Set while the ruler is open by the site runtime (contents-lens.ts), removed at rest, when the strips shrink back to `--moss-contents-lane`. Not a theme hook.",
    },
    CustomProp {
        name: "--moss-contents-row",
        owner: ".moss-contents-ruler",
        default: "20px",
        description: "The height of one row of the contents ruler, at rest and in its hit strip; the title surfaces and the lens derive their sizes from it. The site runtime assumes 20px, so a theme should leave it.",
    },
    CustomProp {
        name: "--moss-contents-measure",
        owner: ".moss-contents-ruler",
        default: "24ch",
        description: "How wide an open title of a `contents: pictures` ruler may be before it wraps onto a second line. A theme may set it on `.moss-contents-ruler`.",
    },
    CustomProp {
        name: "--moss-contents-ink",
        owner: ".moss-contents-ruler-label",
        default: "1",
        description: "How much ink an open title of a `contents: pictures` ruler has, from 0 to 1: full at the row under the pointer, falling to a floor that keeps every title legible. Set by the site runtime (contents-lens.ts) on each open frame; the panel behind the titles does not fade with it. Not a theme hook.",
    },
    CustomProp {
        name: "--moss-place-figure-top",
        owner: "article.container > figure[data-moss-places-explorer]",
        default: "0px",
        description: "The explorer root figure's own rendered top offset in px — the header's rendered height, a layout measurement CSS cannot take on its own. Written by map.ts's `syncFigureOffset` at mount and on resize so the figure's `block-size` can subtract it, filling the viewport below the header exactly instead of reaching a header's worth past it. Not a theme hook: a hand-set value is overwritten on the next resize.",
    },
];

/// A `data-*` attribute moss emits on an element that carries no `moss-*` class.
///
/// `<body data-page="home">` is the case that forced this to exist. It is a
/// first-class part of the contract — the only thing that tells a stylesheet
/// which page it is on, and a site's entire front page can hang off it — but
/// `<body>` has no class, so declaring it would have meant inventing a
/// `moss-body` entry for a class moss never emits. That is the orphan-entry
/// problem in miniature: a contract that names selectors moss does not
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
        description: "Present as `home` on the front page of EVERY language the site publishes, not just the site-default locale's — a multilingual site's `zh-hans/` front page carries this exactly like the default-locale one at the root. Scope front-page-only rules to `body[data-page=\"home\"]` rather than to something merely unique to your homepage today — a hero that fills the screen, a suppressed footer, a different nav treatment.",
    },
    ScopeAttr {
        selector: "body",
        name: "data-path",
        values: &[],
        description: "The page's published address, with a leading and trailing slash, exactly as the sitemap and links spell it: `/` on the front page, `/contact/` for a page at that address, `/zh-hans/about/` for a language edition. It is the address as moss writes it, so a folder named `Film & Sound` is `/film-and-sound/`, and characters outside ASCII appear as written, not percent-encoded. Scope one page with `body[data-path=\"/contact/\"]` and a whole section with `body[data-path^=\"/work/\"]`. A selector on an address breaks when the page is renamed, because the address changes with it.",
    },
    ScopeAttr {
        selector: "body",
        name: "data-section",
        values: &[],
        description: "The url path segment of the folder whose home page sets `color:` for this page: `work` for every page under `/work/`, the nearest coloured folder when they nest. Absent on pages outside any coloured folder, and on a site-wide colour set by the front page. Scope a section with `body[data-section=\"work\"]` and tint it with `var(--moss-section-color)`, instead of `body[data-path^=\"/work/\"]`, which breaks when the folder is renamed.",
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
        description: "Set by block shortcodes, and by a fenced div that carries a width word (`:::{.band screen}`), to escape the text column. The width resolves through `--moss-escape`, clamped by `min(…, 100cqi)` so a narrow viewport stays safe.",
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
    ScopeAttr {
        selector: "main :is(div, h1, h2, h3, h4, h5, h6)",
        name: "data-contents-end",
        values: &[],
        description: "Present on the element an author marked as the end of the page's main sections, with `contents=end` on a fenced div (`::: {.band screen contents=end}`) or on a heading (`## Proposals {contents=end}`). The contents ruler lists only the sections before it and steps aside once the element's top passes the middle of the window, and returns when the reader scrolls back above it. The nav island's sections panel still lists every heading, and the island ignores the marker.",
    },
    ScopeAttr {
        selector: "body",
        name: "data-contents",
        values: &["pictures"],
        description: "Present as `pictures` when a page asks, with `contents: pictures` in its frontmatter, for pictures in its contents list: on a wide screen the ruler in the left margin stays the ruler of dashes, and hovering it opens the rows around the pointer like a wheel, each section that has an image showing a small picture of it beside its title; the nav island's sections panel carries the same pictures. Absent keeps the plain ruler.",
    },
    ScopeAttr {
        selector: ".moss-eyebrow > span, .moss-subtitle, .moss-key-figure, .moss-facts > div, .moss-field-group > [data-field]",
        name: "data-field",
        values: &[],
        description: "The declared field's name, on each element a placed field draws: each eyebrow span, the subtitle, each key figure and each facts row, and each part of a declared group. Scope one field with `[data-field=\"ja\"]` rather than a selector on the page's address.",
    },
];
