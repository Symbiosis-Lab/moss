//! Authored media candidates with exact source spans.
//!
//! This module does not inspect the filesystem. It preserves every physical
//! occurrence from final source bytes, then delegates the missing-asset verdict
//! to the shared reference classifier.

use std::collections::HashSet;

use pulldown_cmark::{Event, LinkType, Tag};

use crate::resolve::md_extract::{extract_md_references, RawRef, RefSyntax};
use crate::resolve::reference::{classify_reference_verdict, ReferenceContext, ResolvedReference};

/// One authored reference that can participate in an asset-resolution verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoredAssetRef {
    /// Raw authored text and its byte spans in the final source bytes.
    pub raw: RawRef,
    /// Whether this syntax follows the embed branch of the shared classifier.
    pub is_embed: bool,
}

impl AuthoredAssetRef {
    /// Ask the canonical resolver whether this occurrence is a blocking missing
    /// asset. `None` includes resolved references and all advisory references.
    pub fn missing_asset(
        &self,
        source_path: &str,
        context: &ReferenceContext,
    ) -> Option<ResolvedReference> {
        classify_reference_verdict(&self.raw.text, source_path, self.is_embed, context)
            .into_missing_asset()
    }
}

/// Extract one candidate for every physical authored reference occurrence.
///
/// Markdown candidates retain the scanner's [`RawRef`] exactly. Hero and
/// gallery candidates use the parser-aligned structural adapter, not the
/// broader rename scanner. A markdown token inside a gallery therefore meets
/// its structural counterpart at the same destination span and is retained
/// once; identical references at different spans remain separate.
///
/// Markdown inside a raw HTML block (`<pre>`, `<script>`, `<style>`, or any
/// tag-opened block up to its blank line) reaches the page as literal text, so
/// it is no candidate. The `src` of a raw `<img>`, `<video>` or `<audio>` tag
/// is one: the build serves that file as it serves a Markdown image's.
pub fn extract_authored_asset_refs(
    source: &str,
    parse_config: &crate::ast::parser::ParseConfig,
) -> Vec<AuthoredAssetRef> {
    let ParsedSpans { image_definitions, literal_html } = parsed_spans(source, parse_config);
    let mut refs: Vec<AuthoredAssetRef> = extract_md_references(source)
        .into_iter()
        .filter(|raw| !holds_ref(&literal_html, raw))
        .filter(|raw| {
            !matches!(raw.syntax, RefSyntax::Definition { .. })
                || definition_is_image(raw, &image_definitions)
        })
        .map(|raw| AuthoredAssetRef {
            is_embed: markdown_ref_is_embed(&raw.syntax)
                || definition_is_image(&raw, &image_definitions),
            raw,
        })
        .collect();
    refs.extend(
        crate::ast::shortcode_extract::authored_asset_spans(source)
            .into_iter()
            .filter_map(|span| structural_raw_ref(source, span)),
    );
    refs.extend(raw_media_tags(source).into_iter().filter_map(|tag| {
        let src = tag.src?;
        Some(AuthoredAssetRef {
            raw: RawRef {
                text: source.get(src.clone())?.to_string(),
                syntax: RefSyntax::StructuralAsset,
                byte_from: tag.byte_offset,
                byte_to: src.end,
                ref_from: src.start,
                ref_to: src.end,
            },
            is_embed: true,
        })
    }));
    refs.sort_by_key(|reference| reference.raw.ref_from);
    refs.dedup_by(|left, right| {
        left.raw.ref_from == right.raw.ref_from && left.raw.ref_to == right.raw.ref_to
    });
    refs
}

/// One author-typed `<img>`, `<video>` or `<audio>` tag.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RawMediaTag {
    /// Lowercase tag name.
    pub tag: String,
    /// Byte offset of the tag's `<` in the source.
    pub byte_offset: usize,
    /// Byte span of the `src` value, without its quotes, when the tag has one.
    pub src: Option<std::ops::Range<usize>>,
}

/// Every raw `<img>`, `<video>` and `<audio>` tag in `source`. A tag shown as
/// code, inside a comment, or in the content of a `<script>`, `<style>` or
/// `<textarea>` is text, not media, and is left out.
pub fn raw_media_tags(source: &str) -> Vec<RawMediaTag> {
    // The mask keeps every offset.
    let live = crate::inert_regions::mask_inert(source);
    let text = raw_text_elements(&live);
    let mut tags = Vec::new();
    for (at, _) in live.match_indices('<') {
        let Some(name) = ["img", "video", "audio"].into_iter().find(|name| ascii_word_at(&live, at + 1, name)) else {
            continue;
        };
        let attrs = live.get(at + 1 + name.len()..).unwrap_or_default();
        // The class after the name keeps `<imgur>` and `<videos>` out.
        let ends_name = attrs.chars().next().is_some_and(|c| c.is_whitespace() || matches!(c, '/' | '>'));
        if !ends_name || text.iter().any(|span| span.contains(&at)) {
            continue;
        }
        let attrs = attrs.split_once('>').map_or(attrs, |(inside, _)| inside);
        let offset = at + 1 + name.len();
        let src = src_value(attrs).map(|value| offset + value.start..offset + value.end);
        tags.push(RawMediaTag { tag: name.to_string(), byte_offset: at, src });
    }
    tags
}

/// Each `<script>`, `<style>` and `<textarea>` element in `live`, to its
/// closing tag or, left open, to the end, as in a browser: its content is text.
fn raw_text_elements(live: &str) -> Vec<std::ops::Range<usize>> {
    let mut spans = Vec::new();
    let mut from = 0;
    while let Some(at) = live.get(from..).and_then(|rest| rest.find('<')).map(|i| from + i) {
        let name = ["script", "style", "textarea"].into_iter().find(|name| {
            ascii_word_at(live, at + 1, name)
                && !live.get(at + 1 + name.len()..).and_then(|rest| rest.chars().next()).is_some_and(|c| c.is_alphanumeric() || c == '_')
        });
        let Some(name) = name else {
            from = at + 1;
            continue;
        };
        let end = closing_tag_end(live, at + 1 + name.len(), name).unwrap_or(live.len());
        spans.push(at..end);
        from = end;
    }
    spans
}

/// The end of the first `</name>` at or after `from`, allowing space before the `>`.
fn closing_tag_end(live: &str, from: usize, name: &str) -> Option<usize> {
    live.get(from..)?.match_indices("</").find_map(|(i, _)| {
        let after_name = from + i + 2 + name.len();
        if !ascii_word_at(live, from + i + 2, name) {
            return None;
        }
        let rest = live.get(after_name..)?;
        let gap = rest.len() - rest.trim_start().len();
        rest.get(gap..)?.starts_with('>').then_some(after_name + gap + 1)
    })
}

/// The value of the first `src` attribute in `attrs`, quoted or bare, as a range into it.
fn src_value(attrs: &str) -> Option<std::ops::Range<usize>> {
    attrs.char_indices().filter(|(_, c)| c.is_whitespace()).find_map(|(i, c)| {
        let name = i + c.len_utf8();
        if !ascii_word_at(attrs, name, "src") {
            return None;
        }
        let value = attrs.get(name + 3..)?.trim_start().strip_prefix('=')?.trim_start();
        let at = attrs.len() - value.len();
        match value.chars().next()? {
            quote @ ('"' | '\'') => value.get(1..)?.find(quote).map(|len| at + 1..at + 1 + len),
            _ => {
                let len = value.find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '>')).unwrap_or(value.len());
                (len > 0).then_some(at..at + len)
            }
        }
    })
}

/// Whether `word` (ASCII) starts at byte `at` of `s`, in any case.
fn ascii_word_at(s: &str, at: usize, word: &str) -> bool {
    s.get(at..at + word.len()).is_some_and(|found| found.eq_ignore_ascii_case(word))
}

fn definition_is_image(raw: &RawRef, image_definitions: &HashSet<std::ops::Range<usize>>) -> bool {
    matches!(raw.syntax, RefSyntax::Definition { .. }) && holds_ref(image_definitions, raw)
}

/// Whether one of `spans` holds the whole destination of `raw`.
fn holds_ref<'a>(spans: impl IntoIterator<Item = &'a std::ops::Range<usize>>, raw: &RawRef) -> bool {
    spans.into_iter().any(|span| span.start <= raw.ref_from && raw.ref_to <= span.end)
}

/// What the Markdown parser alone can say about `source`.
struct ParsedSpans {
    /// Definition source ranges whose destination is rendered by at least one
    /// Markdown image reference. Pulldown owns label normalization (including
    /// case-folding and collapsed/shortcut forms); the caller only joins its
    /// definition span to the already-exact `RawRef` destination span.
    image_definitions: HashSet<std::ops::Range<usize>>,
    /// Raw HTML blocks whose text the page carries as written. A block holding
    /// a `:::` line is left out: a shortcode is lifted out of the source before
    /// the parser runs, so its body is Markdown wherever it sits.
    literal_html: Vec<std::ops::Range<usize>>,
}

fn parsed_spans(source: &str, parse_config: &crate::ast::parser::ParseConfig) -> ParsedSpans {
    let mut parser =
        crate::ast::pulldown(source, crate::ast::parser::parser_options(parse_config.math)).into_offset_iter();
    let mut spans = ParsedSpans { image_definitions: HashSet::new(), literal_html: Vec::new() };
    while let Some((event, range)) = parser.next() {
        // allow:math-events-ignored — only the source span of an HTML block and
        // of an image's reference definition is read, for comparison against
        // `RawRef` spans. Nothing is rebuilt from the event stream.
        match event {
            Event::Start(Tag::HtmlBlock) => {
                let holds_shortcode = source
                    .get(range.clone())
                    .is_some_and(|block| block.lines().any(|line| line.trim_start().starts_with(":::")));
                if !holds_shortcode {
                    spans.literal_html.push(range);
                }
            }
            Event::Start(Tag::Image {
                link_type: LinkType::Reference | LinkType::Collapsed | LinkType::Shortcut,
                id,
                ..
            }) => {
                if let Some(definition) = parser.reference_definitions().get(id.as_ref()) {
                    spans.image_definitions.insert(definition.span.clone());
                }
            }
            _ => {}
        }
    }
    spans
}

fn markdown_ref_is_embed(syntax: &RefSyntax) -> bool {
    matches!(
        syntax,
        RefSyntax::WikilinkStemEmbed
            | RefSyntax::WikilinkPathEmbed
            | RefSyntax::WikilinkAliasedEmbed { .. }
            | RefSyntax::MarkdownImage { .. }
            | RefSyntax::StructuralAsset
    )
}

fn structural_raw_ref(
    source: &str,
    span: crate::resolve::md_extract::AssetPathSpan,
) -> Option<AuthoredAssetRef> {
    let value = source.get(span.value.clone())?;
    let relative = value.find(&span.path)?;
    let ref_from = span.value.start + relative;
    let ref_to = ref_from + span.path.len();
    Some(AuthoredAssetRef {
        raw: RawRef {
            text: span.path,
            syntax: RefSyntax::StructuralAsset,
            byte_from: span.outer.start,
            byte_to: span.outer.end,
            ref_from,
            ref_to,
        },
        is_embed: true,
    })
}

#[cfg(test)]
#[path = "authored_assets_tests.rs"]
mod tests;
