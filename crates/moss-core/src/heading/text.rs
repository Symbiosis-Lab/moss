//! "Inline content → plain text", once.
//!
//! Two walkers need this operation and they cannot be merged, because they
//! read different inputs at different times:
//!
//! | walker | input | when | consumer |
//! |---|---|---|---|
//! | [`events_to_text`] | `&[pulldown_cmark::Event]` | during parse, before the AST exists | the `<hN id>` slug |
//! | [`crate::ast::plain_text::inlines_to_plain_text`] | `&[Inline]` | after parse | [`super::extract`]'s autocomplete label |
//!
//! What they must NOT do is disagree about what a piece of content looks
//! like as text. They did: `collect_heading_text` in `ast/parser.rs` and
//! `inlines_to_text` in `extract_headings.rs` were independent `match`
//! arms over independent enums, so a heading's slug and its autocomplete
//! label could drift apart — which is exactly what the July 2026 math
//! cluster found (`$f*g$` came out of one and `$fg$` out of the other).
//!
//! So: **one policy, two adapters.** [`crate::ast::plain_text::TextAtom`] is
//! the vocabulary the policy speaks; `push_atom` IS the policy and is the
//! only place that decides what an atom's text is; each walker's job is
//! reduced to classifying its own node type into an atom. Changing what math
//! (or a line break, or code) looks like in plain text is a one-line edit in
//! one function, and both surfaces move together by construction.
//!
//! The `&[Inline]` half of the policy (and its adapter,
//! [`crate::ast::plain_text::inlines_to_plain_text`]) moved to
//! `ast/plain_text.rs` (ADR-036) once a third non-heading consumer
//! (`build::page::meta::extract_description`) appeared — exactly the
//! trigger that module's promotion doc comment named in advance. This
//! module keeps only [`events_to_text`], the mid-parse event-stream half
//! that has no AST to walk yet.
//!
//! ## The one difference that remains, and why it is not a bug to fix here
//!
//! The two walkers see different *vocabularies*, not different policies:
//! the event stream carries `SoftBreak`/`HardBreak` events that the slug
//! walker has always ignored, while the AST folds a `SoftBreak` into
//! `Inline::Text("\n")` and a `HardBreak` into [`Inline::LineBreak`]. A
//! multi-line setext heading therefore slugs as `foobar` but labels as
//! `foo bar`. That predates this module and is a *behavioral* question —
//! changing it moves live anchors. It is pinned by
//! `setext_soft_break_divergence_is_pinned_not_fixed` below so the next
//! person meets it as a decision rather than as a surprise.

use std::borrow::Cow;

use crate::ast::math_text::math_source;
use crate::ast::plain_text::{push_atom, TextAtom};
use pulldown_cmark::Event;

/// Flatten the parser events in `events[start..end]` to plain text.
///
/// Runs mid-parse, where the only thing that exists is the event stream —
/// this is what `ast/parser.rs` slugs into the rendered `<hN id="…">`. The
/// caller passes the range *inside* the heading tags (exclusive of the
/// matching `Event::End(TagEnd::Heading)`).
///
/// Mirrors production's `transform_events` heading-text collection at
/// `src-tauri/src/build/markdown/pipeline.rs`. Inline HTML
/// (`Event::InlineHtml` / `Event::Html`) is intentionally skipped, so
/// `# FAREWELL,<br>AND ERASE` slugs as `FAREWELL,AND ERASE` with no `<br>`
/// in the anchor. Link and image *labels* are captured: pulldown walks the
/// events inside `Tag::Link` / `Tag::Image` transparently and their
/// `Event::Text` payloads land here, matching production; the href does
/// not.
pub(crate) fn events_to_text(events: &[Event<'_>], start: usize, end: usize) -> String {
    let mut out = String::new();
    for event in &events[start..end] {
        match event {
            Event::Text(t) => push_atom(&mut out, TextAtom::Verbatim(t)),
            Event::Code(c) => push_atom(&mut out, TextAtom::Verbatim(c)),
            Event::InlineMath(t) => push_atom(&mut out, math_atom(t, false)),
            Event::DisplayMath(t) => push_atom(&mut out, math_atom(t, true)),
            // Start/End tags carry no text of their own; their contents
            // arrive as their own Text events (image `alt` included).
            // Soft/HardBreak deliberately contribute nothing — see the
            // module doc's "one difference that remains".
            _ => {}
        }
    }
    out
}

/// Build a [`TextAtom::Math`] from the inner TeX pulldown hands this
/// walker (delimiters stripped) plus its display flag. `events_to_text`'s
/// own convenience — the `&[Inline]` walker recovers math from
/// `Inline::Other` via `math_source_from_other` instead, since it has no
/// original event to read the display flag from.
fn math_atom(tex: &str, display: bool) -> TextAtom<'static> {
    TextAtom::Math(Cow::Owned(math_source(tex, display)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::parser::ParseConfig;
    use crate::ast::{parse_with_config, Block};
    use crate::heading::anchor::obsidian_heading_anchor;

    /// The unification's reason to exist: the slug (event walk, mid-parse)
    /// and the label (inline walk, post-parse) must describe the same
    /// heading. `$f*g$` and `$V^*$` are the vectors that caught them apart
    /// — with math OFF the `*` is emphasis markup, so a walker that drops
    /// what it does not recognize produces `$fg$` on one side and `$f*g$`
    /// on the other.
    #[test]
    fn event_walk_and_inline_walk_agree_on_every_heading() {
        for md in [
            "# Euler $e^{i\\pi}=-1$ identity\n",
            "# Convolution $f*g$ and $h*k$ end\n",
            "# Dual $V^*$ and $W^*$ end\n",
            "# Case $$a+b$$ tail\n",
            "# Mixed `code` and *em* and **strong** here\n",
            "# A [link](/x) and ![some alt](/i.png) inline\n",
            "# 中文 $\\alpha$ 标题\n",
            "# Nested *em with `code` and $x^2$* tail\n",
        ] {
            for math in [false, true] {
                let cfg = ParseConfig { math, ..Default::default() };
                let doc = parse_with_config(md, &cfg);
                let Block::Heading { children, id, .. } = &doc.blocks[0] else {
                    panic!("expected a heading for {md:?}");
                };
                let label = crate::ast::plain_text::inlines_to_plain_text(children);
                // `id` was produced by `events_to_text` during the parse.
                assert_eq!(
                    id.as_deref().expect("heading must have an id"),
                    obsidian_heading_anchor(&label),
                    "slug and label disagree for {md:?} (math={math})"
                );
            }
        }
    }

    /// Pins the one place the two walkers still differ, so it is a recorded
    /// decision and not a latent surprise. A setext heading is the only
    /// heading shape that can contain a break at all; the event walk drops
    /// it, the AST folds a SoftBreak into `Text("\n")`. Fixing it would
    /// move live anchors, which is out of scope for a structure-only
    /// consolidation.
    #[test]
    fn setext_soft_break_divergence_is_pinned_not_fixed() {
        let doc = parse_with_config("foo\nbar\n===\n", &ParseConfig::default());
        let Block::Heading { children, id, .. } = &doc.blocks[0] else {
            panic!("expected a setext heading, got {:?}", doc.blocks[0]);
        };
        let label = crate::ast::plain_text::inlines_to_plain_text(children);
        assert_eq!(label, "foo\nbar", "AST keeps the SoftBreak as a newline");
        assert_eq!(id.as_deref(), Some("foobar"), "the event walk drops it");
    }

    #[test]
    fn policy_is_one_function() {
        let mut out = String::new();
        push_atom(&mut out, TextAtom::Verbatim("a"));
        push_atom(&mut out, TextAtom::Break);
        push_atom(&mut out, math_atom("x^2", false));
        push_atom(&mut out, math_atom("y", true));
        assert_eq!(out, "a $x^2$$$y$$");
    }
}
