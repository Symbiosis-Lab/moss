//! Inline `#tag` extraction (Obsidian dialect) — issue #649 P1.
//!
//! Obsidian treats `#tag` in body text as a tag; moss historically read
//! only frontmatter `tags:`, so a typical Obsidian note silently lost
//! every inline tag on publish. This module walks the typed AST after
//! parse and collects those tags as **metadata only** — the tag text
//! stays visible as plain text in the rendered body. That is deliberate:
//! Obsidian renders a tag as a link into its own search UI, but a
//! published moss site has no tag page to link to, so plain text is the
//! correct published rendering today. No AST variant, no HTML change.
//!
//! Tag grammar (Obsidian's, exactly):
//!
//! - `#` must sit at a paragraph/line start or be preceded by whitespace
//!   **in the source** — this excludes URL fragments (`page#section`),
//!   mid-word `a#b`, and a `#` directly after a closing delimiter or
//!   inline construct (`*emph*#x`, `` `code`#x ``, `[link](u)#x`). The
//!   walk threads that context through sibling inlines, and re-joins
//!   adjacent text runs before scanning, so pulldown's delimiter-run
//!   splits (`#_draft` → `#`,`_`,`draft`) cannot hide a tag.
//! - Tag characters are everything EXCEPT Obsidian's exclusion set:
//!   whitespace, U+2000–U+206F (General Punctuation), U+2E00–U+2E7F
//!   (Supplemental Punctuation), and the ASCII punctuation
//!   `'!"#$%&()*+,.:;<=>?@^`{|}~[]\`. CJK and emoji are tag characters;
//!   `/` nests (`#a/b` is one tag `a/b`); `-` and `_` are allowed.
//! - Purely-numeric tags are rejected (`#123` is not a tag; `#1a` is).
//! - The leading `#` is stripped from the stored tag.
//!
//! The walk covers every construct whose text the renderer publishes:
//! Figure captions, callouts (title line included), lists, tables,
//! blockquotes, footnote definitions, link cards, `:::grid` cells and
//! `:::hero` overlays. It scans only [`Inline::Text`] — `Block::CodeBlock`
//! stores its body as a `String` and `Inline::Code` is skipped, so code
//! never yields tags.
//!
//! Known accepted limitations:
//!
//! - pulldown-cmark erases the backslash of an escaped `\#tag` before the
//!   typed AST exists, so an escaped tag is indistinguishable from a real
//!   one here and is extracted too. Fixing that would require pre-parse
//!   source inspection; not worth it for an escape Obsidian itself doesn't
//!   honor in this position.
//! - A callout TITLE is a `String`, not typed inlines, so the two
//!   sibling-context rules the inline walk enforces do not apply inside it:
//!   the parser folds `$…$` math into the title as raw source (a `#` after
//!   whitespace inside title math is extracted), and a `#` directly after
//!   inline markup (`> [!note] *x*#y`) reads as text-adjacent rather than
//!   delimiter-adjacent. Same class as the escaped-`\#tag` limit above.
//! - `Shortcode::Recent`'s fallback body is stored as unparsed markdown
//!   (`fallback_markdown: String`), rendered only when the query matches
//!   zero posts. Tags there are NOT extracted — walking them would mean
//!   re-parsing the string with the caller's `ParseConfig`, which this
//!   module does not have. A tag that only exists in a zero-match fallback
//!   is not indexed.

use std::collections::HashSet;

use super::document::Document;
use super::node::{Block, Inline};
use super::shortcode::Shortcode;

/// Collect every inline `#tag` in `doc`, in stable document order,
/// deduplicated case-insensitively (first-seen form wins), leading `#`
/// stripped.
pub fn extract_inline_tags(doc: &Document) -> Vec<String> {
    let mut raw = Vec::new();
    for block in &doc.blocks {
        in_block(block, &mut raw);
    }
    let mut seen = HashSet::new();
    let mut tags = Vec::new();
    for tag in raw {
        if seen.insert(tag.to_lowercase()) {
            tags.push(tag);
        }
    }
    tags
}

fn in_block(block: &Block, out: &mut Vec<String>) {
    match block {
        Block::Heading { children, .. } | Block::Paragraph(children) => in_inlines(children, out),
        Block::Figure { caption, .. } => {
            if let Some(caption) = caption {
                in_inlines(caption, out);
            }
        }
        Block::Callout {
            title, children, ..
        } => {
            // The marker line's inline text is lifted into `title: String`
            // by the parser and never reaches `in_inlines` — yet the
            // renderer emits it as visible text in `<div class="callout-title">`,
            // so a tag there is on the published page. Scanned FIRST so
            // document order is stable. `at_boundary = true`: the title
            // starts right after the `[!kind]` marker and its separating
            // space, i.e. at a whitespace boundary in the source.
            if let Some(title) = title {
                scan_text(title, true, out);
            }
            for nested in children {
                in_block(nested, out);
            }
        }
        Block::BlockQuote(children)
        | Block::LinkCard { children, .. }
        | Block::FootnoteDefinition { children, .. } => {
            for nested in children {
                in_block(nested, out);
            }
        }
        Block::List { items, .. } => {
            for item in items {
                for nested in item {
                    in_block(nested, out);
                }
            }
        }
        Block::Table { header, rows, .. } => {
            // line_breaks skips `header` (a soft break can't appear there);
            // a tag can, so header cells are walked too.
            for cell in header {
                in_inlines(cell, out);
            }
            for row in rows {
                for cell in row {
                    in_inlines(cell, out);
                }
            }
        }
        // `:::grid` cells and a `:::hero` overlay are typed `Vec<Block>`
        // (PR4.5) that the renderer publishes as ordinary prose, so their
        // tags are as real as any other. Mirrors `visit.rs::visit_block`.
        Block::Shortcode(Shortcode::Grid(args)) => {
            for cell in &args.cells {
                for nested in cell {
                    in_block(nested, out);
                }
            }
        }
        Block::Shortcode(Shortcode::Hero(args)) => {
            for nested in &args.overlay {
                in_block(nested, out);
            }
        }
        // Code is the author's literal text; Subscribe/Buttons/Gallery/Apply
        // carry no prose body; `Recent`'s fallback is unparsed markdown (see
        // the module doc); Other is raw passthrough HTML.
        Block::CodeBlock { .. }
        | Block::Shortcode(
            Shortcode::Subscribe(_)
            | Shortcode::Buttons(_)
            | Shortcode::Gallery(_)
            | Shortcode::Recent(_)
            | Shortcode::Apply(_),
        )
        | Block::ThematicBreak
        | Block::Other(_) => {}
    }
}

fn in_inlines(inlines: &[Inline], out: &mut Vec<String>) {
    in_inlines_at(inlines, true, out);
}

/// Walk one sibling run. `at_boundary` says whether a `#` opening the next
/// text would be tag-openable — true at a paragraph/construct start and
/// after whitespace or a line break, false right after a non-text inline
/// (whose closing delimiter is the preceding source character). Threading
/// it through siblings keeps the scan faithful to how the SOURCE reads,
/// which pulldown's inline events no longer show directly.
fn in_inlines_at(inlines: &[Inline], mut at_boundary: bool, out: &mut Vec<String>) {
    let mut i = 0;
    while i < inlines.len() {
        match &inlines[i] {
            Inline::Text(_) => {
                // Re-join the maximal run of ADJACENT text siblings before
                // scanning: pulldown splits a paragraph at unpaired emphasis
                // delimiters (`#_draft` → `#`,`_`,`draft note`), and the tag
                // only exists in the joined text. Soft breaks are
                // `Inline::Text("\n")`, so line seams join in as ordinary
                // whitespace.
                let mut buf = String::new();
                while let Some(Inline::Text(s)) = inlines.get(i) {
                    buf.push_str(s);
                    i += 1;
                }
                scan_text(&buf, at_boundary, out);
                if let Some(last) = buf.chars().last() {
                    at_boundary = last.is_whitespace();
                }
                continue;
            }
            Inline::Emphasis(children)
            | Inline::Strong(children)
            | Inline::Strikethrough(children)
            | Inline::Link { children, .. } => {
                // The first child text sits right after an opening delimiter
                // (`*`, `[`) in source, and the next sibling right after the
                // closing one — neither seam is a boundary.
                in_inlines_at(children, false, out);
                at_boundary = false;
            }
            // A hard break is whitespace in source; a task marker's `[ ]` is
            // always followed by whitespace (`- [ ] text`). Both leave the
            // next text tag-openable.
            Inline::LineBreak | Inline::TaskMarker(_) => at_boundary = true,
            // Code spans, images, footnote refs and raw HTML all end in a
            // non-whitespace source character (`` ` ``, `)`, `]`, `>`).
            Inline::Image { .. } | Inline::Code(_) | Inline::FootnoteRef(_) | Inline::Other(_) => {
                at_boundary = false;
            }
        }
        i += 1;
    }
}

/// True for characters Obsidian allows inside a tag (see module doc for
/// the exclusion set this negates).
fn is_tag_char(c: char) -> bool {
    !(c.is_whitespace()
        || ('\u{2000}'..='\u{206F}').contains(&c)
        || ('\u{2E00}'..='\u{2E7F}').contains(&c)
        || matches!(
            c,
            '\'' | '!'
                | '"'
                | '#'
                | '$'
                | '%'
                | '&'
                | '('
                | ')'
                | '*'
                | '+'
                | ','
                | '.'
                | ':'
                | ';'
                | '<'
                | '='
                | '>'
                | '?'
                | '@'
                | '^'
                | '`'
                | '{'
                | '|'
                | '}'
                | '~'
                | '['
                | ']'
                | '\\'
        ))
}

/// Scan one re-joined text run for tags. Char-iteration only — no byte
/// slicing. `at_boundary` seeds the look-behind for the run's first char:
/// `false` plants a non-whitespace sentinel standing in for the delimiter
/// (`*`, `` ` ``, `)`…) that closed the preceding sibling in source.
fn scan_text(text: &str, at_boundary: bool, out: &mut Vec<String>) {
    /// Stand-in for a preceding non-whitespace source character the AST no
    /// longer carries. Any non-whitespace char works; U+FFFC is never prose.
    const SENTINEL: char = '\u{FFFC}';
    let mut prev: Option<char> = if at_boundary { None } else { Some(SENTINEL) };
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        // MSRV 1.80: `Option::is_none_or` needs 1.82, hence `map_or`.
        if c == '#' && prev.map_or(true, char::is_whitespace) {
            let mut tag = String::new();
            let mut last = c;
            while let Some(&next) = chars.peek() {
                if !is_tag_char(next) {
                    break;
                }
                tag.push(next);
                last = next;
                chars.next();
            }
            if !tag.is_empty() && !tag.chars().all(|ch| ch.is_ascii_digit()) {
                out.push(tag);
            }
            prev = Some(last);
        } else {
            prev = Some(c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::parser::parse;
    use super::*;

    fn tags(md: &str) -> Vec<String> {
        extract_inline_tags(&parse(md))
    }

    #[test]
    fn cjk_tag_is_extracted() {
        assert_eq!(tags("今天做了 #食谱 一份\n"), vec!["食谱"]);
    }

    #[test]
    fn emoji_tag_is_extracted() {
        assert_eq!(tags("so hot #🔥takes today\n"), vec!["🔥takes"]);
    }

    #[test]
    fn nested_tag_is_one_tag() {
        assert_eq!(
            tags("filed under #inbox/to-read now\n"),
            vec!["inbox/to-read"]
        );
    }

    #[test]
    fn purely_numeric_tag_is_rejected() {
        assert_eq!(tags("see #123 there\n"), Vec::<String>::new());
    }

    #[test]
    fn numeric_with_letter_is_a_tag() {
        assert_eq!(tags("see #1a there\n"), vec!["1a"]);
    }

    #[test]
    fn hash_mid_word_is_not_a_tag() {
        assert_eq!(
            tags("see a#b and https://x.dev/page#section\n"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn tag_inside_inline_code_is_not_extracted() {
        assert_eq!(
            tags("run `git show #recipes` verbatim\n"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn tag_inside_fenced_code_block_is_not_extracted() {
        assert_eq!(tags("```\n#recipes\n```\n"), Vec::<String>::new());
    }

    #[test]
    fn tag_at_start_of_line_is_extracted() {
        // `#recipes` with no space after `#` is NOT an ATX heading, so it
        // stays paragraph text — and the run boundary counts as whitespace.
        assert_eq!(tags("#recipes\n"), vec!["recipes"]);
    }

    #[test]
    fn multiple_tags_on_one_line() {
        assert_eq!(
            tags("both #cooking and #recipes apply\n"),
            vec!["cooking", "recipes"]
        );
    }

    #[test]
    fn dedup_is_case_insensitive_preserving_first_form() {
        assert_eq!(
            tags("first #Recipes then #recipes again\n"),
            vec!["Recipes"]
        );
    }

    #[test]
    fn tag_inside_list_item_is_extracted() {
        assert_eq!(tags("- groceries #errands\n"), vec!["errands"]);
    }

    #[test]
    fn tag_inside_blockquote_is_extracted() {
        assert_eq!(tags("> quoted #wisdom here\n"), vec!["wisdom"]);
    }

    #[test]
    fn double_hash_is_not_a_tag() {
        // The second `#` is an excluded character, so `##tag` never opens
        // a tag (matches Obsidian).
        assert_eq!(tags("see ##tag here\n"), Vec::<String>::new());
    }

    #[test]
    fn bare_hash_yields_nothing() {
        assert_eq!(tags("just a # alone\n"), Vec::<String>::new());
    }

    #[test]
    fn tag_terminates_at_caret() {
        // `^` is in Obsidian's exclusion set (reserved for block IDs:
        // `^block-id`), so it ends the tag rather than joining it.
        assert_eq!(tags("see #foo^bar here\n"), vec!["foo"]);
    }

    #[test]
    fn tag_split_across_delimiter_runs_is_recovered() {
        // pulldown splits the paragraph at the unpaired `_` delimiter into
        // Text("#"), Text("_"), Text("draft note") — the tag only exists in
        // the re-joined text. `_` is an accepted tag character (see
        // `tag_with_underscore` behavior via is_tag_char), so `#_draft` is a
        // tag Obsidian extracts.
        assert_eq!(tags("#_draft note\n"), vec!["_draft"]);
        assert_eq!(tags("tagging #_hidden now\n"), vec!["_hidden"]);
    }

    #[test]
    fn hash_after_inline_code_is_not_a_tag() {
        // In source the char before `#` is the closing backtick — not
        // whitespace, so Obsidian does not open a tag here.
        assert_eq!(tags("run `code`#notag now\n"), Vec::<String>::new());
    }

    #[test]
    fn hash_after_emphasis_is_not_a_tag() {
        // The char before `#` is the closing `*` delimiter.
        assert_eq!(tags("*emphasis*#notag\n"), Vec::<String>::new());
        assert_eq!(tags("**bold**#notag\n"), Vec::<String>::new());
    }

    #[test]
    fn hash_after_link_is_not_a_tag() {
        // The char before `#` is `)` — and the module doc promises
        // URL-adjacent `#` never matches.
        assert_eq!(tags("see [x](https://a.dev)#notag now\n"), Vec::<String>::new());
    }

    #[test]
    fn tag_after_explicit_line_break_is_extracted() {
        // A hard break (two trailing spaces) is whitespace in the source,
        // so a `#` opening the next line is a tag.
        assert_eq!(tags("line one  \n#tag here\n"), vec!["tag"]);
    }

    // -- Shortcode bodies: prose moss publishes, so tags moss must index --

    #[test]
    fn tag_inside_grid_cell_is_extracted() {
        assert_eq!(tags(":::grid\nRecipe #cooking\n:::\n"), vec!["cooking"]);
    }

    #[test]
    fn tag_in_every_grid_cell_is_extracted() {
        assert_eq!(
            tags(":::grid 2\nRecipe #cooking\n+++\nOther #baking\n:::\n"),
            vec!["cooking", "baking"]
        );
    }

    #[test]
    fn tag_inside_hero_overlay_is_extracted() {
        // Both hero shapes: the `image=` attribute path and the
        // leading-body-media-line path run through different `parse_hero`
        // branches, and both keep the overlay as typed blocks.
        assert_eq!(tags(":::hero\nRecipe #cooking\n:::\n"), vec!["cooking"]);
        assert_eq!(
            tags(":::hero {image=a.jpg}\nRecipe #cooking\n:::\n"),
            vec!["cooking"]
        );
    }

    #[test]
    fn tag_inside_grid_link_card_is_extracted() {
        // A compound-link cell is a `Block::LinkCard` nested in the grid.
        assert_eq!(
            tags(":::grid\n[### Notes #cooking\n\ntext](/a)\n:::\n"),
            vec!["cooking"]
        );
    }

    #[test]
    fn in_grid_tag_dedups_against_the_same_top_level_tag() {
        assert_eq!(
            tags("intro #Cooking\n\n:::grid\nRecipe #cooking\n:::\n"),
            vec!["Cooking"]
        );
    }

    // -- Callout titles: visible published text, so indexable text --

    #[test]
    fn tag_in_callout_title_is_extracted() {
        assert_eq!(tags("> [!note] Recipe #cooking\n"), vec!["cooking"]);
        assert_eq!(tags("> [!tip]+ Recipe #cooking\n> body\n"), vec!["cooking"]);
    }

    #[test]
    fn callout_title_tag_precedes_its_body_tag_in_document_order() {
        assert_eq!(
            tags("> [!note] Recipe #cooking\n> also #recipes\n"),
            vec!["cooking", "recipes"]
        );
    }

    #[test]
    fn callout_title_and_body_tags_dedup_case_insensitively() {
        assert_eq!(
            tags("> [!note] Recipe #cooking\n> again #Cooking\n"),
            vec!["cooking"]
        );
    }

    #[test]
    fn hash_mid_word_in_a_callout_title_is_not_a_tag() {
        // The title is scanned with the same grammar as body text: `#` needs
        // a whitespace look-behind, and the title's own start counts as one.
        assert_eq!(
            tags("> [!note] see a#b and page#section\n"),
            Vec::<String>::new()
        );
        assert_eq!(tags("> [!note] #cooking tonight\n"), vec!["cooking"]);
    }
}
