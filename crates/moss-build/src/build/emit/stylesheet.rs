//! The site stylesheet: token layer + core rules + the partials this build
//! actually needs.
//!
//! One emit family, one owner — `blocking.rs` gates, this module assembles,
//! per NORTH-STAR's interception rule ("new emit family → `emit/`-shaped
//! module, never more code in `blocking.rs`").
//!
//! ## The gate is declared, never inferred
//!
//! A partial ships because [`SiteAssets`] says the build uses it, and
//! `SiteAssets` is the union of parse-time facts. It is never decided by
//! scanning emitted HTML for class names. Roughly 60% of the `moss-*` classes
//! in `site.css` are attached at *runtime* by `search.js` / `theme.js`
//! (`createElement`), so a content scan — PurgeCSS, Tailwind-style JIT —
//! deletes the search dialog and the link-preview popover, and the failure is
//! invisible until a reader opens the feature. A safelist then re-encodes by
//! hand what the source already knows, in a second place that drifts.
//!
//! ## Why partials may be appended rather than spliced in place
//!
//! Every rule in `site.css` between the `@layer shortcodes {` opener and its
//! closer is in one cascade layer, so moving a block within that layer is
//! only safe if the move cannot flip which of two rules wins. Each partial
//! re-opens `@layer shortcodes` (repeat blocks of one layer name merge), and
//! `stylesheet_tests::a_partial_only_styles_elements_it_owns` mechanises the
//! condition: **a partial may only style elements it owns.** Every rule it
//! declares must either carry a `.class`/`[attr]` token that appears in no
//! core selector, or declare only custom properties core never declares.
//! A rule that can only match elements core has never heard of cannot change
//! the outcome of any contest core takes part in.
//!
//! That test is the gate for adding a partial, and it is what made the
//! `search` extraction safe rather than assumed. The section's comment header
//! spanned 507 lines, but search's own rules are the first 237; the rest —
//! `.theme-toggle-icon`, `.github-link`,
//! `.mobile-menu-button`, the reading-font control — is core nav chrome that
//! had been filed under the wrong heading and stayed in core. Splitting at the
//! comment boundary would have moved core's nav rules into a partial that most
//! sites never ship.
//!
//! Ownership replaced an earlier rule that compared `(selector, property)`
//! pairs for equality, which was unsound in the dangerous direction: selector
//! *strings* are not element *sets*, so core's `:is(h1, h2, h3)` and a
//! partial's `h2` compared as disjoint while fighting over the same elements
//! (`.a .b` vs `.b`, `*` and `:where()` are the same hole). Deciding overlap
//! in general needs a real selector engine; requiring a marker core has never
//! seen sidesteps the question.

use crate::build::types::SiteAssets;

/// One optional stylesheet partial and the fact that turns it on.
pub struct CssPartial {
    /// Basename in `assets/css/site/`, without `.css`.
    pub name: &'static str,
    /// The file's bytes, `include_str!`-ed at compile time.
    pub source: &'static str,
    /// Reads the site-level fact that gates this partial.
    pub gate: fn(&SiteAssets) -> bool,
}

/// Every optional partial, in the order they are concatenated after core.
///
/// A registration table in the NORTH-STAR pattern-glossary #2 sense: one
/// table per family, with a totality test beside it
/// (`stylesheet_tests::every_partial_file_has_a_row`), explicitly not a
/// macro engine.
pub const CSS_PARTIALS: &[CssPartial] = &[
    CssPartial {
        name: "callouts",
        source: include_str!("../../assets/css/site/callouts.css"),
        gate: |a| a.callouts,
    },
    CssPartial {
        name: "search",
        source: include_str!("../../assets/css/site/search.css"),
        gate: |a| a.search,
    },
    CssPartial {
        name: "sidenotes",
        source: include_str!("../../assets/css/site/sidenotes.css"),
        gate: |a| a.has_footnotes,
    },
    CssPartial {
        name: "vertical",
        source: include_str!("../../assets/css/site/vertical.css"),
        gate: |a| a.vertical,
    },
];

/// The core stylesheet — everything not behind a gate.
///
/// Aliases `template::DEFAULT_CSS` rather than `include_str!`-ing the same
/// file a second time: two embeddings of one file are two things to keep in
/// step, and the template module's constant is the one the render tests
/// already read.
pub const CORE_CSS: &str = crate::build::page::shell::DEFAULT_CSS;

/// The mark and the name beside it — ink colour, the green drop's dark lift,
/// the Chinese face — shared with the launcher, which `@import`s the same
/// file into its own bundle. Appended after core, inside the layer core's
/// rules live in, so a theme keeps the same standing over it as over the
/// colophon rules it serves.
///
/// The `@layer shortcodes {` wrapper is written here rather than in the file
/// because the file has two consumers and only one of them has layers: the
/// launcher's bundle declares none, and a stray `@layer` there would sink the
/// mark's rules below every unlayered rule in the app. So the site states the
/// layer it wants at the point it appends, and the file stays layer-free.
pub const MARK_CSS: &str = include_str!("../../assets/css/mark.css");

/// Cascade-layer order. Must precede every `@layer` block below it.
const LAYER_ORDER: &str = "@layer reset, tokens, base, layout, shortcodes, plugins, themes;\n";

/// The Chinese wordmark's face: Long Cang, a 行書 running hand, subset to the
/// two characters 青苔 is made of and nothing else — 1188 bytes.
///
/// Inlined as a `data:` URI rather than emitted as a hashed asset because it
/// is smaller than the request that would fetch it: 1.6 KB of base64 against a
/// second round trip on every page. A built moss site makes no third-party
/// request and emits no font of its own today; this keeps both true.
///
/// Regenerate from the upstream TTF (google/fonts `ofl/longcang`) with:
///
/// ```text
/// pyftsubset LongCang-Regular.ttf --text=青苔 --flavor=woff2 \
///   --layout-features= --no-hinting --desubroutinize --name-IDs= \
///   --output-file=src/assets/fonts/LongCang-colophon.woff2
/// ```
///
/// OFL 1.1; the licence ships beside it as `LongCang-LICENSE.txt`.
const COLOPHON_FONT: &[u8] = include_bytes!("../../assets/fonts/LongCang-colophon.woff2");

/// The two characters [`COLOPHON_FONT`] contains, as a CSS `unicode-range`.
///
/// This is the guard that makes a two-glyph subset safe to name in a font
/// stack. A face this narrow has no `.notdef` worth showing, and a browser
/// asked to set a character it lacks draws a blank box rather than falling
/// through — so the range confines it to exactly what it has, and every other
/// character reaches the body stack as if the face were not there. Should the
/// wordmark ever gain a character, the label degrades to the body font instead
/// of losing a glyph. `colophon_font_covers_the_wordmark` pins the range
/// against the wordmark strings so the two cannot drift apart silently.
pub const COLOPHON_FONT_RANGE: &[char] = &['青', '苔'];

/// `@font-face` for the colophon wordmark, generated rather than written into
/// `site.css` so the font file stays the single source of its own bytes.
///
/// Unlayered, like the PingFang weight pins in `site.css`: `@font-face` matches
/// no element, so it takes no part in the cascade, but declaring it inside a
/// layer would make it lose to a same-named face declared outside one.
fn colophon_font_face() -> String {
    use base64::Engine as _;
    format!(
        "@font-face{{font-family:'Moss Long Cang';src:url(data:font/woff2;base64,{}) \
format('woff2');font-weight:400;font-display:swap;unicode-range:{}}}\n",
        base64::engine::general_purpose::STANDARD.encode(COLOPHON_FONT),
        COLOPHON_FONT_RANGE
            .iter()
            .map(|c| format!("U+{:X}", *c as u32))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

/// Assemble and minify the stylesheet for a build.
///
/// Layout: layer order → `@layer tokens` (generated from `tokens.json`) →
/// the colophon `@font-face` (generated from the font file) → core rules →
/// [`MARK_CSS`] → each enabled partial.
///
/// At the carve (2026-08-04) an all-true [`SiteAssets`] reproduced the
/// pre-partition rule set exactly — verified by diffing the two extracted
/// ranges against `git show HEAD:…/site.css` byte-for-byte. That is a property
/// of one commit, not an invariant a test can carry: pinning it needs a frozen
/// copy of the old sheet, which the next `site.css` edit invalidates. What is
/// enforced going forward is the ownership rule below, which is what makes the
/// *placement* safe; losslessness of a future carve is a review question.
///
/// # Panics
///
/// If `tokens.json` does not parse — the same contract the previous inline
/// assembly in `blocking.rs` had. It is a compile-time-embedded artifact, so
/// a parse failure is a broken build of moss itself, not a bad vault.
pub fn assemble(assets: &SiteAssets) -> String {
    use moss_core::contract::tokens::{
        format_dark_root_block, format_root_block, load_tokens,
    };
    let tokens =
        load_tokens().unwrap_or_else(|e| panic!("tokens.json must parse at build time: {}", e));
    let tokens_layer = format!(
        "@layer tokens {{\n{}{}}}\n",
        format_root_block(&tokens),
        format_dark_root_block(&tokens),
    );

    let mut full = String::with_capacity(CORE_CSS.len() + 8 * 1024);
    full.push_str(LAYER_ORDER);
    full.push_str(&tokens_layer);
    full.push_str(&colophon_font_face());
    full.push_str(CORE_CSS);
    full.push_str("\n@layer shortcodes {\n");
    full.push_str(MARK_CSS);
    full.push_str("}\n");
    for partial in CSS_PARTIALS.iter().filter(|p| (p.gate)(assets)) {
        full.push('\n');
        full.push_str(partial.source);
    }
    crate::build::page::shell::minify_css(&full)
}

#[cfg(test)]
#[path = "stylesheet_tests.rs"]
mod tests;
