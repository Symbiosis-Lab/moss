//! `[![[image.png]]](/somewhere/)` — a wikilink embed wrapped in a markdown
//! link.
//!
//! # What pulldown-cmark does with it, and why that is unusable
//!
//! Measured against pulldown-cmark 0.13 with moss's exact option set
//! ([`super::parser::parser_options`], `ENABLE_WIKILINKS` included):
//!
//! ```text
//! [![[card.png]]](/awards/x/)
//!   Text("[")
//!   Start(Image { link_type: Inline, dest_url: "/awards/x/" })
//!   Text("]") Text("(/awards/x/)")
//!   End(Image)
//! ```
//!
//! There is no `Link` node; the `<img>`'s `src` is the *link's* URL, the alt
//! text is the leftover source, and the wikilink target — `card.png`, the one
//! thing the author actually named — is gone from the event stream entirely.
//! The equivalent CommonMark spelling `[![alt](card.png)](/awards/x/)` parses
//! as `Link(Image)`, exactly as authored.
//!
//! Because the target is *destroyed*, no post-parse fixup over the typed AST
//! can recover it, and no event-stream fixup can either. A pre-parse
//! substitution is the only available mechanism — the same conclusion, for the
//! same reason, that [`super::line_breaks`] reaches for hard line breaks.
//!
//! # The mechanism
//!
//! Two halves, both in this module:
//!
//! 1. [`substitute`] runs on the markdown *after* shortcode extraction and
//!    replaces the inner `![[…]]` of every confirmed compound shape with a
//!    single-token sentinel (`U+E000 <nonce> : <index> U+E001`, private-use
//!    area — see `OPEN` below for why the nonce is load-bearing). pulldown then
//!    sees `[<sentinel>](/awards/x/)` — an ordinary inline link with plain
//!    text inside — and emits `Link(Text)`.
//! 2. [`restore`] walks the assembled blocks and replaces each sentinel
//!    `Inline::Text` with the inlines produced by parsing the embed source on
//!    its own. Where an `Inline::Image` would be nonsense — a `Url`, a
//!    `title`, an image `alt`, a code span, a raw-HTML payload — the author's
//!    original bytes go back instead.
//!
//! Half 2 re-enters [`super::parser::parse_with_config`] rather than
//! hand-building an `Inline::Image`. That is the whole point of the design:
//! `is_wikilink`, `wikilink_pothole` and the alt-vs-display-attrs-vs-params
//! classification in the parser's `Tag::Image` arm are intricate and they stay
//! in ONE place. A linked embed is by construction identical to the same embed
//! standing alone, because it is literally parsed by the same code.
//!
//! Substitution is line-count preserving (the sentinel contains no newline and
//! replaces a single-line span), which is the invariant `LineLookup` in
//! `parser.rs` needs — see the identical note on the shortcode placeholder in
//! `shortcode_extract.rs`.
//!
//! # What is deliberately NOT matched
//!
//! - Anything inside an inert region ([`crate::inert_regions`]): fenced or
//!   indented code, an inline code span, an HTML comment.
//! - `\![[x]]` — the escape keeps pulldown's literal `!` + wikilink *link*.
//! - `[![[x.png]]]` with no `(url)` — no link to build, so the shape is left
//!   for pulldown, which already parses the embed correctly there.
//! - `![[]]` — an empty target has nothing to resolve; left literal.
//! - `![![[x.png]]](/u)` — an outer `!` makes the whole thing an image alt,
//!   not a link.
//!
//! Grid cells never reach this pass: `:::grid` bodies are lifted out by
//! shortcode extraction before [`substitute`] runs, and
//! `shortcode_extract::detect_compound_link` keeps owning the cell-level
//! `Block::LinkCard` shape (a card is block-level markup with its own
//! `<a class="moss-grid-card">` chrome — a different output from the inline
//! `Link(Image)` this module produces, not a duplicate of it).

use std::borrow::Cow;

use super::node::{Block, Inline};
use super::parser::ParseConfig;
use super::url::Url;
use crate::inert_regions::mask_inert;

/// Opens a substitution sentinel. Private-use area, so it survives
/// pulldown-cmark as ordinary `Event::Text` (verified: no splitting, no
/// escaping, inside links, emphasis and table cells alike).
///
/// The PUA codepoint alone is NOT a uniqueness argument — an author can type
/// `U+E000`, and a sentinel-shaped run in their prose would otherwise be
/// consumed and replaced by an image they never wrote. The body therefore
/// carries the per-extraction nonce, exactly as `shortcode_extract`'s
/// `<!--MOSS_SC_{nonce}_{index}-->` does: a literal collision would have to
/// embed a hash of the very text containing it, which is computationally
/// improbable. Anything that does not match the nonce is left as author text.
const OPEN: char = '\u{e000}';
/// Closes a substitution sentinel.
const CLOSE: char = '\u{e001}';

/// The `![[…]]` sources lifted out by [`substitute`], indexed by sentinel id.
#[derive(Debug, Default)]
pub(super) struct LinkedEmbeds {
    sources: Vec<String>,
    /// The per-extraction nonce this run's sentinels carry. Empty when
    /// nothing was substituted.
    nonce: String,
}

/// Replace the inner `![[…]]` of every `[![[…]]](url)` in `markdown` with a
/// sentinel, returning the rewritten markdown and the lifted sources.
///
/// `nonce` is [`super::shortcode_extract::ExtractionResult::nonce`] — the
/// hash of this parse's input. Reusing it rather than minting a second one
/// keeps ONE answer to "is this run of bytes a moss sentinel or the author's".
///
/// Returns `Cow::Borrowed` untouched when there is no match, which is the
/// overwhelmingly common case — the scan is a `memchr`-shaped search for the
/// four-byte `[![[` opener over an inert-masked copy.
pub(super) fn substitute<'a>(markdown: &'a str, nonce: &str) -> (Cow<'a, str>, LinkedEmbeds) {
    let mut embeds = LinkedEmbeds::default();
    if !markdown.contains("[![[") {
        return (Cow::Borrowed(markdown), embeds);
    }
    // `mask_inert` is byte-length- and line-preserving, so an offset found in
    // the mask is the same offset in `markdown`.
    let mask = mask_inert(markdown);
    let bytes = mask.as_bytes();

    let mut out = String::with_capacity(markdown.len());
    let mut copied = 0usize;
    let mut i = 0usize;
    while i + 4 <= bytes.len() {
        if !bytes[i..].starts_with(b"[![[") {
            i += 1;
            continue;
        }
        // An outer `!` (`![![[x]]](/u)`) makes this an image's alt text, not a
        // link; a backslash escapes the `[`.
        if i > 0 && (bytes[i - 1] == b'!' || bytes[i - 1] == b'\\') {
            i += 1;
            continue;
        }
        let Some((embed_end, target_len)) = embed_span(bytes, i + 1) else {
            i += 1;
            continue;
        };
        if target_len == 0 || !link_closes_at(bytes, embed_end) {
            i += 1;
            continue;
        }
        let embed_start = i + 1;
        // Char-aligned: `i` indexes the ASCII `[` of `[![[`, so `embed_start`
        // is its ASCII `!`; `embed_end` is one past the ASCII pair `]]` that
        // `embed_span` located. No byte of a multi-byte UTF-8 sequence is ever
        // ASCII, so every one of these offsets is a char boundary — the same
        // argument `inert_regions` makes for the mask these bytes come from.
        #[allow(clippy::string_slice)]
        {
            out.push_str(&markdown[copied..embed_start]);
            out.push_str(&sentinel_for(nonce, embeds.sources.len()));
            embeds
                .sources
                .push(markdown[embed_start..embed_end].to_string());
        }
        copied = embed_end;
        i = embed_end;
    }
    if embeds.sources.is_empty() {
        return (Cow::Borrowed(markdown), embeds);
    }
    embeds.nonce = nonce.to_string();
    // Char-aligned: `copied` is an `embed_end` (see above) or 0.
    #[allow(clippy::string_slice)]
    out.push_str(&markdown[copied..]);
    (Cow::Owned(out), embeds)
}

/// The sentinel that stands in for embed `index` during this parse.
/// `U+E000 <nonce> : <index> U+E001` — no newline, so the substitution is
/// line-count preserving, which is what `LineLookup` in `parser.rs` needs.
fn sentinel_for(nonce: &str, index: usize) -> String {
    format!("{OPEN}{nonce}:{index}{CLOSE}")
}

/// Given `start` pointing at the `!` of `![[`, return the byte index just
/// past the closing `]]` plus the target's byte length, or `None` if the
/// embed is unterminated or spans a line break.
fn embed_span(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    let target_start = start + 3;
    let mut j = target_start;
    while j + 1 < bytes.len() {
        match bytes[j] {
            // A wikilink never spans lines, and `[`/`]` inside the target
            // means this is some other shape (a nested link, an unclosed
            // embed) that we must not claim.
            b'\n' | b'[' => return None,
            b']' if bytes[j + 1] == b']' => return Some((j + 2, j - target_start)),
            b']' => return None,
            _ => j += 1,
        }
    }
    None
}

/// True when `at` is the `]` closing the outer link text and it is followed
/// by a complete, single-line `(destination)`.
///
/// Deliberately strict: only the exact `[<embed>](url)` shape is claimed. A
/// link text that holds more than the embed (`[![[x.png]] more](/u)`) keeps
/// today's behavior rather than gaining a second, differently-derived one.
fn link_closes_at(bytes: &[u8], at: usize) -> bool {
    if bytes.get(at) != Some(&b']') || bytes.get(at + 1) != Some(&b'(') {
        return false;
    }
    let mut depth = 1usize;
    let mut j = at + 2;
    while j < bytes.len() {
        match bytes[j] {
            b'\\' => j += 1,
            b'\n' => return false,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return true;
                }
            }
            _ => {}
        }
        j += 1;
    }
    false
}

/// Replace the sentinels in `blocks` with the inlines their `![[…]]` sources
/// parse to on their own. A no-op when [`substitute`] found nothing.
///
/// EVERY field of `Block` and `Inline` is visited, enforced by the compiler:
/// no match arm in [`Restore::in_block`] / [`Restore::in_inline`] uses `..`,
/// so a new AST field cannot be forgotten here. Inline vectors get the embed
/// as an `Inline::Image`; the string-ish leaves — `Url`, `title`, image `alt`,
/// `wikilink_pothole`, callout `title`, footnote `label`, `Inline::Code`,
/// `Block::CodeBlock`, `Block::Other` — get the author's original bytes back
/// instead. `Block::Shortcode` is the one exception, and only because its
/// cells were parsed (and restored) by their own `parse_document` call.
///
/// Must run BEFORE `assign_heading_id_suffixes`: a heading whose text held a
/// sentinel gets its `id` recomputed here, and the duplicate-numbering pass
/// has to see the corrected base slug.
pub(super) fn restore(blocks: &mut [Block], embeds: &LinkedEmbeds, config: &ParseConfig) {
    if embeds.sources.is_empty() {
        return;
    }
    let ctx = Restore {
        embeds,
        // The embed is parsed as a fragment of the enclosing paragraph, so it
        // must NOT promote to a `Block::Figure` — we want the bare
        // `Inline::Image` to nest inside the `Inline::Link`. `math` is carried
        // because a pothole may hold `$…$`; the other flags are irrelevant to
        // a one-line inline parse.
        config: ParseConfig {
            emit_source_lines: false,
            implicit_figure: false,
            source_line_offset: 0,
            math: config.math,
            hard_line_breaks: false,
        },
    };
    for block in blocks.iter_mut() {
        ctx.in_block(block);
    }
}

struct Restore<'a> {
    embeds: &'a LinkedEmbeds,
    config: ParseConfig,
}

impl Restore<'_> {
    /// # Why every arm destructures exhaustively (no `..`)
    ///
    /// Four sentinel leaks have shipped from this file, each one a field the
    /// author of the previous fix did not think of: `Block::Table.header`,
    /// then `Inline::Image.wikilink_pothole`, with `Block::Callout.title` and
    /// `Block::FootnoteDefinition.label` sitting unvisited behind a
    /// `..`-swallowed match arm. Finding an input that reaches a field is a
    /// weak proof — the reviewer who could not build one for `title` was
    /// right about today's parser and would be wrong the day it changes.
    ///
    /// So no arm here may use `..`. Adding a field to `Block` or `Inline` is
    /// now a COMPILE ERROR in this function, which forces the next author to
    /// answer "can this hold a sentinel?" instead of leaking one. Bind an
    /// irrelevant field to `_` and the answer is recorded in the source.
    fn in_block(&self, block: &mut Block) {
        match block {
            Block::Heading { children, id, level: _ } => {
                if self.in_inlines(children) {
                    // The `id` was slugged mid-parse from the event stream,
                    // when the heading's text still read `U+E000…`. Recompute
                    // it from the restored inlines using the OTHER adapter of
                    // the same one policy (`heading::text`'s module doc: one
                    // policy, two adapters) — `inlines_to_plain_text` +
                    // `obsidian_heading_anchor` is exactly the equality
                    // `heading::text`'s `event_walk_and_inline_walk_agree_…`
                    // test already asserts must hold for every heading. Only
                    // sentinel-bearing headings are touched, so no existing
                    // anchor moves.
                    let text = crate::ast::plain_text::inlines_to_plain_text(children);
                    *id = Some(crate::heading::anchor::obsidian_heading_anchor(&text));
                }
            }
            Block::Paragraph(children) => {
                self.in_inlines(children);
            }
            Block::Figure {
                image,
                caption,
                width: _,
                align: _,
                class_names: _,
                img_style: _,
            } => {
                self.in_inline(image);
                if let Some(caption) = caption {
                    self.in_inlines(caption);
                }
            }
            Block::LinkCard { url, children } => {
                self.literal_url(url);
                for nested in children {
                    self.in_block(nested);
                }
            }
            // `title` is plain text on the callout marker line
            // (`> [!note] [![[x.png]]](/u)`), not an inline vector — the
            // author's bytes go back.
            Block::Callout {
                title,
                children,
                kind: _,
                fold: _,
            } => {
                self.literal_opt(title);
                for nested in children {
                    self.in_block(nested);
                }
            }
            Block::BlockQuote(children) => {
                for nested in children {
                    self.in_block(nested);
                }
            }
            // The definition's `label` must stay byte-equal to the
            // `Inline::FootnoteRef` that points at it, so both are restored
            // the same way.
            Block::FootnoteDefinition { label, children } => {
                self.literal(label);
                for nested in children {
                    self.in_block(nested);
                }
            }
            Block::List {
                items,
                ordered: _,
                start: _,
                item_source_lines: _,
            } => {
                for item in items {
                    for nested in item {
                        self.in_block(nested);
                    }
                }
            }
            Block::Table {
                header,
                rows,
                alignments: _,
                header_source_line: _,
                row_source_lines: _,
            } => {
                // The header row is a `Vec<Vec<Inline>>` of its own; a `..`
                // that swallowed it dropped the author's embed on the floor
                // and published a `U+E000` as link text.
                for cell in header.iter_mut().chain(rows.iter_mut().flatten()) {
                    self.in_inlines(cell);
                }
            }
            // Raw payloads: a sentinel can only land here if the compound
            // shape sat inside an HTML block (not an inert region), so put
            // the author's bytes back rather than leaking `U+E000`.
            Block::CodeBlock { value, lang } => {
                self.literal(value);
                self.literal_opt(lang);
            }
            Block::Other(html) => {
                self.literal(html);
            }
            // A shortcode's cells were parsed by their own `parse_document`
            // call and restored there.
            Block::Shortcode(_) | Block::ThematicBreak => {}
        }
    }

    /// Returns true when this vector (or anything nested in it) changed.
    fn in_inlines(&self, inlines: &mut Vec<Inline>) -> bool {
        let has_sentinel = inlines
            .iter()
            .any(|i| matches!(i, Inline::Text(t) if t.contains(OPEN)));
        if !has_sentinel {
            let mut changed = false;
            for inline in inlines.iter_mut() {
                changed |= self.in_inline(inline);
            }
            return changed;
        }
        let mut out: Vec<Inline> = Vec::with_capacity(inlines.len());
        for mut inline in std::mem::take(inlines) {
            if let Inline::Text(text) = &inline {
                if text.contains(OPEN) {
                    out.extend(self.expand(text));
                    continue;
                }
            }
            self.in_inline(&mut inline);
            out.push(inline);
        }
        *inlines = out;
        true
    }

    /// Returns true when anything under `inline` changed.
    fn in_inline(&self, inline: &mut Inline) -> bool {
        match inline {
            Inline::Emphasis(children)
            | Inline::Strong(children)
            | Inline::Strikethrough(children) => self.in_inlines(children),
            // `url` and `title` are reachable: `[![[a.png]]]([![[b.png]]](/u))`
            // puts a sentinel inside the outer link's DESTINATION, and an
            // `href` holding a private-use codepoint is not a URL.
            Inline::Link {
                children,
                url,
                title,
                is_wikilink: _,
            } => {
                let mut changed = self.in_inlines(children);
                changed |= self.literal_url(url);
                changed |= self.literal_opt(title);
                changed
            }
            // `wikilink_pothole` is the fourth sentinel-bearing field found in
            // this file: `![[a.png|[![[b.png]]](/u)]]` puts a whole compound
            // shape in the pothole, `substitute` replaces its inner embed, and
            // the `..` that used to sit here skipped the field — leaving a
            // live `U+E000` for `dispatch_wikilink_embeds` to re-read as
            // display params.
            Inline::Image {
                src,
                alt,
                title,
                wikilink_pothole,
                is_wikilink: _,
            } => {
                let mut changed = self.literal_url(src);
                changed |= self.literal(alt);
                changed |= self.literal_opt(title);
                changed |= self.literal_opt(wikilink_pothole);
                changed
            }
            Inline::Code(code) => self.literal(code),
            Inline::Other(raw) => self.literal(raw),
            // A footnote reference's label is restored so it keeps matching
            // the `Block::FootnoteDefinition` label restored above.
            Inline::FootnoteRef(label) => self.literal(label),
            Inline::Text(_) | Inline::LineBreak | Inline::TaskMarker(_) => false,
        }
    }

    /// Split `text` on its sentinels, parsing each embed source into inlines.
    fn expand(&self, text: &str) -> Vec<Inline> {
        let mut out: Vec<Inline> = Vec::new();
        for segment in self.segments(text) {
            match segment {
                Segment::Text("") => {}
                Segment::Text(s) => out.push(Inline::Text(s.to_string())),
                Segment::Embed(source) => out.extend(self.embed_inlines(source)),
            }
        }
        out
    }

    /// Split `text` into literal runs and embed sources.
    ///
    /// A sentinel-shaped run that is not one of THIS parse's sentinels — a
    /// lone `U+E000`, a foreign or absent nonce, an id with no entry — stays
    /// literal text. That is what makes an author who types `U+E000` in prose
    /// safe: their bytes come back out, byte for byte. Every byte of `text`
    /// appears in exactly one returned segment, so no caller can lose one.
    ///
    /// Char-aligned throughout: every offset is either a `str::find` result
    /// (always a char boundary) or that result advanced by the matched char's
    /// own `len_utf8()`.
    #[allow(clippy::string_slice)]
    fn segments<'t>(&'t self, text: &'t str) -> Vec<Segment<'t>> {
        let mut out: Vec<Segment<'t>> = Vec::new();
        let mut rest = text;
        let mut pending = 0usize;
        while let Some(open) = rest[pending..].find(OPEN) {
            let open = pending + open;
            let after = &rest[open + OPEN.len_utf8()..];
            let source = after
                .find(CLOSE)
                .and_then(|close| {
                    // `<nonce>:<index>` — a body that is not this parse's
                    // nonce belongs to the author, not to us.
                    let body = &after[..close];
                    let id = body.strip_prefix(&self.embeds.nonce)?.strip_prefix(':')?;
                    id.parse::<usize>().ok().map(|id| (close, id))
                })
                .and_then(|(close, id)| self.embeds.sources.get(id).map(|s| (close, s)));
            match source {
                Some((close, source)) => {
                    out.push(Segment::Text(&rest[..open]));
                    out.push(Segment::Embed(source));
                    rest = &after[close + CLOSE.len_utf8()..];
                    pending = 0;
                }
                None => pending = open + OPEN.len_utf8(),
            }
        }
        out.push(Segment::Text(rest));
        out
    }

    /// Parse one `![[…]]` source on its own and hand back its inlines.
    ///
    /// A well-formed embed yields exactly `[Inline::Image { is_wikilink: true,
    /// .. }]`. Anything else (an embed the parser declined to model) falls
    /// back to the author's literal bytes — never a panic, never a dropped
    /// span.
    fn embed_inlines(&self, source: &str) -> Vec<Inline> {
        let doc = super::parser::parse_with_config(source, &self.config);
        match doc.blocks.into_iter().next() {
            Some(Block::Paragraph(inlines))
                if matches!(inlines.as_slice(), [Inline::Image { .. }]) =>
            {
                inlines
            }
            _ => vec![Inline::Text(source.to_string())],
        }
    }

    /// Put the author's original bytes back into a raw string payload —
    /// anywhere an `Inline::Image` would be nonsense (a `Url`, a `title`, an
    /// `alt`, a code span, a raw-HTML payload). Returns true if it changed.
    fn literal(&self, text: &mut String) -> bool {
        if !text.contains(OPEN) {
            return false;
        }
        let source = std::mem::take(text);
        let mut out = String::with_capacity(source.len());
        for segment in self.segments(&source) {
            match segment {
                Segment::Text(s) | Segment::Embed(s) => out.push_str(s),
            }
        }
        *text = out;
        true
    }

    /// [`literal`](Self::literal) for an optional string (`title`).
    fn literal_opt(&self, text: &mut Option<String>) -> bool {
        text.as_mut().is_some_and(|t| self.literal(t))
    }

    /// [`literal`](Self::literal) for a URL. Both variants are handled because
    /// `Url` is `pub` and a caller could hand `restore` an already-resolved
    /// tree; at parse time every URL is still `Unresolved`.
    fn literal_url(&self, url: &mut Url) -> bool {
        match url {
            Url::Unresolved(raw) => self.literal(raw),
            Url::Resolved(resolved) => self.literal(&mut resolved.href),
        }
    }
}

/// One piece of a sentinel-bearing string: either literal author text or the
/// `![[…]]` source a sentinel stands for.
enum Segment<'a> {
    Text(&'a str),
    Embed(&'a str),
}

#[cfg(test)]
#[path = "linked_embed_tests.rs"]
mod tests;
