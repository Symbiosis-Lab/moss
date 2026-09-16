//! The P1 math fallback node and its inverse.
//!
//! P1's contract is that math is **never silently deleted**. Two shapes
//! carry an equation through the AST, and this module owns both plus the
//! conversion between them, so the format and its parser cannot drift:
//!
//! | Shape | Built by | Used where |
//! |---|---|---|
//! | `<code class="moss-math" data-moss-math="…">` in [`Inline::Other`] | [`math_inline`] | rendered HTML |
//! | `$…$` / `$$…$$` markdown source | [`math_source`] | every plain-text collector |
//!
//! **Why plain-text collectors get the source and not bare TeX.** pulldown
//! hands a walker the *inner* TeX only — delimiters stripped, inner bytes
//! otherwise untouched — so `$` + tex + `$` reproduces the author's
//! original bytes exactly. Restoring the delimiters is what keeps three
//! independent surfaces in agreement:
//!
//! 1. **The math=off slug.** `# Euler $e^{i\pi}=-1$ identity` must produce
//!    the same `<h1 id>` whether or not `[site].math` is on, or flipping
//!    the flag silently breaks every existing deep link, in-page TOC entry
//!    and `[[Page#Heading]]` wikilink pointing at that heading.
//! 2. **The wikilink graph.** `build/scan/scan.rs` slugs headings off the
//!    RAW heading line; being a line scanner it cannot know where math
//!    begins, so it always includes the `$` bytes. The render side has to
//!    match it, or the graph resolves a link to a fragment the page lacks.
//! 3. **`heading::extract`**, whose module doc calls byte-identity with the
//!    rendered `<hN id>` "the keystone invariant".
//!
//! Bare TeX would satisfy none of the three. Markup would be actively wrong
//! in these contexts — `alt` is an HTML attribute and the slug feeds a URL
//! fragment.

use super::hooks::escape_text;
use super::node::Inline;

const PREFIX_INLINE: &str = r#"<code class="moss-math" data-moss-math="inline">"#;
const PREFIX_DISPLAY: &str = r#"<code class="moss-math" data-moss-math="display">"#;
const SUFFIX: &str = "</code>";

/// Build the P1 math fallback node: the equation's own markdown source —
/// delimiters included — HTML-escaped, in a marked `<code>` span.
///
/// The `data-moss-math` attribute carries display-vs-inline so a later
/// phase's renderer can typeset from the AST without re-deriving it, and so
/// the CSS can size display math differently without a second class.
///
/// **Why the `$` delimiters are kept.** P1 ships no typesetting engine, so
/// this span is what the reader actually sees. Emitting the bare inner TeX
/// would silently swallow two characters of the author's prose, which is the
/// same content-loss P1 exists to prevent — just moved from "equation
/// deleted" to "delimiters deleted". It is invisible for a real equation and
/// destructive for a false positive:
///
/// ```text
/// 一个$5，两个$10     bare TeX → 一个5，两个10      (prices corrupted)
///                     source   → 一个$5，两个$10    (byte-identical)
/// ```
///
/// pulldown's close rule fires on any non-whitespace byte, so unspaced CJK
/// currency parses as math (`moss doctor --math` exists to surface exactly
/// this), and `[site].math` defaults on — the false positive is the case to
/// optimize for. Keeping the delimiters also makes this node agree with
/// [`math_source`], so an equation has ONE spelling across the body, image
/// alt text, heading slugs and meta descriptions instead of two.
///
/// P2/P3 replace this span with typeset SVG, at which point the delimiters
/// disappear along with the fallback.
pub(crate) fn math_inline(tex: &str, display: bool) -> Inline {
    let prefix = if display { PREFIX_DISPLAY } else { PREFIX_INLINE };
    Inline::Other(format!("{prefix}{}{SUFFIX}", escape_text(&math_source(tex, display))))
}

/// Reconstruct an equation's markdown source — the TeX with its `$` / `$$`
/// delimiters restored — from the inner TeX pulldown hands the walker.
///
/// `pub` because plain-text collectors are not confined to this crate: the
/// email walker in `moss::infra::newsletter` builds an image `alt` attribute
/// and needs the same restored-delimiter form, or the two surfaces disagree
/// about what an equation looks like in plain text.
pub fn math_source(tex: &str, display: bool) -> String {
    let delim = if display { "$$" } else { "$" };
    format!("{delim}{tex}{delim}")
}

/// Recover the markdown source of a math node from an [`Inline::Other`]
/// payload, or `None` if the payload is some other raw-HTML passthrough.
///
/// The inverse of [`math_inline`]. Plain-text walkers that see the AST
/// rather than the event stream (`heading::text::inlines_to_text`, the
/// crate's one AST walker — `extract_hero` delegates to it) have no access
/// to the original event, so recovering from the node is the only way to
/// honor P1's never-silently-delete contract. Round-tripping is pinned by
/// `math_source_round_trips_through_the_node` below — that test is what
/// keeps this from drifting away from the builder three lines above it.
pub(crate) fn math_source_from_other(html: &str) -> Option<String> {
    let prefix = if html.starts_with(PREFIX_DISPLAY) {
        PREFIX_DISPLAY
    } else if html.starts_with(PREFIX_INLINE) {
        PREFIX_INLINE
    } else {
        return None;
    };
    // The node's payload is ALREADY the delimited source — `math_inline`
    // builds it with `math_source` — so this only has to unescape. Re-adding
    // the delimiters here would double them (`$$E=mc^2$$` for inline math).
    Some(
        html.strip_prefix(prefix)?
            .strip_suffix(SUFFIX)?
            // Only the three characters `escape_text` writes, unescaped in the
            // order that makes `&amp;lt;` come back as the literal `&lt;`
            // rather than as `<`.
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&"),
    )
}

/// Decode a math [`Inline::Other`] node into `(inner_tex, display)` — the raw
/// LaTeX the author typed (delimiters stripped, unescaped) and whether it was
/// `$$…$$` (display) or `$…$` (inline), or `None` for any non-math passthrough.
///
/// This is what lets the renderer route a math node through
/// [`RenderHooks::render_math`](crate::ast::RenderHooks::render_math) without a
/// dedicated `Inline::Math` AST variant (ADR-030 D3): the P1 node already
/// carries the source verbatim, so P2's typesetter recovers the exact bytes the
/// engine needs. Round-tripping `math_inline` → this is pinned by
/// `node_parts_round_trips` below.
///
/// `pub` for the same reason as [`math_source`]: the email HTML renderer in
/// `moss::infra::newsletter` walks `Document` directly (ADR-036) rather than
/// going through `RenderHooks`, so it decodes `Inline::Other` math nodes here
/// to route them through its own hosted-PNG math path, falling back to the
/// escaped source on refusal — the same three-question gate `render_math`
/// documents, just applied by a second, email-specific lowering.
pub fn math_node_parts(html: &str) -> Option<(String, bool)> {
    let source = math_source_from_other(html)?;
    // `math_source_from_other` returns the *delimited* source ($tex$ / $$tex$$);
    // `math_source_from_other` already told prefix-vs-display via the same
    // prefix check, so recover `display` the same way and strip the matching
    // delimiter off both ends. `$$` before `$` so display is not mis-read.
    let display = html.starts_with(PREFIX_DISPLAY);
    let delim = if display { "$$" } else { "$" };
    let inner = source
        .strip_prefix(delim)
        .and_then(|s| s.strip_suffix(delim))?;
    Some((inner.to_string(), display))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_parts_round_trips() {
        for tex in ["E=mc^2", "a < b", "S = \\{x : x > 0\\}", "\\frac{a}{b}"] {
            for display in [false, true] {
                let Inline::Other(html) = math_inline(tex, display) else {
                    panic!("math_inline must build an Inline::Other");
                };
                assert_eq!(
                    math_node_parts(&html),
                    Some((tex.to_string(), display)),
                    "round-trip failed for {tex:?} display={display}"
                );
            }
        }
    }

    #[test]
    fn node_parts_rejects_non_math() {
        assert_eq!(math_node_parts("<div>hi</div>"), None);
        assert_eq!(math_node_parts("<code>plain</code>"), None);
    }

    #[test]
    fn source_restores_delimiters() {
        assert_eq!(math_source("E=mc^2", false), "$E=mc^2$");
        assert_eq!(math_source("a+b", true), "$$a+b$$");
    }

    #[test]
    fn math_source_round_trips_through_the_node() {
        // Every TeX here exercises a character `escape_text` rewrites, so a
        // change to either side of the escape pair fails this test.
        for tex in ["E=mc^2", "a < b", "a > b", "x &amp; y", "S = \\{x : x > 0\\}"] {
            for display in [false, true] {
                let Inline::Other(html) = math_inline(tex, display) else {
                    panic!("math_inline must build an Inline::Other");
                };
                assert_eq!(
                    math_source_from_other(&html).as_deref(),
                    Some(math_source(tex, display).as_str()),
                    "round-trip failed for {tex:?} display={display}"
                );
            }
        }
    }

    #[test]
    fn non_math_passthrough_is_not_claimed() {
        assert_eq!(math_source_from_other("<div>hello</div>"), None);
        assert_eq!(math_source_from_other("<code>plain</code>"), None);
        // A `moss-math-error` node (P2) must not be mistaken for a fallback.
        assert_eq!(
            math_source_from_other(r#"<code class="moss-math-error">x</code>"#),
            None
        );
    }
}
