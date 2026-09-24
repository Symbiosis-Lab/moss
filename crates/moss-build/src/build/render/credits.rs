//! Credit rows: the byline at the head of a page and the colophon at its foot.
//!
//! **On every page kind.** Frontmatter that exists only to be displayed is
//! displayed; everything else is metadata, and the body is the author's.
//! `byline:` and `colophon:` exist only to be displayed, so a folder index gets
//! them exactly as an article does — the motivating case is a folder whose
//! speaker credits are a page-level byline. The mirror half of the same rule:
//! `description:` is metadata everywhere and prints nowhere, because it has no
//! stable provenance (sometimes authored, sometimes derived from the body by
//! `build/page/meta.rs`) and a reader cannot tell which.
//!
//! # Where the credits go, and why it is always somewhere
//!
//! The byline sits under the page's title and the colophon at its foot. Three
//! kinds of page answer "what title?" differently:
//!
//! - **An article** has a title block moss knows the shape of (H1, optional
//!   blockquote deck, date row). The byline goes after it.
//! - **A folder index** has an H1 moss injects itself, either in the cover row
//!   or above the body. The byline goes directly after that H1, the colophon
//!   after the children listing.
//! - **Everything else** — the root homepage, a `home: true` or language-root
//!   folder page, a plain page (`layout: page`, a `nav:` page, a root-level
//!   `about.md`) — gets no H1 from moss at all. Whatever title it has, the
//!   author wrote in the body.
//!
//! For that third kind [`splice_byline_at_page_head`] reads the body: if it
//! opens with an `<h1>`, the byline goes under it, which is the same place a
//! reader finds it on the other two kinds. If it does not, the byline goes at
//! the very top of the page content — under a hero if the page has one, since
//! `hero_html` is a separate template variable rendered outside `<main>`, so
//! "top of the content" already means "below the hero" without asking.
//!
//! Emitting nothing here was the alternative, and it is the wrong one: an
//! author who writes `byline:` has written it to be read, and a page that
//! silently drops it is the exception this module exists to remove. There is
//! no sub-case where the answer is nothing — a page always has a top, and the
//! top of a page with no title is a defensible place for a credit line.
//!
//! Only a *leading* `<h1>` counts. A page whose body opens with prose and
//! carries an `<h1>` in the middle prepends instead, because splicing there
//! would drop the byline into the middle of the piece.
//!
//! # Three emission sites, exactly one of which fires
//!
//! `render/html.rs` assembles a page in three places, and one boolean decides
//! between them:
//!
//! - `is_article_page` → the article path, and only it.
//! - otherwise, in the non-homepage arm: a folder index with its own title
//!   block takes the folder path; its `else` — a home-override folder or a
//!   plain page — takes the page-head path.
//! - otherwise, in the homepage arm (a different `match` arm, so mutually
//!   exclusive with both of the above): the page-head path.
//!
//! The `is_article_page` gate is load-bearing rather than decorative: a
//! `layout: article` folder index is assembled by BOTH the folder branch and
//! the article branch — the folder branch for its title, cover and children
//! listing, the article branch for its date row and its credits — so an
//! ungated folder-branch emission printed each block twice. The same is true
//! of a `layout: article` homepage.
//!
//! `byline:` and `colophon:` are **display strings**, not structured data —
//! one row per list entry, or per line of a YAML block scalar:
//!
//! ```yaml
//! byline: |
//!   作者　糜緒洋
//!   編輯　謝丁
//! colophon: |
//!   首發媒體　[端傳媒](https://…)、[單讀](https://…)
//!   封面　基輔米迦勒修道院門口的陣亡將士紀念牆（拍攝：糜緒洋）
//! ```
//!
//! Two fields, one renderer, because they differ only in where they land. The
//! split itself is what the reference publications agree on: two or three
//! names before the piece, everything else — first publisher, contributor
//! biographies, production credits — after it.
//!
//! Three decisions are worth stating, because each costs something:
//!
//! 1. **Rows render as inline markdown.** Real credits carry links — the
//!    first-publication row above is the common case, not an exotic one — so
//!    escaping the row as plain text would break the field's main use. Rows go
//!    through [`crate::build::markdown::render_markdown_to_html`], the one
//!    markdown path in moss, and the single `<p>` it returns is unwrapped. A
//!    row that renders as anything other than one paragraph (a stray `#`, a
//!    list dash) is escaped as literal text instead, so a typo shows up as
//!    itself rather than as a heading inside the masthead. Sharing that path
//!    also means a row behaves exactly like body text: external links get the
//!    site's new-tab treatment, and inline HTML passes through as it does in
//!    the body (the file is the author's own — this is not a trust boundary).
//! 2. **moss cannot style the role apart from the name.** With a free-form
//!    string there is nothing to tell moss that `作者` is a label and
//!    `糜緒洋` is a person, so the whole row is one muted line. A site that
//!    wants the label tracked-out (the CJK stand-in for small caps) writes
//!    that in its own theme CSS, or writes the label in markdown emphasis.
//! 3. **The row separator is whatever the author typed.** A full-width space
//!    (`作者　糜緒洋`) is preserved verbatim — rows are trimmed at the ends
//!    only, never re-spaced in the middle.

/// Render the byline block for an article, or `None` when there is nothing
/// to show — the authored `byline:` rows, the automatic place line, or both.
///
/// `emit_source_fm` mirrors the date row: when the preview is running for the
/// editor, the block carries `data-source-fm="byline"` (or `"location"` on
/// the place line's own `<div>`) so a click maps back to the frontmatter
/// field. Stripped from shipped HTML by `build::ship`.
///
/// The two pieces are resolved independently and only `None` when BOTH are —
/// a page with `location:` set but no `byline:` (the common case) must still
/// show its place line, which keying this function's result on `rows` alone
/// would silently drop.
pub fn render_byline_html(rows: &[String], emit_source_fm: bool, place_line: Option<&str>) -> Option<String> {
    let byline = render_rows("moss-byline", "moss-byline-row", "byline", rows, emit_source_fm);
    let place = render_place_line_html(place_line, emit_source_fm);
    match (byline, place) {
        (None, None) => None,
        (Some(b), None) => Some(b),
        (None, Some(p)) => Some(p),
        (Some(b), Some(p)) => Some(format!("{}{}", b, p)),
    }
}

/// The automatic place line's own block, or `None` when there is none. Not
/// row-split like `render_rows` — the line is inherently one row, so
/// `moss-place-line` is the only class the contract table needs.
fn render_place_line_html(place_line: Option<&str>, emit_source_fm: bool) -> Option<String> {
    let line = place_line?;
    let mut out = String::from(r#"<div class="moss-place-line""#);
    if emit_source_fm {
        out.push_str(r#" data-source-fm="location""#);
    }
    out.push('>');
    out.push_str(&render_row(line));
    out.push_str("</div>");
    Some(out)
}

/// Append the colophon to the end of a page's content — the one placement it
/// has on every page kind that is not an article, after the children listing
/// when there is one. A no-op when the field is absent.
pub fn push_colophon(content: &mut String, rows: &[String], emit_source_fm: bool) {
    if let Some(colophon) = render_colophon_html(rows, emit_source_fm) {
        content.push_str(&colophon);
    }
}

/// Render the foot colophon for an article, or `None` when there is nothing
/// to show. Same rows, same markdown, different place on the page.
pub fn render_colophon_html(rows: &[String], emit_source_fm: bool) -> Option<String> {
    render_rows(
        "moss-article-colophon",
        "moss-article-colophon-row",
        "colophon",
        rows,
        emit_source_fm,
    )
}

/// Put the byline at the head of a page moss gives no title of its own, and
/// return the page content with it in place.
///
/// Under the author's own opening `<h1>` when the body starts with one, at the
/// very top of the content when it does not. See the module docs for why the
/// second case renders rather than skips. Returns the content untouched when
/// there is no byline to place.
pub fn splice_byline_at_page_head(
    content: String,
    rows: &[String],
    emit_source_fm: bool,
    place_line: Option<&str>,
) -> String {
    let Some(byline) = render_byline_html(rows, emit_source_fm, place_line) else {
        return content;
    };
    // Same splice the article path uses, so the byline lands in the same
    // relation to the title on every page kind — including a claimed leaf's
    // cover, which wraps that title behind a recognized prefix
    // `splice_after_title_block` already knows to step past; a page with no
    // title block at all still falls through to its own prepend.
    crate::build::markdown::html_post::splice_after_title_block(&content, &byline)
}

/// The shared body of both renderers.
fn render_rows(
    block_class: &str,
    row_class: &str,
    fm_field: &str,
    rows: &[String],
    emit_source_fm: bool,
) -> Option<String> {
    if rows.is_empty() {
        return None;
    }
    let mut out = format!(r#"<div class="{}""#, block_class);
    if emit_source_fm {
        out.push_str(&format!(r#" data-source-fm="{}""#, fm_field));
    }
    out.push('>');
    for row in rows {
        out.push_str(&format!(r#"<div class="{}">"#, row_class));
        out.push_str(&render_row(row));
        out.push_str("</div>");
    }
    out.push_str("</div>");
    Some(out)
}

/// Render one credit row as INLINE markdown.
///
/// Reuses the fragment renderer and unwraps its single paragraph rather than
/// introducing a second markdown path — moss has exactly one, and a credit row
/// is not a good enough reason for a second. When the row does not render as
/// one plain paragraph, its literal text is escaped instead.
///
/// Public under the name `render_display_row` for the one other place moss
/// shows a short authored line that may carry a link: a hero's
/// `caption="…"`. A cover credit and a colophon credit are the same kind of
/// text and must not diverge in what markdown they accept.
pub(crate) fn render_display_row(row: &str) -> String {
    render_row(row)
}

fn render_row(row: &str) -> String {
    let html = crate::build::markdown::render_markdown_to_html(row);
    let trimmed = html.trim();
    match trimmed
        .strip_prefix("<p>")
        .and_then(|rest| rest.strip_suffix("</p>"))
    {
        // One paragraph and nothing else — `</p>` may only appear at the end,
        // or the row produced more than one block and the unwrap would splice
        // unbalanced markup into the page.
        Some(inner) if !inner.contains("</p>") => inner.to_string(),
        _ => crate::build::features::html_escape(row),
    }
}

#[cfg(test)]
#[path = "credits_tests.rs"]
mod tests;
