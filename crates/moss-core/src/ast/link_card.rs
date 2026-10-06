//! Pure-Rust rendering for a grid cell whose whole content is one link
//! (`Block::LinkCard`) that turns out to leave the site.
//!
//! `render::render_block`'s `Block::LinkCard` arm calls
//! [`render_external_link_card`] for the external case. This crate has no
//! I/O, so it can never fetch a title or a favicon — the sibling, richer
//! renderer for that is `crate::build::render::grid_cells::render_external_card`
//! in moss-build, which overrides this output for any page that goes
//! through the ordinary body-plan pipeline (cache reads, the full cover
//! pipeline for an authored image). What lives here is the FLOOR: correct,
//! author-content-first output for a document rendered outside that
//! pipeline (see `resolve_page_body`'s doc comment on a plan-less page).
//!
//! Both renderers agree on one thing per the owner's "one card kind"
//! decision: an external link becomes a `.moss-card`, the same shell an
//! internal page card uses, never the retired `.moss-grid-card.link-preview`
//! shape.

use super::hooks::{escape_attr, escape_text};
use super::node::{Block, Inline};
use super::url::{ResolvedUrl, Url};

/// Registrable domain for a URL's kicker text.
/// `"https://www.foo.com/bar"` → `"foo.com"`.
pub fn extract_domain(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
}

/// The title fallback when a card has neither authored text nor a fetched
/// one: the domain plus whatever path follows it, so the card still says
/// something more specific than the bare domain. `"https://foo.com/a/b?x=1"`
/// → `"foo.com/a/b"` (query and fragment dropped — they're rarely
/// legible as a title).
pub fn domain_and_path(url: &str) -> String {
    let without_scheme = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let without_query = without_scheme
        .split('?')
        .next()
        .unwrap_or(without_scheme)
        .split('#')
        .next()
        .unwrap_or(without_scheme);
    without_query.trim_start_matches("www.").trim_end_matches('/').to_string()
}

/// The href + alt of the image in a cell's cover position — its FIRST
/// block, alone in a paragraph or wrapped in a figure. Mirrors
/// `build::render::grid_cells::cover_image_href`'s rule for a hand-built
/// cell; kept as a separate copy (rather than a shared call) because that
/// one reads a whole PAGE's already-resolved cell, while this one is the
/// pure fallback used with no cache or media pipeline in reach.
pub fn cover_image_href(blocks: &[Block]) -> Option<(&str, &str)> {
    let image = match blocks.first()? {
        Block::Figure { image, .. } => image,
        Block::Paragraph(inlines) => match inlines.as_slice() {
            [only] => only,
            _ => return None,
        },
        _ => return None,
    };
    match image {
        Inline::Image {
            src: Url::Resolved(r),
            alt,
            ..
        } => Some((r.href.as_str(), alt.as_str())),
        _ => None,
    }
}

/// Plain text of a block sequence, markup dropped — the pure-crate twin of
/// `build::render::grid_cells::blocks_text`. Deliberately does NOT fold in
/// an image's `alt` (unlike `ast::plain_text::inlines_to_plain_text`,
/// which is a different policy for a different job — see that module's own
/// doc comment): a caption title should read as the caption, not as
/// "alt-text caption", and the two renderers of a `Block::LinkCard` must
/// agree on this text or the same page can show two different titles
/// depending on which one ran.
fn blocks_text(blocks: &[Block]) -> String {
    let mut out = String::new();
    for block in blocks {
        match block {
            Block::Paragraph(inlines) => push_inlines_text(&mut out, inlines),
            Block::Heading { children, .. } => push_inlines_text(&mut out, children),
            _ => {}
        }
        out.push(' ');
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn push_inlines_text(out: &mut String, inlines: &[Inline]) {
    for inline in inlines {
        match inline {
            Inline::Text(t) | Inline::Code(t) => out.push_str(t),
            Inline::Emphasis(children) | Inline::Strong(children) | Inline::Strikethrough(children) => {
                push_inlines_text(out, children)
            }
            Inline::Link { children, .. } => push_inlines_text(out, children),
            Inline::LineBreak => out.push(' '),
            Inline::Image { .. } | Inline::Other(_) | Inline::FootnoteRef(_) | Inline::TaskMarker(_) => {}
        }
    }
}

/// Render a whole-cell `Block::LinkCard` whose URL leaves the site, as the
/// same `.moss-card` shell an internal collection card uses.
///
/// `children` is the card's own inner content (an image, a heading, or
/// both). When it opens with an image, that image becomes the cover —
/// author content always wins over anything a build might otherwise fetch
/// — rendered as a bare `<img>` (no variant pipeline: this crate has no
/// `.moss/build` to synthesize one from). Otherwise the card falls back to
/// the plain no-cover placeholder, same as an internal card with no cover.
/// The title is whatever text remains once the cover image is accounted
/// for, or the URL's domain and path when the cell carried no words at all
/// (a bare `[![alt](img)](url)`, no caption).
pub fn render_external_link_card(resolved: &ResolvedUrl, children: &[Block]) -> String {
    let cover = cover_image_href(children);
    let cover_html = match cover {
        Some((href, alt)) => format!(
            r#"<div class="moss-card-cover"><img src="{}" alt="{}" /></div>"#,
            escape_attr(href),
            escape_attr(alt)
        ),
        None => r#"<div class="moss-card-cover moss-card-no-cover"></div>"#.to_string(),
    };
    // The cover image's own block already spoke for itself above; the
    // title comes from whatever text is left (a heading or caption
    // alongside it), so an image-plus-heading cell doesn't repeat the same
    // words in both slots.
    let text_blocks: &[Block] = if cover.is_some() && !children.is_empty() {
        &children[1..]
    } else {
        children
    };
    let title_text = blocks_text(text_blocks);
    let title = if title_text.is_empty() {
        domain_and_path(&resolved.href)
    } else {
        title_text
    };
    let domain = extract_domain(&resolved.href);
    format!(
        r#"<a href="{}" class="moss-card" data-external target="_blank" rel="noopener">{}<div class="moss-card-content"><span class="moss-card-kicker">{}</span><span class="moss-card-meta"></span><span class="moss-card-title">{}</span></div></a>"#,
        escape_attr(&resolved.href),
        cover_html,
        escape_text(&domain),
        escape_text(&title),
    )
}

#[cfg(test)]
#[path = "link_card_tests.rs"]
mod tests;
