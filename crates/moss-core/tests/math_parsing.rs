//! P1 LaTeX math: parse + honest fallback.
//!
//! The class of bug this file exists to prevent: enabling
//! `Options::ENABLE_MATH` without handler arms makes pulldown-cmark emit
//! `Event::InlineMath` / `Event::DisplayMath`, which the typed-AST parser's
//! catch-all arms drop on the floor — `Energy $E = mc^2$.` renders as
//! `<p>Energy .</p>`. Silent deletion of the author's content. Every test
//! below is a guard against that, not a test of typesetting (there is no
//! engine in P1 — math renders as escaped source in `<code class="moss-math">`).
//!
//! See docs/archive/2026-07-21-latex-math-design.md §4 (P1) and ADR-030.

use moss_core::ast::{parse_with_config, render_document, DefaultHooks, ParseConfig};

fn render(markdown: &str, math: bool) -> String {
    let config = ParseConfig {
        math,
        ..Default::default()
    };
    let doc = parse_with_config(markdown, &config);
    render_document(&doc, &DefaultHooks::new())
}

// ---------------------------------------------------------------------------
// The regression guard — math must never be silently dropped
// ---------------------------------------------------------------------------

/// THE P1 TEST. Measured pre-fix behavior with ENABLE_MATH and no arms:
/// `<p>Energy .</p>` — the equation vanished. If this ever regresses, the
/// author's content is being deleted by the build.
#[test]
fn math_is_never_silently_dropped() {
    let html = render("Energy $E = mc^2$.", true);

    assert!(
        !html.contains("<p>Energy .</p>"),
        "math was SILENTLY DELETED (the ENABLE_MATH-without-arms bug): {html}"
    );
    assert!(
        html.contains("E = mc^2"),
        "the LaTeX source must survive to the output: {html}"
    );
    // The surrounding prose must be intact too.
    assert!(html.contains("Energy "), "prose lost: {html}");
}

// ---------------------------------------------------------------------------
// Inline math
// ---------------------------------------------------------------------------

#[test]
fn inline_math_renders_as_moss_math_code_span() {
    let html = render("$E = mc^2$", true);
    // The span carries the author's SOURCE, delimiters included — P1 ships no
    // typesetting engine, so dropping the `$` would silently eat two
    // characters of prose (see `ast::math_text::math_inline`).
    assert!(
        html.contains(r#"<code class="moss-math" data-moss-math="inline">$E = mc^2$</code>"#),
        "unexpected inline math markup: {html}"
    );
}

/// The reason the delimiters are kept, stated as a test: a `$`-span that is
/// really prose must survive byte-for-byte. `一个$5，两个$10` parses as math
/// (a full-width comma is non-whitespace, so it closes the span) and math is
/// on by default, so this is the common case, not the exotic one.
#[test]
fn a_false_positive_math_span_loses_no_characters() {
    let html = render("一个$5，两个$10", true);
    assert!(
        html.contains("一个") && html.contains("$5，两个$") && html.contains("10"),
        "currency prose lost characters to the math fallback: {html}"
    );
    let text: String = html
        .replace(r#"<code class="moss-math" data-moss-math="inline">"#, "")
        .replace("</code>", "");
    assert!(
        text.contains("一个$5，两个$10"),
        "reader-visible text must equal the author's source: {text}"
    );
}

#[test]
fn inline_math_is_inert_when_math_is_off() {
    let html = render("$E = mc^2$", false);
    assert!(
        !html.contains("moss-math"),
        "math:false must not parse math: {html}"
    );
    assert!(
        html.contains("$E = mc^2$"),
        "with math off the source must pass through literally: {html}"
    );
}

/// `ParseConfig::default()` must leave math OFF, so the ~40 in-crate
/// `parse()` callers and every existing snapshot fixture are unaffected.
/// Production opts in via `[site].math` on `SiteConfig`.
#[test]
fn math_defaults_to_off() {
    assert!(!ParseConfig::default().math);
    let doc = moss_core::ast::parse("$E = mc^2$");
    let html = render_document(&doc, &DefaultHooks::new());
    assert!(
        html.contains("$E = mc^2$"),
        "default parse() changed: {html}"
    );
}

// ---------------------------------------------------------------------------
// Display math
// ---------------------------------------------------------------------------

#[test]
fn display_math_renders_with_display_marker() {
    let html = render("$$ x^2 $$", true);
    assert!(
        html.contains(r#"data-moss-math="display""#),
        "display math must be marked as display: {html}"
    );
    assert!(html.contains("x^2"), "display TeX lost: {html}");
}

#[test]
fn display_math_block_survives_on_its_own_lines() {
    let html = render("Before\n\n$$\n\\frac{a}{b}\n$$\n\nAfter", true);
    assert!(html.contains(r#"data-moss-math="display""#), "{html}");
    assert!(html.contains(r"\frac{a}{b}"), "{html}");
    assert!(html.contains("Before"), "{html}");
    assert!(html.contains("After"), "{html}");
}

// ---------------------------------------------------------------------------
// Escaping — TeX is full of characters that are HTML metacharacters
// ---------------------------------------------------------------------------

#[test]
fn math_source_is_html_escaped() {
    let html = render("$a < b & c$", true);
    assert!(
        html.contains("a &lt; b &amp; c"),
        "TeX must be HTML-escaped: {html}"
    );
    // No raw metacharacter may survive inside the math span.
    let span = html
        .split(r#"data-moss-math="inline">"#)
        .nth(1)
        .and_then(|s| s.split("</code>").next())
        .unwrap_or_else(|| panic!("no math span in {html}"));
    assert_eq!(span, "$a &lt; b &amp; c$");
    // No raw `<`, and every `&` must open an entity (a bare `&` would mean
    // the escaper ran on `<` but not `&`, or ran twice on one and not the
    // other).
    assert!(!span.contains('<'), "raw `<` in math span: {span:?}");
    assert!(
        span.replace("&lt;", "")
            .replace("&amp;", "")
            .find('&')
            .is_none(),
        "unescaped `&` in math span: {span:?}"
    );
}

#[test]
fn math_cannot_inject_markup() {
    let html = render(r#"$</code><script>alert(1)</script>$"#, true);
    assert!(
        !html.contains("<script>"),
        "math span allowed script injection: {html}"
    );
}

// ---------------------------------------------------------------------------
// Wiring: math inside constructs whose inlines flow through
// `parse_inline_event`'s whitelist. `collect_item_blocks` is its ONLY
// caller, so **list items are the whole of that surface** — a missing
// whitelist entry drops math only there, while parse_inline alone looks
// green. This is the mechanism-vs-wiring guard.
//
// Table cells do NOT go through it: `Tag::TableCell` uses
// `collect_inlines_until` → `parse_inline` directly. The table-cell test
// below is a real regression test for that separate path, but it is not
// load-bearing for the whitelist — verified by deleting the two math arms,
// which fails the list-item test and leaves the table-cell test green.
// ---------------------------------------------------------------------------

#[test]
fn math_survives_inside_list_items() {
    let html = render("- energy $E = mc^2$ here\n- plain", true);
    assert!(
        html.contains("E = mc^2"),
        "math dropped inside a list item — parse_inline_event whitelist is \
         missing the math events: {html}"
    );
}

#[test]
fn math_survives_inside_a_table_cell() {
    let html = render("| a | b |\n|---|---|\n| $x^2$ | y |", true);
    assert!(
        html.contains("x^2"),
        "math dropped inside a table cell: {html}"
    );
}

/// The shortcode sub-parse used to call the default-config `parse()`, so a
/// math-on site rendered `$E=mc^2$` as an equation in prose and as literal
/// text one line later inside a `:::hero` or `:::grid` — the same page in
/// two dialects. These pin every sub-parse surface to the caller's config.
mod shortcode_bodies_inherit_the_callers_config {
    use moss_core::ast::parser::{parse_with_config, ParseConfig};

    fn math_on() -> ParseConfig {
        ParseConfig { math: true, ..Default::default() }
    }

    fn rendered(md: &str, config: &ParseConfig) -> String {
        let doc = parse_with_config(md, config);
        moss_core::ast::render::render_document(&doc, &moss_core::ast::hooks::DefaultHooks::new())
    }

    #[test]
    fn hero_overlay_typesets_math_like_surrounding_prose() {
        let html = rendered(":::hero\ntext $E=mc^2$ end\n:::\n", &math_on());
        // The discriminator is the math span, not the absence of `$` — the
        // P1 fallback deliberately keeps the delimiters, so "still literal"
        // now means "no moss-math wrapper", which
        // `math_off_leaves_shortcode_bodies_literal` covers from the far side.
        assert!(
            html.contains(r#"<code class="moss-math" data-moss-math="inline">$E=mc^2$</code>"#),
            "hero overlay kept literal $…$ while prose became math: {html}"
        );
    }

    #[test]
    fn grid_cell_typesets_math_like_surrounding_prose() {
        let html = rendered(":::grid\nEnergy is $E=mc^2$ here\n|\nsecond cell\n:::\n", &math_on());
        assert!(
            html.contains(r#"data-moss-math="inline">$E=mc^2$</code>"#),
            "grid cell kept literal $…$: {html}"
        );
    }

    #[test]
    fn math_off_leaves_shortcode_bodies_literal() {
        let html = rendered(":::hero\ntext $E=mc^2$ end\n:::\n", &ParseConfig::default());
        assert!(html.contains("$E=mc^2$"), "math=off must not typeset: {html}");
        assert!(!html.contains("moss-math"));
    }

    /// Once the config reaches the overlay, the equation becomes an
    /// `Inline::Other` — and the hero's plain-text walker feeds
    /// `<meta name="description">`. Fixing the leak without this arm would
    /// have converted a rendering inconsistency into silent data loss.
    #[test]
    fn hero_overlay_text_keeps_math_for_the_description_chain() {
        let mut doc = parse_with_config(
            ":::hero\nEnergy is $E=mc^2$ exactly.\n:::\n",
            &math_on(),
        );
        let extraction = moss_core::ast::extract_hero::extract_hero(
            &mut doc,
            &moss_core::ast::hooks::DefaultHooks::new(),
        )
        .expect("hero must be extracted");
        let text = extraction.overlay_text.expect("hero must yield overlay text");
        assert!(text.contains("$E=mc^2$"), "meta description lost the equation: {text:?}");
    }
}

/// P1's contract is that math is never *silently deleted*. Three text
/// collectors inside `parse_inline` matched `Event::Text`/`Event::Code`
/// and swallowed everything else, so an equation in an image caption, a
/// heading, or a callout title vanished from the page. Each of these
/// asserts the recovered markdown source, not merely "something survived".
mod plain_text_collectors_keep_math {
    use moss_core::ast::node::{Block, Inline};
    use moss_core::ast::parser::{parse_with_config, ParseConfig};

    fn math_on() -> ParseConfig {
        ParseConfig { math: true, ..Default::default() }
    }

    /// `alt` is an HTML attribute, so the equation must come back as source
    /// text — markup here would leak a tag into `alt=`. The `<figcaption>`
    /// is a different surface (option B, 2026-07): it is the alt CONTENT
    /// rendered as inline markdown, so the caption carries the typed math
    /// node (routed through `render_math` for typesetting), never the
    /// flattened source. Both directions asserted so neither surface can
    /// silently adopt the other's shape.
    #[test]
    fn image_alt_and_caption_keep_the_equation() {
        let doc = parse_with_config("![before $E=mc^2$ after](a.png)\n", &math_on());
        let Block::Figure { image, caption, .. } = &doc.blocks[0] else {
            panic!("expected a figure, got {:?}", doc.blocks[0]);
        };
        let Inline::Image { alt, .. } = image else {
            panic!("expected an image");
        };
        assert_eq!(alt, "before $E=mc^2$ after");
        assert!(!alt.contains('<'), "alt must stay plain text, got {alt:?}");

        let caption = caption.as_ref().expect("implicit figure must have a caption");
        assert!(
            caption
                .iter()
                .any(|i| matches!(i, Inline::Other(html) if html.contains("data-moss-math"))),
            "caption must carry the typed math node, got {caption:?}"
        );
        assert!(
            !caption
                .iter()
                .any(|i| matches!(i, Inline::Text(t) if t.contains("$E=mc^2$"))),
            "caption must not carry the flattened math source, got {caption:?}"
        );
    }

    /// Not a deletion but a shape change: the title truncated at the first
    /// `$` and the remainder fell into the callout body.
    #[test]
    fn callout_title_is_not_truncated_at_the_first_dollar() {
        let doc = parse_with_config("> [!note] Energy $E=mc^2$ explained\n> body\n", &math_on());
        let Block::Callout { title, children, .. } = &doc.blocks[0] else {
            panic!("expected a callout, got {:?}", doc.blocks[0]);
        };
        assert_eq!(title.as_deref(), Some("Energy $E=mc^2$ explained"));
        // The tail must not have spilled into the body.
        let body = format!("{children:?}");
        assert!(!body.contains("explained"), "title tail leaked into body: {body}");
    }

    /// The heading TEXT was always fine (`Inline::Other` is a raw
    /// passthrough); it was the id/anchor that lost the math.
    #[test]
    fn heading_renders_math_but_slugs_the_source() {
        let doc = parse_with_config("# Euler $e^{i\\pi}=-1$ identity\n", &math_on());
        let Block::Heading { id, children, .. } = &doc.blocks[0] else {
            panic!("expected a heading");
        };
        // Byte-identical to the math=OFF slug — enabling [site].math must
        // not silently rewrite a published anchor.
        assert_eq!(id.as_deref(), Some("euler-$e{ipi}=-1$-identity"));
        let off = parse_with_config("# Euler $e^{i\\pi}=-1$ identity\n", &ParseConfig::default());
        let Block::Heading { id: off_id, .. } = &off.blocks[0] else { panic!() };
        assert_eq!(id, off_id);
        // …while the visible heading still typesets the equation.
        assert!(children.iter().any(|c| matches!(c, Inline::Other(h) if h.contains("moss-math"))));
    }
}

// ---------------------------------------------------------------------------
// The published contract must describe what moss actually emits
// ---------------------------------------------------------------------------

/// The `.moss-math` component contract is what theme authors read, via
/// `moss describe` and `docs/reference/contract.md`. Nothing compared its
/// declared `example_html` to a real render, so when the P1 fallback
/// changed to keep the `$` delimiters, the contract kept advertising the
/// old bare-TeX output and no test noticed. `components_sync_test` cannot
/// catch this — it only matches `class="moss-..."` literals.
///
/// Mutation check: drop the `$` from either side of the contract's
/// `example_html` and this goes red.
#[test]
fn moss_math_contract_example_matches_a_real_render() {
    let entry = moss_core::contract::components::COMPONENTS
        .iter()
        .find(|c| c.class == "moss-math")
        .expect("the .moss-math component must be declared in COMPONENTS");

    let html = render(entry.example_markdown, true);

    assert!(
        html.contains(entry.example_html),
        "the declared example_html is not what moss emits for the declared \
         example_markdown.\n  markdown: {:?}\n  declared: {}\n  actual:   {}",
        entry.example_markdown,
        entry.example_html,
        html.trim()
    );
}
