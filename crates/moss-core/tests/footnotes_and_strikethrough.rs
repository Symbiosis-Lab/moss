//! Strikethrough and footnotes: markdown in, HTML out.
//!
//! The class of bug this file exists to prevent: `parser_options` turns on
//! `ENABLE_STRIKETHROUGH` and `ENABLE_FOOTNOTES`, so pulldown-cmark emits
//! `Tag::Strikethrough`, `Event::FootnoteReference` and
//! `Tag::FootnoteDefinition` — and between 2026-05-28 and this fix the typed
//! AST modeled none of them, so every one hit a catch-all. Measured:
//! `~~gone~~ stays` → `<p>gone stays</p>`; `Text[^1].\n\n[^1]: b` →
//! `<p>Text.</p>\n<p>b</p>`; `- ~~gone~~ stays` → `<li><p>gone</p><p>
//! stays</p></li>`. Silent deletion and structural corruption.
//!
//! Every test renders through `render_document`, the production path. Unit
//! tests over hand-built `Block`s cannot catch the wiring failures here: the
//!1598 in-crate tests all passed while the bug shipped for two months,
//! because zero of them contained a `~~` or a `[^`.
//!
//! See ADR-035.

use moss_core::ast::{classify_remaining_urls, parse, render_document, DefaultHooks};
use std::collections::HashSet;

fn render(markdown: &str) -> String {
    render_document(&parse(markdown), &DefaultHooks::new())
}

/// `render` for markdown carrying a URL (a `:::hero` media line, a link).
///
/// `render_document` debug-asserts on any `Url::Unresolved` reaching it, so a
/// caller must classify between parse and render — production does this via
/// `resolve_urls`. Tests that skip it panic inside the hero hook rather than
/// failing on the thing they assert.
fn render_with_urls(markdown: &str) -> String {
    let mut doc = parse(markdown);
    classify_remaining_urls(&mut doc);
    render_document(&doc, &DefaultHooks::new())
}

// ---------------------------------------------------------------------------
// The regression guards
// ---------------------------------------------------------------------------

#[test]
fn strikethrough_is_never_silently_dropped() {
    let html = render("~~gone~~ stays");
    assert!(
        !html.contains("<p>gone stays</p>"),
        "strikethrough markup was SILENTLY DELETED: {html}"
    );
    assert_eq!(html, "<p><del>gone</del> stays</p>\n");
}

#[test]
fn footnote_definition_is_never_demoted_to_prose() {
    let html = render("Text[^1].\n\n[^1]: the note\n");
    assert!(
        !html.contains("<p>Text.</p>"),
        "the footnote marker was SILENTLY DELETED: {html}"
    );
    assert!(
        html.contains(r##"<sup class="moss-footnote-ref" id="fnref-1" tabindex="-1"><a href="#fn-1" role="doc-noteref">1</a></sup>"##),
        "expected a numbered marker: {html}"
    );
    assert!(
        html.contains(r#"<section class="moss-footnotes" role="doc-endnotes">"#),
        "expected an endnote section: {html}"
    );
    assert!(html.contains("the note"), "note body lost: {html}");
}

#[test]
fn footnote_marker_and_endnote_item_are_focusable_targets() {
    let html = render("Text[^1].\n\n[^1]: the note\n");
    assert!(
        html.contains(
            r##"<sup class="moss-footnote-ref" id="fnref-1" tabindex="-1">"##
        ),
        "marker <sup> must carry tabindex=\"-1\" so #fnref-1 fragment \
         navigation can focus it: {html}"
    );
    assert!(
        html.contains(r##"<li id="fn-1" tabindex="-1">"##),
        "endnote <li> must carry tabindex=\"-1\" so #fn-1 fragment \
         navigation can focus it: {html}"
    );
}

// ---------------------------------------------------------------------------
// Strikethrough in every inline-bearing container
// ---------------------------------------------------------------------------

#[test]
fn strikethrough_in_tight_list_item_keeps_one_paragraph() {
    // Pre-fix this split the item in two: `<li><p>gone</p><p> stays</p></li>`.
    // A missing arm in `parse_inline_event` does not merely delete, it
    // restructures — which is why the tight-list shape is asserted whole.
    let html = render("- ~~gone~~ stays\n- b\n");
    assert_eq!(
        html,
        "<ul>\n<li><del>gone</del> stays</li>\n<li>b</li>\n</ul>\n"
    );
}

#[test]
fn strikethrough_in_table_cell_survives() {
    let html = render("| a |\n| --- |\n| ~~x~~ y |\n");
    assert!(html.contains("<td><del>x</del> y</td>"), "got: {html}");
}

#[test]
fn strikethrough_nested_in_strong_survives() {
    let html = render("**bold ~~struck~~**");
    assert_eq!(html, "<p><strong>bold <del>struck</del></strong></p>\n");
}

#[test]
fn strikethrough_in_blockquote_and_heading_survives() {
    let html = render("## ~~old~~ new\n\n> ~~quoted~~ out\n");
    assert!(html.contains("<del>old</del> new"), "heading: {html}");
    assert!(html.contains("<del>quoted</del> out"), "blockquote: {html}");
    // The marker must not shift the heading's anchor slug.
    assert!(html.contains(r#"id="old-new""#), "slug drifted: {html}");
}

// ---------------------------------------------------------------------------
// Footnote markers reach the index from every nested container.
//
// These are the tests a top-level-paragraph-only suite cannot write. Before
// `render_blocks_with` existed, each of these rendered a literal `[^1]` in
// the prose while the endnote section still emitted an `<li>` with a
// back-link to an id that was never written.
// ---------------------------------------------------------------------------

/// The renderer turns `[^label]` into a reference only when the document
/// DEFINES that label (`ctx.index.number(label)`), and never inside a code
/// span — both halves render as ordinary text.
///
/// Pinned here because the description stripper in
/// `src-tauri/src/build/page/meta.rs` mirrors exactly this rule. If the body
/// ever starts consuming an undefined token, the page and its own
/// <meta name="description"> would disagree about the same characters — which
/// is the bug that made the stripper gate on definitions in the first place.
#[test]
fn a_bracket_caret_token_is_prose_unless_the_document_defines_it() {
    let undefined = render("Use [^0-9] to strip non-digits.\n");
    assert!(
        undefined.contains("[^0-9]"),
        "prose was consumed as a footnote marker: {undefined}"
    );
    assert!(
        !undefined.contains("moss-footnote"),
        "an undefined label grew an endnote: {undefined}"
    );

    let in_code = render("Write `[^1]` where it belongs[^1].\n\n[^1]: the aside\n");
    assert!(
        in_code.contains("<code>[^1]</code>"),
        "a code span lost its literal marker: {in_code}"
    );
    assert_eq!(
        in_code.matches("moss-footnote-ref").count(),
        1,
        "the marker inside the code span was numbered too: {in_code}"
    );
}

#[test]
fn footnote_ref_inside_blockquote_gets_numbered_marker() {
    let html = render("> Quoted[^1].\n\n[^1]: note\n");
    assert!(!html.contains("[^1]"), "marker rendered literally: {html}");
    assert!(
        html.contains(r##"<sup class="moss-footnote-ref" id="fnref-1" tabindex="-1">"##),
        "got: {html}"
    );
    assert_backrefs_resolve(&html);
}

#[test]
fn footnote_ref_inside_callout_body_gets_numbered_marker() {
    let html = render("> [!note] Title\n> Body[^1].\n\n[^1]: note\n");
    assert!(html.contains(r#"class="callout""#), "not a callout: {html}");
    assert!(!html.contains("[^1]"), "marker rendered literally: {html}");
    assert!(
        html.contains(r##"<sup class="moss-footnote-ref" id="fnref-1" tabindex="-1">"##),
        "got: {html}"
    );
    assert_backrefs_resolve(&html);
}

#[test]
fn footnote_ref_inside_multi_paragraph_list_item_gets_numbered_marker() {
    // A loose list item renders its blocks through the nested-block walk —
    // the branch a tight single-paragraph item never takes.
    let html = render("- first para\n\n  second[^1] para\n\n[^1]: note\n");
    assert!(html.contains("<li>"), "not a list: {html}");
    assert!(!html.contains("[^1]"), "marker rendered literally: {html}");
    assert!(
        html.contains(r##"<sup class="moss-footnote-ref" id="fnref-1" tabindex="-1">"##),
        "got: {html}"
    );
    assert_backrefs_resolve(&html);
}

#[test]
fn footnote_ref_inside_table_cell_and_heading_gets_numbered_marker() {
    let html = render("## Head[^a]\n\n| c |\n| --- |\n| x[^b] |\n\n[^a]: A\n\n[^b]: B\n");
    assert!(!html.contains("[^a]"), "heading marker literal: {html}");
    assert!(!html.contains("[^b]"), "cell marker literal: {html}");
    // The marker is not reading text, so it must not enter the anchor slug.
    assert!(html.contains(r#"id="head""#), "slug drifted: {html}");
    assert_backrefs_resolve(&html);
}

// ---------------------------------------------------------------------------
// Numbering contract
// ---------------------------------------------------------------------------

#[test]
fn numbering_follows_first_reference_not_source_order() {
    let html = render("B[^b] then A[^a].\n\n[^a]: alpha\n\n[^b]: bravo\n");
    let b = html
        .find("bravo")
        .expect("bravo in output");
    let a = html.find("alpha").expect("alpha in output");
    assert!(b < a, "endnotes must be in first-reference order: {html}");
    assert!(html.contains(r##"href="#fn-1" role="doc-noteref">1</a>"##), "{html}");
    assert!(html.contains(r#"<li id="fn-1" tabindex="-1">"#), "{html}");
    assert_backrefs_resolve(&html);
}

#[test]
fn repeated_reference_gets_one_note_and_one_backref_each() {
    let html = render("A[^1] and B[^1].\n\n[^1]: shared\n");
    assert!(html.contains(r#"id="fnref-1""#), "{html}");
    assert!(html.contains(r#"id="fnref-1-2""#), "{html}");
    assert_eq!(html.matches(r#"<li id="fn-1" tabindex="-1">"#).count(), 1, "{html}");
    assert_eq!(
        html.matches(r#"class="moss-footnote-backref""#).count(),
        2,
        "one back-link per marker: {html}"
    );
    assert_backrefs_resolve(&html);
}

/// The back-arrow must carry VARIATION SELECTOR-15 (`&#xFE0E;`) right after
/// U+21A9. Without it, Chrome on Android/iOS picks the emoji presentation and
/// the link renders as a coloured glyph instead of matching the body text.
#[test]
fn backref_arrow_forces_text_presentation() {
    let html = render("A[^1].\n\n[^1]: note\n");
    assert!(html.contains("&#8617;&#xFE0E;</a>"), "{html}");
}

#[test]
fn marker_inside_a_note_body_is_numbered_and_backlinked() {
    // The ordering trap: note 1's body carries a marker, so the back-link
    // lists cannot be written until every body has rendered.
    let html = render("A[^1].\n\n[^1]: see [^2]\n\n[^2]: two\n");
    assert!(html.contains(r#"<li id="fn-2" tabindex="-1">"#), "{html}");
    assert_backrefs_resolve(&html);
}

#[test]
fn definition_written_inside_a_blockquote_is_hoisted_and_numbered() {
    // pulldown nests the definition under the blockquote. Indexing only the
    // top level would leave the marker pointing at an id nobody emits.
    let html = render("> [^1]: nested def\n\nText[^1].\n");
    assert!(
        html.contains(r#"<section class="moss-footnotes""#),
        "definition was not hoisted: {html}"
    );
    assert!(html.contains("nested def"), "note body lost: {html}");
    assert!(
        !html.contains("<blockquote>\n<p>nested def</p>"),
        "the definition still rendered inside the quote: {html}"
    );
    assert_backrefs_resolve(&html);
}

#[test]
fn definition_never_referenced_still_renders_its_text() {
    // The `[^` digraph population: a line that merely STARTS with `[^…]:`
    // (a regex class, a shell glob) parses as a definition. moss numbers it
    // rather than dropping it, so nothing the author wrote disappears.
    let html = render("Use this:\n\n[^abc]: whatever\n");
    assert!(html.contains("whatever"), "author text dropped: {html}");
    assert!(html.contains(r#"<li id="fn-1" tabindex="-1">"#), "{html}");
    assert!(
        !html.contains("moss-footnote-backref"),
        "an unreferenced note has nothing to link back to: {html}"
    );
    assert_backrefs_resolve(&html);
}

#[test]
fn repeated_label_first_definition_wins_endnote_and_repeat_renders_in_place() {
    // FootnoteIndex::definition is explicitly first-wins ("a repeated label
    // is an authoring error; first wins"): the FIRST definition is hoisted
    // into the endnote, and the repeat renders in place in the body — wrong
    // but visible, which beats GFM's silent drop. ADR-035 § render contract.
    let html = render("A[^1].\n\n[^1]: first\n\n[^1]: second\n");
    assert_eq!(html.matches(r#"<li id="fn-1" tabindex="-1">"#).count(), 1, "{html}");
    let (_, after_li) = html
        .split_once(r#"<li id="fn-1" tabindex="-1">"#)
        .expect("endnote li present");
    let (endnote, _) = after_li.split_once("</li>").expect("endnote li closes");
    assert!(
        endnote.contains("first"),
        "the FIRST definition must win the endnote: {html}"
    );
    assert!(
        !endnote.contains("second"),
        "the repeat must not reach the endnote: {html}"
    );
    assert!(html.contains("second"), "the repeat was dropped: {html}");
    assert_backrefs_resolve(&html);
}

#[test]
fn a_repeat_whose_first_definition_is_nested_inside_another_note_still_renders() {
    // The walk-order counter `is_hoisted` used to keep never saw a
    // definition nested inside a hoisted note — the body pass skips the
    // hoisted subtree without descending — so the later top-level repeat
    // passed for the first sighting, was treated as hoisted, and emitted
    // nothing: the author's text reached no surface at all, violating the
    // "wrong but visible" contract two tests up. Identity-decided hoisting
    // does not depend on which pass asks or what it skipped.
    let html =
        render("A[^a] B[^b].\n\n[^a]: OUTERBODY\n\n    > [^b]: INNERB\n\n[^b]: BTOPREPEAT\n");
    assert!(
        html.contains("BTOPREPEAT"),
        "the repeat was silently deleted: {html}"
    );
    // First-reference order: a = fn-1, b = fn-2. The nested FIRST
    // definition wins b's endnote…
    let (_, after_b) = html.split_once(r#"<li id="fn-2" tabindex="-1">"#).expect("fn-2 present");
    let (endnote_b, _) = after_b.split_once("</li>").expect("fn-2 closes");
    assert!(endnote_b.contains("INNERB"), "{html}");
    // …and is NOT also duplicated into its host note's endnote: it is
    // hoisted, so it emits nothing when fn-1's body renders.
    let (_, after_a) = html.split_once(r#"<li id="fn-1" tabindex="-1">"#).expect("fn-1 present");
    let (endnote_a, _) = after_a.split_once("</li>").expect("fn-1 closes");
    assert!(endnote_a.contains("OUTERBODY"), "{html}");
    assert!(
        !endnote_a.contains("INNERB"),
        "the nested definition belongs to its own endnote, not its host's: {html}"
    );
    assert_backrefs_resolve(&html);
}

/// Numbering is FIRST-REFERENCE order — the order the READER meets the
/// markers. A repeat definition's body renders in place, so a marker living
/// only there is met mid-body; `collect_refs` used to skip every definition
/// body (hoisted bodies are re-walked in endnote order, which is right for
/// them), so such a marker was numbered on the never-referenced branch,
/// after everything else, and the printed numbers read 1, 3, 2.
#[test]
fn a_marker_living_only_in_a_repeats_body_is_numbered_where_the_reader_meets_it() {
    let html = render("x[^a]\n\n[^a]: A1\n\n[^a]: A2 [^c]\n\n[^c]: CEE\n\ny[^d]\n\n[^d]: DEE\n");
    let pos = |needle: &str| html.find(needle).unwrap_or_else(|| panic!("{needle}: {html}"));
    assert!(
        pos(r#"id="fnref-1""#) < pos(r#"id="fnref-2""#)
            && pos(r#"id="fnref-2""#) < pos(r#"id="fnref-3""#),
        "printed numbers out of reading order: {html}"
    );
    assert_backrefs_resolve(&html);
}

/// With no body markers at all, the reader meets every marker inside the
/// endnote section, top to bottom. `[^a]` lists first (source order of the
/// never-referenced), and its body references `[^b]` — met before the
/// reader reaches `[^c]`'s note — so b numbers 2 and c numbers 3. The old
/// build appended the never-referenced tail AFTER the body-walking loop
/// ended, so [^a]'s body was never walked and b landed last: 1, 3, 2.
#[test]
fn a_marker_met_inside_an_unreferenced_notes_body_is_numbered_in_reading_order() {
    let html = render("Plain text.\n\n[^a]: see [^b]\n\n[^c]: c text\n\n[^b]: b text\n");
    let (_, after1) = html.split_once(r#"<li id="fn-1" tabindex="-1">"#).expect("fn-1 present");
    let (note1, _) = after1.split_once("</li>").expect("fn-1 closes");
    assert!(
        note1.contains(r##"href="#fn-2""##),
        "the marker met inside note 1 must carry the NEXT number: {html}"
    );
    let (_, after2) = html.split_once(r#"<li id="fn-2" tabindex="-1">"#).expect("fn-2 present");
    let (note2, _) = after2.split_once("</li>").expect("fn-2 closes");
    assert!(note2.contains("b text"), "b must be note 2: {html}");
    assert_backrefs_resolve(&html);
}

#[test]
fn a_document_without_footnotes_gains_no_endnote_section() {
    let html = render("Just prose, and a `[^not-a-note]` in a code span.\n");
    assert!(!html.contains("moss-footnotes"), "{html}");
    assert!(html.contains("[^not-a-note]"), "code span mangled: {html}");
}

// ---------------------------------------------------------------------------
// Task lists render as checkboxes (ADR-035 § Task lists, amended)
// ---------------------------------------------------------------------------

#[test]
fn task_list_renders_checkboxes() {
    let html = render("- [ ] todo\n- [x] done\n");
    assert!(
        html.contains(r#"<input type="checkbox" disabled /> todo"#),
        "unchecked box missing: {html}"
    );
    assert!(
        html.contains(r#"<input type="checkbox" disabled checked /> done"#),
        "checked box missing: {html}"
    );
    assert!(
        !html.contains("[ ]") && !html.contains("[x]"),
        "raw markers leaked into output: {html}"
    );
}

/// The wiring failure this construct is prone to: `ENABLE_TASKLISTS` without a
/// `TaskListMarker` arm keeps the item TEXT and silently deletes the checkbox,
/// so the list looks fine while having lost its meaning. Measured before the
/// fix: `- [ ] todo` rendered `<li>todo</li>`.
#[test]
fn task_marker_is_not_silently_dropped() {
    let html = render("- [ ] todo\n");
    assert!(html.contains("todo"), "item text lost: {html}");
    assert!(
        html.contains("<input"),
        "checkbox deleted while text survived — the marker arm is unwired: {html}"
    );
}

/// A non-task list must not grow checkboxes, and a bracket that is not a task
/// marker stays literal.
#[test]
fn ordinary_lists_and_bare_brackets_are_untouched() {
    let plain = render("- one\n- two\n");
    assert!(!plain.contains("<input"), "plain list grew a checkbox: {plain}");

    let mid = render("- see [x] in the table\n");
    assert!(
        !mid.contains("<input"),
        "a bracket mid-item is not a task marker: {mid}"
    );
}

// ---------------------------------------------------------------------------
// The invariant that generalizes
// ---------------------------------------------------------------------------

/// Marker ids and back-link targets agree EXACTLY: every `href="#fnref-N"`
/// in the endnote section has a matching `id="fnref-N"` in the body, and
/// every marker id is linked back to. Note targets agree one way — every
/// `href="#fn-N"` resolves — because a note nobody referenced legitimately
/// carries an `id="fn-N"` with no incoming link.
///
/// A dangling fragment is invisible in a diff and obvious to a reader who
/// clicks it, which is why this is the one assertion every footnote test
/// above ends with.
fn assert_backrefs_resolve(html: &str) {
    let ids: HashSet<String> = attr_values(html, r#"id=""#).into_iter().collect();
    let hrefs: HashSet<String> = attr_values(html, r##"href="#"##).into_iter().collect();
    let pick = |set: &HashSet<String>, prefix: &str| -> HashSet<String> {
        set.iter()
            .filter(|s| s.starts_with(prefix))
            .cloned()
            .collect()
    };
    assert_eq!(
        pick(&ids, "fnref-"),
        pick(&hrefs, "fnref-"),
        "marker ids and back-links disagree: {html}"
    );
    let note_ids = pick(&ids, "fn-");
    for href in pick(&hrefs, "fn-") {
        assert!(
            note_ids.contains(&href),
            "marker points at #{href}, which nothing emits: {html}"
        );
    }
}

fn attr_values(html: &str, prefix: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find(prefix) {
        rest = &rest[at + prefix.len()..];
        match rest.find('"') {
            Some(end) => {
                out.push(rest[..end].to_string());
                rest = &rest[end..];
            }
            None => break,
        }
    }
    out
}

#[test]
fn every_footnote_anchor_resolves_in_a_document_that_uses_all_of_them() {
    let html = render(concat!(
        "Intro[^a] and again[^a].\n\n",
        "> Quoted[^b].\n\n",
        "- item\n\n  loose[^c] para\n\n",
        "> [!note]\n> Callout[^d].\n\n",
        "| h |\n| --- |\n| cell[^e] |\n\n",
        "[^a]: alpha, see [^f]\n\n",
        "[^b]: bravo\n\n",
        "[^c]: charlie\n\n",
        "[^d]: delta\n\n",
        "[^e]: echo\n\n",
        "[^f]: foxtrot\n",
    ));
    assert_backrefs_resolve(&html);
    for word in [
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot",
    ] {
        assert!(html.contains(word), "{word} lost: {html}");
    }
    assert!(!html.contains("[^"), "a marker rendered literally: {html}");
}

// ---------------------------------------------------------------------------
// Hoisting must not leave residue where the definition used to be
// ---------------------------------------------------------------------------
//
// A definition written inside a blockquote, a list item or a callout is
// nested under that container by pulldown-cmark. The renderer emits the
// container's opening tag, hoists the definition (emitting nothing), then
// emits the closing tag — leaving an empty element on the page. The empty
// `<li>` paints a stray bullet; the empty `<blockquote>` is a spurious
// landmark for assistive tech; the callout keeps a fully chromed box.

#[test]
fn hoisting_a_definition_out_of_a_list_item_removes_the_stray_bullet() {
    let html = render("Ref[^a].\n\n- [^a]: listed note\n");
    assert!(
        !html.contains("<li>\n</li>"),
        "an empty list item survived the hoist — it paints a stray bullet: {html}"
    );
    assert!(!html.contains("<ul>\n</ul>"), "an empty list survived: {html}");
    assert!(html.contains("listed note"), "note body lost: {html}");
    assert_backrefs_resolve(&html);
}

#[test]
fn hoisting_a_definition_out_of_a_blockquote_removes_the_empty_quote() {
    let html = render("Ref[^a].\n\n> [^a]: quoted note\n");
    assert!(
        !html.contains("<blockquote>\n</blockquote>"),
        "an empty blockquote survived the hoist: {html}"
    );
    assert!(html.contains("quoted note"), "note body lost: {html}");
    assert_backrefs_resolve(&html);
}

#[test]
fn hoisting_a_definition_out_of_a_callout_leaves_no_empty_content_box() {
    let html = render("Ref[^a].\n\n> [!note] T\n> [^a]: quoted note\n");
    assert!(
        !html.contains(r#"<div class="callout-content">"#),
        "an empty callout body survived the hoist: {html}"
    );
    assert!(
        html.contains(r#"<div class="callout-title">T</div>"#),
        "the author's callout title must survive — hoisting moves the note, \
         it does not delete the surrounding text: {html}"
    );
    assert!(html.contains("quoted note"), "note body lost: {html}");
    assert_backrefs_resolve(&html);
}

#[test]
fn a_container_that_still_has_content_is_untouched() {
    // The guard on the guard: pruning must only fire when the hoist emptied
    // the container, never when the author wrote something alongside.
    let html = render("> quoted[^1]\n>\n> [^1]: note\n");
    assert!(
        html.contains("<blockquote>"),
        "a blockquote with prose was pruned: {html}"
    );
    assert!(html.contains("quoted"), "blockquote prose lost: {html}");

    let list = render("- item[^2]\n- [^2]: note\n");
    assert!(list.contains("<li"), "a list with a real item was pruned: {list}");
    assert!(list.contains("item"), "list item text lost: {list}");

    // An item the AUTHOR left empty still renders — pruning is about the
    // hoist, not about tidying the author's markup.
    let empty = render("- one\n-\n- two\n");
    assert_eq!(
        empty.matches("<li").count(),
        3,
        "an author-empty list item must keep its bullet: {empty}"
    );
    assert_backrefs_resolve(&html);
}

#[test]
fn a_list_item_holding_only_unicode_whitespace_is_text_not_residue() {
    // U+00A0 / U+3000 / U+2009 are `char::is_whitespace`, so a `.trim()`-based
    // emptiness test reads a CJK full-width space — a character CJK authors type
    // deliberately for indentation — as "the hoist emptied this" and deletes the
    // item. Pruning fires only when the hoist moved content out; text the author
    // typed is never residue.
    for (name, md) in [
        ("nbsp", "- one\n- \u{a0}\n- two\n"),
        ("ideographic space", "- one\n- \u{3000}\n- two\n"),
        ("thin space", "- one\n- \u{2009}\n- two\n"),
    ] {
        let html = render(md);
        assert_eq!(
            html.matches("<li").count(),
            3,
            "{name}: a whitespace-only list item was deleted: {html:?}"
        );
    }

    // The whole document must not vanish when that item is the only one, and
    // the surrounding containers must survive with it.
    let solo = render("- \u{a0}\n");
    assert!(
        solo.contains("<li"),
        "a lone whitespace-only item emptied the document: {solo:?}"
    );

    let nested = render("- one\n  - \u{a0}\n- two\n");
    assert_eq!(
        nested.matches("<ul").count(),
        2,
        "the nested list around a whitespace-only item disappeared: {nested:?}"
    );

    let quoted = render("> - \u{a0}\n");
    assert!(
        quoted.contains("<blockquote>"),
        "the blockquote around a whitespace-only item disappeared: {quoted:?}"
    );

    let callout = render("> [!note] Title\n> - \u{a0}\n");
    assert!(
        callout.contains(r#"<div class="callout-content">"#),
        "the callout body around a whitespace-only item disappeared: {callout:?}"
    );

    // Ordered lists renumber silently when an item is dropped, so pin the count.
    let ordered = render("3. one\n4. \u{a0}\n5. two\n");
    assert_eq!(
        ordered.matches("<li").count(),
        3,
        "an ordered list silently renumbered around a deleted item: {ordered:?}"
    );
}

// ---------------------------------------------------------------------------
// Heading ids follow RENDER order, not source order
// ---------------------------------------------------------------------------
//
// The renderer hoists the first definition of every label to the endnote
// section at the END of the page, so a heading inside a definition written
// ABOVE a same-titled body heading renders BELOW it. Numbering the ids in
// source order handed the bare slug to the endnote copy, and `[[Note#Notes]]`
// — slugged from heading text with no counter — scrolled the reader into the
// endnote list instead of the section.

fn body_and_endnote(html: &str) -> (String, String) {
    let at = html
        .find(r#"<section class="moss-footnotes""#)
        .expect("an endnote section");
    (html[..at].to_string(), html[at..].to_string())
}

#[test]
fn a_definition_written_first_does_not_steal_the_body_headings_slug() {
    for md in [
        // definition first, heading on the definition line
        "[^a]: ## Notes\n\nRef[^a].\n\n## Notes\n",
        // definition first, indented heading in the note body
        "[^a]: body\n\n    ## Notes\n\nRef[^a].\n\n## Notes\n",
        // definition mid-document, right after the paragraph that cites it
        "Intro[^a].\n\n[^a]: see\n\n    ## Notes\n\n## Notes\n\nbody text\n",
    ] {
        let html = render(md);
        let (body, endnote) = body_and_endnote(&html);
        assert!(
            body.contains(r#"id="notes""#),
            "the VISIBLE body heading must keep the bare slug: {md:?} → {html}"
        );
        assert!(
            endnote.contains(r#"id="notes-1""#),
            "the hoisted endnote heading must take the suffix: {md:?} → {html}"
        );
        assert_eq!(
            html.matches(r#"id="notes""#).count(),
            1,
            "duplicate DOM id: {md:?} → {html}"
        );
        assert_backrefs_resolve(&html);
    }
}

#[test]
fn a_definition_written_last_still_numbers_after_the_body() {
    // The ordering that already worked stays working.
    let html = render("## Notes\n\nX[^h].\n\n[^h]: body\n\n    ## Notes\n");
    let (body, endnote) = body_and_endnote(&html);
    assert!(body.contains(r#"id="notes""#), "{html}");
    assert!(endnote.contains(r#"id="notes-1""#), "{html}");
    assert_backrefs_resolve(&html);
}

#[test]
fn two_hoisted_headings_are_numbered_in_endnote_order() {
    // Endnote order is FIRST-REFERENCE order, a third ordering that agrees
    // with neither source nor body position. The ids must follow it.
    let html = render("[^z]: ## Notes\n\n[^a]: ## Notes\n\nRef[^a] then[^z].\n");
    let (_, endnote) = body_and_endnote(&html);
    let first = endnote.find(r#"id="notes""#).expect("bare slug in endnotes");
    let second = endnote
        .find(r#"id="notes-1""#)
        .expect("suffixed slug in endnotes");
    assert!(
        first < second,
        "the endnote rendered first must hold the bare slug: {endnote}"
    );
    assert_backrefs_resolve(&html);
}

// ---------------------------------------------------------------------------
// A definition inside a shortcode body is NOT hoisted, so its headings are
// body headings
// ---------------------------------------------------------------------------
//
// Grid cells, the hero overlay and compound-link cards render through the
// context-free `render_blocks` entry point with a fresh `FootnoteCtx`
// (ADR-035), and `FootnoteIndex`'s collector stops at shortcode bodies — so a
// definition written in a cell is never hoisted to the endnotes. It renders
// IN PLACE, first in the DOM. Bucketing its headings as "hoisted" numbered
// them after the entire body, handing the bare slug to a copy that renders
// last and the suffix to the one the reader sees first — the exact inversion
// the render-order numbering above exists to prevent.

#[test]
fn a_definition_inside_a_grid_cell_is_a_body_heading_not_an_endnote_one() {
    for (name, md) in [
        (
            "grid cell, same-titled body heading",
            ":::grid 2\n[^a]: ## Notes\n\ntext\n+++\nb\n:::\n\n## Notes\n\nbody\n",
        ),
        (
            "hero overlay, same-titled body heading",
            ":::hero\ncover.jpg\n---\n[^a]: ## Notes\n:::\n\n## Notes\n\nbody\n",
        ),
    ] {
        let html = render_with_urls(md);
        let first = html
            .find(r#"id="notes""#)
            .unwrap_or_else(|| panic!("{name}: no bare slug at all: {html}"));
        let second = html
            .find(r#"id="notes-1""#)
            .unwrap_or_else(|| panic!("{name}: no suffixed slug at all: {html}"));
        assert!(
            first < second,
            "{name}: the heading that renders FIRST must hold the bare slug — \
             the cell's copy is in-place and comes first in the DOM: {html}"
        );
        assert_eq!(
            html.matches(r#"id="notes""#).count(),
            1,
            "{name}: duplicate DOM id: {html}"
        );
    }
}

#[test]
fn a_cell_definition_does_not_hoist_the_document_definition_of_the_same_label() {
    // The `hoisted` set is document scope. Letting a cell-scoped definition
    // claim the label pushed the REAL top-level definition into the body
    // bucket, so the endnote copy — which renders last — took the bare slug.
    let html = render(":::grid 2\n[^a]: ## Alpha\n+++\nb\n:::\n\nRef[^a]\n\n[^a]: ## Alpha\n");
    let (body, endnote) = body_and_endnote(&html);
    assert!(
        body.contains(r#"id="alpha""#),
        "the visible grid card renders first and must keep the bare slug: {html}"
    );
    assert!(
        endnote.contains(r#"id="alpha-1""#),
        "the hoisted endnote copy must take the suffix: {html}"
    );
    assert_eq!(
        html.matches(r#"id="alpha""#).count(),
        1,
        "duplicate DOM id: {html}"
    );
    assert_backrefs_resolve(&html);
}
