//! `::: {accordion}` — folded callouts that open one at a time.
//!
//! The fence emits `<div data-accordion="NAME">` and the callouts inside it
//! are ordinary sibling blocks, so this pass pairs them up after parsing: every
//! folded callout directly between the opener and its `</div>` takes `NAME`,
//! which renders as the native `<details name>` that makes a group exclusive.

use super::node::Block;

const MARKER: &str = " data-accordion=\"";

/// The group name written on a fence's opening `<div>`, if it is one.
fn group_of_opener(html: &str) -> Option<&str> {
    let rest = html.trim_start().strip_prefix("<div")?;
    let start = rest.find(MARKER)? + MARKER.len();
    let tail = rest.get(start..)?;
    tail.get(..tail.find('"')?)
}

/// Give every folded callout directly inside an accordion fence its group.
pub(crate) fn group_callouts(blocks: &mut [Block]) {
    for i in 0..blocks.len() {
        let Block::Other(html) = &blocks[i] else { continue };
        let Some(name) = group_of_opener(html).map(str::to_owned) else { continue };
        let mut depth = html.matches("<div").count() as isize - html.matches("</div>").count() as isize;
        for block in &mut blocks[i + 1..] {
            match block {
                Block::Other(html) => {
                    depth += html.matches("<div").count() as isize - html.matches("</div>").count() as isize;
                    if depth <= 0 {
                        break;
                    }
                }
                Block::Callout { fold: Some(_), group, .. } if depth == 1 => *group = Some(name.clone()),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::{parse, render::render_document, hooks::DefaultHooks};

    fn html(md: &str) -> String {
        render_document(&parse(md), &DefaultHooks::default())
    }

    #[test]
    fn folded_callouts_in_a_fence_share_a_name_and_two_fences_have_two() {
        let out = html("::: {accordion}\n\n> [!faq]- One\n> a\n\n> [!faq]- Two\n> b\n\n:::\n\n> [!faq]- Loose\n> c\n\n::: {accordion}\n\n> [!faq]- Three\n> d\n\n:::\n");
        let names: Vec<&str> = out.match_indices(" name=\"").map(|(i, _)| &out[i + 7..]).map(|s| &s[..s.find('"').unwrap()]).collect();
        assert_eq!(names.len(), 3, "{out}");
        assert_eq!(names[0], names[1]);
        assert_ne!(names[1], names[2]);
        assert_eq!(out.matches("<details").count(), 4);
    }
}
