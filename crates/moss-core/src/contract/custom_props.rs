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
        name: "--moss-title-size",
        owner: "moss-article-title",
        default: "an article's title: --moss-read-title (40px); a folder's title: 2rem, and no more than that on a phone",
        description: "Font size of a page's title and a folder's title, at every width: set it once (a length, or `clamp()`) and the defaults no longer apply. Without it a folder's title is 2rem from 48rem up and follows the reading size below that, capped at 2rem so a phone's title is never larger than a desktop's.",
    },
    CustomProp {
        name: "--moss-hero-max-height",
        owner: "moss-hero",
        default: "70vb",
        description: "Cap on hero media block size on desktop (height horizontally, width under vertical typesetting). `none` removes the crop entirely rather than raising it — the image renders at its own intrinsic size instead of a taller band; to make the band taller, give this a longer length (e.g. `120vb`) instead. Note the wrapper has its own cap — `.moss-hero { max-block-size: min(80vb, 800px) }` reads the same property, so setting it once lifts both. It is also the minimum height of a `mobile=overlay` hero on a phone, so `100svh` makes that hero fill the first screen. A hero's `max-height=` option sets it on that hero. On a closing picture it is the whole height of the picture's box from 48rem, in place of the picture's own shape, and the build measures the words' ink over the part of the picture that capped box shows; a value a theme sets itself is honoured by the stylesheet but not measured. On a phone it is a closing picture's minimum height instead (with `fade=top`, at least 38rem).",
    },
    CustomProp {
        name: "--moss-hero-content-width",
        owner: "moss-hero-content",
        default: "auto",
        description: "Width of the words block over a hero's picture, measured as its border box (the block's padding is inside it), from tablet width up (a phone stacks or full-widths it). Unset, the block is as wide as its text, so a title over a wide poster can shrink to its longest word; give it a length or a percentage of the hero (e.g. `29%`, `24rem`) to use the poster's clear area. It also replaces the block's default cap of `max(600px, 35%)`, so a wider value is not clamped. On a closing picture this sets the block's width; a title's own measure belongs on the heading, as `max-inline-size` in `ch` on the closing heading, which moss reads.",
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
        description: "Crop anchor for hero media, which is `object-fit: cover`. The default anchors the top, which suits landscapes; use `center` for portraits and faces. A hero image's own display words (`focus:X%,Y%`, `top`, `bottom left`, ...) after its pipe set the crop for that image instead.",
    },
    CustomProp {
        name: "--moss-hero-tone-color",
        owner: "moss-hero",
        default: "#2c2825",
        description: "Text colour over a pale ('light'-toned) hero image, in both overlaid layouts (desktop, and mobile in `data-mobile=\"overlay\"`). Deliberately not `var(--moss-color-text)`: that token is `#2c2825` in light mode but `#d4cbba` in dark, while the photograph underneath does not repaint with the colour scheme, so the token's dark-mode value would put pale text back on the same pale image. Set this to tune the exact shade; it stays fixed across both colour schemes.",
    },
    CustomProp {
        name: "--moss-hero-title-size",
        owner: "moss-hero-content",
        default: "2.5rem (1.75rem under 64rem, 1.4rem on a phone overlay)",
        description: "Font size of a hero's lead heading, at every width: set it once (a length, or `clamp()` of px, rem and vw) and the stepped defaults no longer apply. A closing picture's ink measurement reads it when it is declared in `:root`, so the words can stay in ink with no panel.",
    },
    CustomProp {
        name: "--moss-hero-link-size",
        owner: "moss-hero-content",
        default: "1em",
        description: "Font size of the link button in a hero's words (the closing picture's call to action). Read by the ink measurement when declared in `:root`.",
    },
    CustomProp {
        name: "--moss-hero-link-padding",
        owner: "moss-hero-content",
        default: "0.5em 1.2em",
        description: "Padding of the link button in a hero's words, block then inline, in `em` of the link's own size. Read by the ink measurement when declared in `:root` as one or two `em` values; any other form leaves the words their panel.",
    },
    CustomProp {
        name: "--moss-hero-words-gap",
        owner: "moss-hero-content",
        default: "var(--moss-space-sm)",
        description: "The space between a hero's heading and its text, and between its text and its row of links. One value sets both. A heading followed straight by the links is two gaps from them: the heading's and the link's. Read by the ink measurement when it is a positive px, rem or vw length in `:root`.",
    },
    CustomProp {
        name: "--moss-hero-credit-size",
        owner: "moss-hero-credit",
        default: "0.75rem",
        description: "Font size of a closing picture's credit (`caption=`), on a 1.45 line. Read by the ink measurement when it is a positive px, rem or vw length in `:root`, so the credit's area and the contrast it needs follow the size.",
    },
    CustomProp {
        name: "--moss-hero-panel-bg",
        owner: "moss-hero-content",
        default: "rgba(20, 16, 12, 0.82)",
        description: "Backing panel behind overlay text in both overlaid hero layouts (desktop, and mobile under `mobile=overlay`), so the text stays legible wherever a drawn line, lettering or a face in the photo falls under it. moss does not set this itself: it publishes a per-render panel colour as `--item-hero-panel-bg`, tinted from the image's own scan-cached dominant colour (`color_extract::panel_background`) and engineered so the panel's WCAG contrast against the overlay text holds even in the worst case of its own translucency — composited over pure black or pure white, which bounds every actual pixel underneath. This token, when a theme sets it (on `:root` or `.moss-hero`), takes precedence over that computed colour. The flat default here only applies when neither exists (no media manifest). A pale ('light'-toned) hero reads this against a different literal fallback, `rgba(250, 248, 244, 0.82)` — same property, the other side of the dark/light text split `--moss-hero-tone-color` makes.",
    },
    CustomProp {
        name: "--moss-hero-credit-font",
        owner: "moss-hero-credit",
        default: "var(--moss-font-body)",
        description: "The face of a closing picture's credit (`caption=`). It defaults to moss's UI face, `--moss-font-body`; set it to `var(--moss-font-mono)`, or any font stack, to give the credit a face of its own. The build does not see the face: it takes a letter as 0.6 em wide, a monospace face's width, when it works out how many lines the credit sets and so where its ink is measured, which can only make that area larger.",
    },
    CustomProp {
        name: "--moss-credit-end",
        owner: "moss-hero-closing",
        default: "0px",
        description: "Where a closing picture's credit box ends, measured from the picture's inline end: the end of the page's text column less the credit's 0.5rem of padding, so the credit's text is right-aligned to the column's end. Set by moss on the closing picture from `--moss-words-inset`; read-only for a theme (`--moss-hero-credit-font` restyles the credit).",
    },
    CustomProp {
        name: "--moss-closing-clear",
        owner: "nav-icons",
        default: "0px",
        description: "How much of the window's height a closing picture covers from its top edge down to the window's foot, set on the controls by the contents list's script as the page scrolls. The page's controls (search, language, theme), where they stand in the foot of the window beside a `contents: pictures` list, ride that far up, so they stay above the picture instead of over it. Read-only for a theme.",
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
        name: "--moss-container-padding-narrow",
        owner: ":root",
        default: "1.5rem",
        description: "The page's side gutter below 48rem, a phone. `--moss-container-padding` (`2rem`) is the gutter from 48rem up; below it moss uses this value instead. Set this, on `:root`, to change the phone gutter alone (`1rem` for 16px) without touching the desktop gutter. Setting `--moss-container-padding` itself still changes the gutter at every width.",
    },
    CustomProp {
        name: "--moss-font-section",
        owner: "article",
        default: "var(--moss-font-body)",
        description: "Face of the section headings in a page's text (`#`, `##`, `###` in the body). They use the body face; `--moss-font-heading` sets the page's title, a folder's title and a card's title. Set this to give the section headings a face of their own, for example `var(--moss-font-heading)` for serif sections.",
    },
    CustomProp {
        name: "--moss-collage-gap",
        owner: "moss-grid",
        default: "6px",
        description: "Gap between the tiles of a collage (`:::grid {collage}`), the same in both directions at every width.",
    },
    CustomProp {
        name: "--moss-collage-lead",
        owner: "moss-grid",
        default: "6fr (4fr for an upright lead)",
        description: "Width of a collage's lead column on a wide screen, against six `1fr` columns for the other tiles. Unset, it is `6fr`, half the collage, or `4fr` when the lead cell is `data-upright` (a picture taller than wide). Setting it fixes the width whatever the lead holds.",
    },
    CustomProp {
        name: "--moss-collage-row",
        owner: "moss-grid",
        default: "216px (160px at 8 or 9 tiles, 180px under 1024px, 300px at 3 or 4)",
        description: "Least height of each collage row on a wide screen. A row is never shorter; it grows only when a quote or other text in it needs more room, never for a picture, whose media is absolutely placed.",
    },
    CustomProp {
        name: "--moss-collage-span",
        owner: "moss-grid-card",
        default: "set by the build",
        description: "How many of the six columns right of the lead a collage tile takes on a wide screen, written by the build into the tile's `style`. Read-only for a theme: a row's spans sum to six.",
    },
    CustomProp {
        name: "--moss-grid-parts",
        owner: "moss-grid-card",
        default: "1",
        description: "How many tracks each card of a `:::grid {rows=equal}` spans: the most children (picture, title, text, list, a fenced `{.class}` block, …) any card in the grid has once it is rendered, written by the build into the grid's `style`. A raw-HTML or fenced wrapper around a whole card counts as one. Read-only for a theme.",
    },
    CustomProp {
        name: "--moss-grid-part-gap",
        owner: "moss-grid-card",
        default: "10px",
        description: "The space between the parts of one card in a `:::grid {rows=equal}`. It is also the grid's row gap there, so a theme's own `gap` on such a grid does not set it (the part gap wins, `!important`): set `--moss-grid-card-gap` for the space between cards.",
    },
    CustomProp {
        name: "--moss-grid-card-gap",
        owner: "moss-grid-card",
        default: "the grid's usual row gap",
        description: "The space between the rows of cards in a `:::grid {rows=equal}`, written as a top margin on every card below the first row. Needed because the grid's own row gap is the gap between a card's parts there (`--moss-grid-part-gap`). A value below the part gap is raised to the part gap.",
    },
    CustomProp {
        name: "--moss-collage-span-sm",
        owner: "moss-grid-card",
        default: "set by the build",
        description: "How many of six columns a collage tile takes on a phone, written by the build into the tile's `style`: 6 for a quote or other text, which takes a full row at its own height; 2 for a picture (square), and 3 (4:3) or 6 (3:2) for the pictures of a short last row. Read-only for a theme.",
    },
    CustomProp {
        name: "--moss-link-color",
        owner: "article",
        default: "var(--moss-color-text)",
        description: "Colour of a link in a page's text (`article a`, not a button). Links rest in the text colour and are underlined; set this to accent them, for example `--moss-link-color: var(--moss-color-accent)`.",
    },
    CustomProp {
        name: "--moss-link-hover-color",
        owner: "article",
        default: "var(--moss-color-accent)",
        description: "Colour of a link in a page's text under the pointer or with focus. In the dark theme the default is `--moss-color-accent-hover`. A theme that accents its resting links sets this to a different colour, for example `var(--moss-color-accent-hover)`, so a hover or focus still shows.",
    },
    CustomProp {
        name: "--moss-font-cjk",
        owner: "moss-quote",
        default: "var(--moss-font-heading)",
        description: "Face of a collage quote tile whose words are Chinese or Japanese (`data-script=\"cjk\"`, written by the build), set at 17px to 21px with wide tracking. The default is the heading face, whose Han serifs follow the page's Traditional or Simplified setting. `--moss-font-quote`, when set, wins.",
    },
    CustomProp {
        name: "--moss-font-quote",
        owner: "moss-quote",
        default: "var(--moss-font-body)",
        description: "Face of a quote cell's words (`figure.moss-quote`). Unset, the body face, whose stack already carries the CJK faces, so a quote in Chinese needs no markup.",
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
        name: "--moss-place-world-detail",
        owner: "moss-places-viewport",
        default: "1",
        description: "How much of the built-in world map's detail (sea-floor and relief bands, urban areas, rivers) the explorer shows: 1 down to its clean depth, falling to 0 past it, where only the flat land and water tones remain instead of enlarged shapes the data cannot support. Written by the explorer from the camera's zoom on every settle; detail layers are unaffected. Not a theme hook: a hand-set value is overwritten.",
    },
    CustomProp {
        name: "--moss-place-land",
        owner: "moss-places-viewport",
        default: "#e3e6d5",
        description: "The flat land tone of the world map, which stays once its detail has faded past the clean depth. `.moss-place-map` already carries the site's light and dark values; the default here matches the light one.",
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
        name: "--moss-nav-island-edge",
        owner: "body",
        default: "1px",
        description: "The floating nav island bar's border width. Declared on `body` with the bar's other geometry so the headerless controls that stand in the bar's end slot are placed from the same numbers. Not a theme hook.",
    },
    CustomProp {
        name: "--moss-nav-island-pad-block",
        owner: "body",
        default: "6px",
        description: "The floating nav island bar's padding above and below its controls. Declared on `body`, beside `--moss-nav-island-edge`, for the same reason. Not a theme hook.",
    },
    CustomProp {
        name: "--moss-nav-island-pad-end",
        owner: "body",
        default: "8px",
        description: "The floating nav island bar's padding at its end edge, before its last control or the headerless controls' slot. Declared on `body`, beside `--moss-nav-island-edge`. Not a theme hook.",
    },
    CustomProp {
        name: "--moss-nav-island-control-pad",
        owner: "body",
        default: "12px",
        description: "The island's sections button padding, both sides together; with its 1em glyph at `--moss-size-lg` it sets the bar's height, and the headerless controls are centred on that. Not a theme hook.",
    },
    CustomProp {
        name: "--moss-nav-island-controls",
        owner: "body",
        default: "2.25rem",
        description: "How wide the headerless island's controls (`.nav-icons`) are in a row: the slot the bar keeps free at its end for them, and the room the page title's first line keeps for them at the top. Measured and written by the site runtime (nav-island.ts) on layout. Not a theme hook.",
    },
    CustomProp {
        name: "--moss-nav-title-line",
        owner: "body",
        default: "unset",
        description: "Where the centre of the page title's first line is, in px from the top of the headerless island's controls' containing block: at the top of the page the controls (`.nav-icons`) stand centred on it. Measured and written by the site runtime (nav-island.ts); unset on a page with no title, which keeps the bar's place. Not a theme hook.",
    },
    CustomProp {
        name: "--moss-nav-island-controls-guess",
        owner: "body",
        default: "3rem",
        description: "The room the page title's first line keeps for the headerless controls before the site runtime has measured them, or without scripting: 3rem for each control (one, two or three), more than any of them takes. Not a theme hook.",
    },
    CustomProp {
        name: "--moss-contents-inset",
        owner: "body",
        default: "32px",
        description: "How far the contents ruler stands from the window's left edge (its dashes start one `--moss-contents-pad` further in, where the headerless controls under the ruler start too). The lens's fade is placed from it. Not a theme hook.",
    },
    CustomProp {
        name: "--moss-contents-pad",
        owner: "body",
        default: "8px",
        description: "The contents ruler list's own side padding, which its hit strips reach back across. Not a theme hook.",
    },
    CustomProp {
        name: "--moss-contents-dash",
        owner: "body",
        default: "10px",
        description: "The length of a contents ruler dash at rest (and of a sections panel row's dash). Not a theme hook.",
    },
    CustomProp {
        name: "--moss-contents-lane",
        owner: "body",
        default: "unset",
        description: "The lane kept free for the contents list on a page with `contents: pictures`, from the window's left edge to a gutter before the text: `--moss-contents-inset` plus twice `--moss-contents-pad` plus `--moss-contents-row` plus `--moss-contents-gutter` (92px), set only where the window is at least 64rem wide, the pointer is fine and the page is horizontal and left-to-right, and unset everywhere else. Where it is set the page column narrows to `min(column, --moss-page-inline - 2 * lane)`, and, when the list also fits the window's height, the list is up and the floating bar stays away (the island's `data-ruler` is `on` or `lane`); the site runtime reads only whether it is set. The list's invisible hit strips end a gutter short of it. Not a theme hook.",
    },
    CustomProp {
        name: "--moss-page-inline",
        owner: "main > *",
        default: "100vw",
        description: "`main`'s width, as a registered length (`@property`), computed where `main`'s children set it so a block inside a nested size container reads the page's width and not its container's. Outside `main` it is 100vw. Not a theme hook.",
    },
    CustomProp {
        name: "--moss-contents-column",
        owner: "body",
        default: "unset",
        description: "The page column before the contents list's lane narrows it: `--moss-content-width` as `body` resolves it, copied so the children of `body` can take the smaller of it and the room the lane leaves. Set only where `--moss-contents-lane` is. Not a theme hook.",
    },
    CustomProp {
        name: "--moss-contents-veil",
        owner: ".moss-contents-ruler-veil",
        default: "var(--moss-color-bg)",
        description: "The colour of the fade behind the open names of a `contents: pictures` list: the background colour of the block under the list's middle (a tinted band, or the page), read by the site runtime each time the list opens. Not a theme hook.",
    },
    CustomProp {
        name: "--moss-contents-gutter",
        owner: "body",
        default: "24px",
        description: "The clear space between the contents list's lane and the text column on a page with `contents: pictures`: part of `--moss-contents-lane`, and where the list's hit strips stop. Not a theme hook.",
    },
    CustomProp {
        name: "--moss-contents-row",
        owner: "body",
        default: "20px",
        description: "The height of one row of the contents ruler, at rest and in its hit strip; the title surfaces and the lens derive their sizes from it. The site runtime assumes 20px, so a theme should leave it.",
    },
    CustomProp {
        name: "--moss-contents-measure",
        owner: ".moss-contents-ruler",
        default: "34ch",
        description: "How wide an open name of a `contents: pictures` ruler may be; a longer title is ended with an ellipsis, never wrapped. A theme may set it on `.moss-contents-ruler`.",
    },
    CustomProp {
        name: "--moss-contents-ink",
        owner: ".moss-contents-ruler-label",
        default: "1",
        description: "How near an open row of a `contents: pictures` ruler is to the focus, from 0 to 1, written on its link: its name's ink runs from a floor that keeps every name legible at 0 to full at 1. Set by the site runtime (contents-lens.ts) while the list is open. Not a theme hook.",
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
        selector: "main :is(h1, h2, h3, h4, h5, h6)",
        name: "data-contents-label",
        values: &[],
        description: "The section's short name, from `{contents-label=\"Short name\"}` at the end of its heading (with `{#id}` and `contents=end` in any order). The contents ruler, the nav island's section name and its sections panel show it instead of the heading's text; the heading itself is unchanged. A name too long for its row is cut at its end with an ellipsis.",
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
