//! Bound on container nesting, enforced on the event stream before the tree is built.

use pulldown_cmark::{Event, Tag, TagEnd};

/// The deepest the parser nests container blocks (lists, list items,
/// blockquotes, footnote and definition bodies). Every later pass over the
/// tree (rendering, visitors, drop) recurses once per level, so a bound here
/// bounds all of them; 32 levels render on a 768 KB debug-build stack, 2.6x under a 2 MB worker.
pub const MAX_BLOCK_NESTING: usize = 32;

fn is_container_start(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::BlockQuote(_)
            | Tag::List(_)
            | Tag::Item
            | Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
    )
}

fn is_container_end(tag: &TagEnd) -> bool {
    matches!(
        tag,
        TagEnd::BlockQuote(_)
            | TagEnd::List(_)
            | TagEnd::Item
            | TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
    )
}

/// Drop the Start/End pair of every container opened past
/// [`MAX_BLOCK_NESTING`], so the content inside stays as flat content of the
/// container at the limit. A dropped list item becomes a soft break so the
/// text of neighbouring items does not run together. Returns the byte offset
/// of the first dropped container, if any.
pub(super) fn cap_nesting<'a>(
    events: Vec<Event<'a>>,
    offsets: Vec<std::ops::Range<usize>>,
) -> (Vec<Event<'a>>, Vec<std::ops::Range<usize>>, Option<usize>) {
    let mut kept_stack: Vec<bool> = Vec::new();
    let mut kept_depth = 0;
    let mut first_dropped = None;
    let mut out_events = Vec::with_capacity(events.len());
    let mut out_offsets = Vec::with_capacity(offsets.len());
    for (event, offset) in events.into_iter().zip(offsets) {
        match &event {
            Event::Start(tag) if is_container_start(tag) => {
                let keep = kept_depth < MAX_BLOCK_NESTING;
                kept_stack.push(keep);
                if keep {
                    kept_depth += 1;
                } else {
                    first_dropped.get_or_insert(offset.start);
                    if matches!(tag, Tag::Item) {
                        out_events.push(Event::SoftBreak);
                        out_offsets.push(offset);
                    }
                    continue;
                }
            }
            Event::End(tag) if is_container_end(tag) => {
                if kept_stack.pop() == Some(true) {
                    kept_depth -= 1;
                } else {
                    continue;
                }
            }
            // Leaves: they never open or close a container.
            Event::InlineMath(_) | Event::DisplayMath(_) => {}
            _ => {}
        }
        out_events.push(event);
        out_offsets.push(offset);
    }
    (out_events, out_offsets, first_dropped)
}
