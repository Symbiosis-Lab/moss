//! Pure extraction of a document's headings (text + slug + level) for the
//! editor's `[[Page#Heading]]` autocomplete. Reuses `parse()` (which runs
//! `assign_heading_id_suffixes`) so the returned slugs are byte-identical
//! to the rendered `<hN id="...">` attributes — the keystone invariant.
//!
//! The plain-text flattening is [`crate::ast::plain_text::inlines_to_plain_text`]
//! — shared with the event-stream walker the parser slugs from, so the label
//! this returns and the `<hN id>` cannot describe the heading differently.
//!
//! v1 extracts TOP-LEVEL headings only (the common case). Headings nested
//! inside callouts / blockquotes / lists are not offered for autocomplete;
//! a recursive walk is a follow-up if needed.

use crate::ast::parser::ParseConfig;
use crate::ast::plain_text::inlines_to_plain_text;
use crate::ast::{parse_with_config, Block};

/// A heading discovered in a document, in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadingInfo {
    /// Plain-text heading title (inline markup flattened).
    pub text: String,
    /// Final deduped slug — matches the rendered `<hN id="...">`.
    pub slug: String,
    /// Heading level 1..=6.
    pub level: u8,
}

/// Extract all top-level headings from `markdown` in document order, with
/// final (deduped) slugs identical to the rendered `<hN id>`.
///
/// Uses [`ParseConfig::default`], which has math **off**. A site with
/// `[site].math` on must call [`extract_headings_with_config`] instead:
/// with math off, `$…$` is ordinary text and the slug happens to come out
/// right, but that is a coincidence of this release's delimiter-preserving
/// design and not something callers should lean on.
pub fn extract_headings(markdown: &str) -> Vec<HeadingInfo> {
    extract_headings_with_config(markdown, &ParseConfig::default())
}

/// [`extract_headings`], parsing with the caller's [`ParseConfig`].
///
/// The keystone invariant is byte-identity with the rendered `<hN id>`, and
/// the render path parses with the *site's* config. Parsing here with a
/// different one is therefore a way to violate the invariant without
/// touching any slug logic, which is exactly what happened while this
/// function called the bare `parse()`.
pub fn extract_headings_with_config(markdown: &str, config: &ParseConfig) -> Vec<HeadingInfo> {
    let doc = parse_with_config(markdown, config);
    let mut out = Vec::new();
    for block in &doc.blocks {
        if let Block::Heading {
            level,
            children,
            id,
        } = block
        {
            let text = inlines_to_plain_text(children);
            out.push(HeadingInfo {
                text: text.trim().to_string(),
                slug: id.clone().unwrap_or_default(),
                level: *level,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_text_slug_level() {
        let md = "# Title\n\n## Getting Started\n\ntext\n\n### Sub *em*\n";
        let hs = extract_headings(md);
        assert_eq!(hs.len(), 3);
        assert_eq!(hs[0], HeadingInfo { text: "Title".into(), slug: "title".into(), level: 1 });
        assert_eq!(hs[1], HeadingInfo { text: "Getting Started".into(), slug: "getting-started".into(), level: 2 });
        assert_eq!(hs[2], HeadingInfo { text: "Sub em".into(), slug: "sub-em".into(), level: 3 });
    }

    #[test]
    fn dedups_duplicate_slugs() {
        let md = "## Setup\n\n## Setup\n";
        let hs = extract_headings(md);
        assert_eq!(hs[0].slug, "setup");
        assert_eq!(hs[1].slug, "setup-1");
    }

    #[test]
    fn preserves_cjk() {
        let md = "## 中文标题\n";
        let hs = extract_headings(md);
        assert_eq!(hs[0].slug, "中文标题");
        assert_eq!(hs[0].text, "中文标题");
    }

    #[test]
    fn empty_doc_no_headings() {
        assert!(extract_headings("just a paragraph\n").is_empty());
    }

    #[test]
    fn slug_matches_obsidian_anchor_for_punctuation() {
        // Keystone: slug must equal what obsidian_heading_anchor produces
        // (the same fn the renderer uses for id=). Spot-check a heading with
        // punctuation that the algorithm keeps/strips distinctively.
        let md = "## Step 1: Install\n";
        let hs = extract_headings(md);
        assert_eq!(hs[0].slug, crate::heading::anchor::obsidian_heading_anchor("Step 1: Install"));
    }

    /// The keystone invariant, with math in the heading — the case that
    /// broke it. The slug the walker extracts, the `<hN id>` the renderer
    /// emits, and the raw-line slug the wikilink graph computes in
    /// `build/scan/scan.rs` must agree byte-for-byte. That is the invariant
    /// that holds unconditionally, because all three see the same `$` bytes.
    ///
    /// **Agreement with the math=OFF slug is NOT universal**, and asserting
    /// it as such was wrong. With math off the TeX is ordinary markdown, so
    /// any markdown-active character inside it is consumed before slugging:
    /// `$f*g$ and $h*k$` has its two `*` eaten as emphasis, and `$V^*$ and
    /// $W^*$` — plain dual-space notation — likewise. See the `*` cases
    /// below and ADR-030 §"Upgrade-time anchor movement".
    #[test]
    fn math_heading_slug_is_identical_across_every_surface() {
        let md = "# Euler $e^{i\\pi}=-1$ identity\n";
        let math_on = ParseConfig { math: true, ..Default::default() };

        let extracted = extract_headings_with_config(md, &math_on);
        assert_eq!(extracted.len(), 1);

        // 1 ↔ 2: extracted slug == the id the renderer emits.
        let doc = crate::ast::parse_with_config(md, &math_on);
        let Block::Heading { id, .. } = &doc.blocks[0] else {
            panic!("expected a heading, got {:?}", doc.blocks[0]);
        };
        assert_eq!(extracted[0].slug, *id.as_ref().expect("heading must have an id"));

        // 3: the wikilink graph slugs the RAW heading line. THIS is the
        // strong one — a mismatch resolves a link to a fragment the page
        // does not have.
        assert_eq!(
            extracted[0].slug,
            crate::heading::anchor::obsidian_heading_anchor("Euler $e^{i\\pi}=-1$ identity")
        );

        // 4: this TeX has no markdown-active characters, so the math=OFF
        // slug happens to coincide too. Conditional, not universal.
        let off = extract_headings_with_config(md, &ParseConfig::default());
        assert_eq!(
            extracted[0].slug, off[0].slug,
            "TeX with no markdown-active characters must slug identically either way"
        );

        // And the human-readable label keeps the equation rather than
        // showing a hole where it used to be.
        assert_eq!(extracted[0].text, "Euler $e^{i\\pi}=-1$ identity");
        assert_eq!(extracted[0].text, off[0].text);
    }

    /// Pins the exception, so nobody re-asserts the false universal.
    ///
    /// `*` inside TeX is emphasis to a math-OFF parser. Turning `[site].math`
    /// on therefore MOVES these anchors — a real, user-visible upgrade cost
    /// recorded in ADR-030 and the moss-core CHANGELOG. What must still hold
    /// is graph agreement: math-ON slug == the raw-line slug the wikilink
    /// scanner computes, so links and anchors never disagree on a live site.
    #[test]
    fn markdown_active_chars_in_tex_move_the_anchor_but_keep_graph_agreement() {
        let math_on = ParseConfig { math: true, ..Default::default() };
        for (md, raw, expect_off) in [
            (
                "# Convolution $f*g$ and $h*k$ end\n",
                "Convolution $f*g$ and $h*k$ end",
                "convolution-$fg$-and-$hk$-end",
            ),
            ("# Dual $V^*$ and $W^*$ end\n", "Dual $V^*$ and $W^*$ end", "dual-$v$-and-$w$-end"),
        ] {
            let on = extract_headings_with_config(md, &math_on);
            let off = extract_headings_with_config(md, &ParseConfig::default());

            // Graph agreement — unconditional.
            assert_eq!(
                on[0].slug,
                crate::heading::anchor::obsidian_heading_anchor(raw),
                "math-ON slug diverged from the raw-line slug the wikilink graph computes"
            );

            // The documented divergence: emphasis ate the `*` with math off.
            assert_eq!(off[0].slug, expect_off, "math-OFF slug drifted from what ADR-030 records");
            assert_ne!(
                on[0].slug, off[0].slug,
                "expected this heading's anchor to MOVE when math is enabled"
            );
        }
    }

    /// `$$…$$` has no markdown-active characters here, so both slugs agree.
    #[test]
    fn display_math_in_a_heading_keeps_both_delimiters() {
        let math_on = ParseConfig { math: true, ..Default::default() };
        let hs = extract_headings_with_config("# Case $$a+b$$ tail\n", &math_on);
        assert_eq!(hs[0].text, "Case $$a+b$$ tail");
        assert_eq!(
            hs[0].slug,
            crate::heading::anchor::obsidian_heading_anchor("Case $$a+b$$ tail"),
            "graph agreement — the invariant that always holds"
        );
        assert_eq!(
            hs[0].slug,
            extract_headings_with_config("# Case $$a+b$$ tail\n", &ParseConfig::default())[0].slug
        );
    }

    /// Two headings differing only inside their math must stay distinct.
    /// Dropping the TeX collapsed them to the same base slug, so the second
    /// silently acquired a `-1` suffix and the anchors became order-dependent.
    #[test]
    fn headings_differing_only_inside_math_do_not_collide() {
        let math_on = ParseConfig { math: true, ..Default::default() };
        let hs = extract_headings_with_config("## Case $a$\n\n## Case $b$\n", &math_on);
        assert_ne!(hs[0].slug, hs[1].slug);
        assert!(!hs[1].slug.ends_with("-1"), "slug {:?} collided", hs[1].slug);
    }

    /// The keystone invariant across a shortcode boundary. A `:::grid` cell
    /// is parsed by a recursive `parse_fragment_with_config`, which is told
    /// to SKIP `assign_heading_id_suffixes` precisely so its heading arrives
    /// holding the bare slug — the cell renders into the SAME page, so the
    /// page's own parse must be the only pass that numbers. When the nested
    /// parse numbered too, a cell was disambiguated against its own private
    /// counter and then again against the page's, which both re-collided and
    /// produced impossible shapes like `notes-1-1`.
    ///
    /// The counter has to be shared, or the page emits two `id="notes"` and
    /// the browser resolves `#notes` to whichever comes first in the DOM
    /// (the card), never the author's section.
    #[test]
    fn grid_cell_heading_shares_the_page_id_counter() {
        let md = ":::grid\n### Notes\n:::\n\n## Notes\n";
        let hs = extract_headings(md);
        assert_eq!(hs.len(), 1, "only top-level headings are offered: {hs:?}");
        assert_eq!(
            hs[0].slug, "notes-1",
            "the body section must report the id it actually renders with"
        );

        let mut doc = crate::ast::parse(md);
        crate::ast::classify_remaining_urls(&mut doc);
        let html = crate::ast::render_document(&doc, &crate::ast::DefaultHooks::new());
        assert_eq!(
            html.matches(r#"id="notes""#).count(),
            1,
            "duplicate DOM id: {html}"
        );
        assert!(
            html.contains(r#"id="notes-1""#),
            "the body heading lost its disambiguated id: {html}"
        );
    }

    /// Every `id=` a page publishes, in DOM order.
    fn rendered_ids(md: &str) -> Vec<String> {
        let mut doc = crate::ast::parse(md);
        crate::ast::classify_remaining_urls(&mut doc);
        let html = crate::ast::render_document(&doc, &crate::ast::DefaultHooks::new());
        let mut ids = Vec::new();
        let mut rest = html.as_str();
        while let Some(at) = rest.find("id=\"") {
            let Some(after) = rest.get(at + 4..) else { break };
            let Some(end) = after.find('"') else { break };
            let Some(id) = after.get(..end) else { break };
            ids.push(id.to_string());
            rest = after.get(end + 1..).unwrap_or("");
        }
        ids
    }

    /// One cell holding TWO same-titled headings. `grid_cell_heading_shares_
    /// the_page_id_counter` puts one heading per cell, where the nested parse
    /// assigns no suffix at all — so it cannot see a cell that arrives already
    /// carrying `notes-1` and gets suffixed a second time by the page walk.
    /// Assert on the whole id set, not one slug: the collision lands on
    /// whatever slug the double-suffixing produced, which counting `id="notes"`
    /// never sees.
    #[test]
    fn a_cell_holding_two_same_titled_headings_still_yields_unique_ids() {
        for (name, md) in [
            ("grid", "## Notes\n\n:::grid\n### Notes\n\n### Notes\n:::\n"),
            (
                "hero overlay",
                ":::hero\ncover.jpg\n---\n### Notes\n\n### Notes\n:::\n\n## Notes\n",
            ),
            (
                "compound-link card",
                ":::grid\n[### Notes\n\n### Notes](https://example.com/a)\n:::\n\n## Notes\n",
            ),
        ] {
            let ids = rendered_ids(md);
            let unique: std::collections::HashSet<&String> = ids.iter().collect();
            assert_eq!(
                unique.len(),
                ids.len(),
                "{name}: duplicate DOM id among {ids:?}"
            );
        }
    }

    /// A slug rule can never mint `notes-1-1`; only a second suffixing pass
    /// over an already-suffixed id can. Two cells, two same-titled headings
    /// each, is the shape that made it visible.
    #[test]
    fn no_id_carries_a_doubled_suffix() {
        let ids = rendered_ids(":::grid 2\n### Notes\n\n### Notes\n+++\n### Notes\n\n### Notes\n:::\n\n## Notes\n");
        for id in &ids {
            assert!(
                !id.contains("-1-"),
                "id {id:?} was suffixed twice — the nested parse numbered it \
                 and the page walk numbered it again: {ids:?}"
            );
        }
        let unique: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "duplicate DOM id among {ids:?}");
    }
}
