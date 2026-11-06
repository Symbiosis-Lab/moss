//! `::: {accordion}` — folded callouts that open one at a time.
//!
//! The extractor writes the fence as `<div data-accordion="NAME">` … `</div>`
//! followed by an end marker carrying the same `NAME`, and the callouts inside
//! are ordinary sibling blocks. This pass walks the blocks once, keeping the
//! fences opened and not yet ended, and gives each folded callout the innermost
//! open name, which renders as the native `<details name>` that makes a group
//! exclusive. Membership comes from the markers, never from counting tags, so
//! author HTML and comments inside the fence cannot shift it.

use super::node::Block;

const MARKER: &str = " data-accordion=\"";

/// The comment the extractor writes after a fence's closing `</div>`.
pub(crate) fn end_marker(name: &str) -> String {
    format!("<!-- {name} end -->")
}

/// The group name written on a fence's opening `<div>`, if it is one.
fn group_of_opener(html: &str) -> Option<&str> {
    let rest = html.trim_start().strip_prefix("<div")?;
    let start = rest.find(MARKER)? + MARKER.len();
    let tail = rest.get(start..)?;
    tail.get(..tail.find('"')?)
}

/// Give every folded callout inside an accordion fence its group.
pub(crate) fn group_callouts(blocks: &mut [Block]) {
    let mut open: Vec<String> = Vec::new();
    for block in blocks.iter_mut() {
        match block {
            Block::Other(html) => {
                if let Some(name) = group_of_opener(html) {
                    open.push(name.to_owned());
                } else if let Some(at) = open.iter().rposition(|n| html.trim() == end_marker(n)) {
                    open.truncate(at);
                }
            }
            Block::Callout { fold: Some(_), group, .. } => {
                if let Some(name) = open.last() {
                    *group = Some(name.clone());
                }
            }
            _ => {}
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

    fn names(out: &str) -> Vec<Option<String>> {
        out.split("<details").skip(1).map(|d| {
            let tag = &d[..d.find('>').unwrap()];
            tag.find(" name=\"").map(|i| {
                let t = &tag[i + 7..];
                t[..t.find('"').unwrap()].to_string()
            })
        }).collect()
    }

    #[test]
    fn html_comments_and_raw_html_do_not_shift_membership() {
        let md = "::: {accordion}\n\n<!-- example: <div> -->\n\n> [!faq]- One\n> a\n\n<div>\n\n> [!faq]- Two\n> b\n\n:::\n\n> [!faq]- Loose\n> c\n";
        let n = names(&html(md));
        assert_eq!(n.len(), 3);
        assert!(n[0].is_some() && n[0] == n[1], "{n:?}");
        assert_eq!(n[2], None, "{n:?}");
    }

    #[test]
    fn nested_containers_inside_and_around_a_fence() {
        let md = "::::: {.outer}\n\n> [!faq]- Before\n> x\n\n::: {accordion}\n\n::: {.band}\n\n> [!faq]- Inner\n> a\n\n:::\n\n> [!faq]- Two\n> b\n\n:::\n\n> [!faq]- After\n> y\n\n:::::\n";
        let n = names(&html(md));
        assert_eq!(n.len(), 4, "{n:?}");
        assert_eq!(n[0], None);
        assert!(n[1].is_some() && n[1] == n[2], "{n:?}");
        assert_eq!(n[3], None, "{n:?}");
    }

    #[test]
    fn a_fence_inside_a_fence_names_its_own_callouts() {
        let md = "::: {accordion}\n\n> [!faq]- A\n> a\n\n::: {accordion}\n\n> [!faq]- B\n> b\n\n:::\n\n> [!faq]- C\n> c\n\n:::\n";
        let n = names(&html(md));
        assert_eq!(n.len(), 3);
        assert_eq!(n[0], n[2]);
        assert_ne!(n[0], n[1]);
        assert!(n[1].is_some());
    }
}
