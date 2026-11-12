//! Authored media candidates with exact source spans.
//!
//! This module does not inspect the filesystem. It preserves every physical
//! occurrence from final source bytes, then delegates the missing-asset verdict
//! to the shared reference classifier.

use std::collections::HashSet;

use pulldown_cmark::{Event, LinkType, Parser, Tag};

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
/// it is no candidate.
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
    refs.sort_by_key(|reference| reference.raw.ref_from);
    refs.dedup_by(|left, right| {
        left.raw.ref_from == right.raw.ref_from && left.raw.ref_to == right.raw.ref_to
    });
    refs
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
        Parser::new_ext(source, crate::ast::parser::parser_options(parse_config.math)).into_offset_iter();
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
