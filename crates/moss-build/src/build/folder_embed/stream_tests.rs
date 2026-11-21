use super::*;
use crate::build::scan::replies::ThreadMember;
use moss_core::fields::PageLink;

fn page(path: &str) -> ParsedDocument {
    ParsedDocument { source_path: Some(path.into()), url_path: format!("{}/index.html", path.trim_end_matches(".md")), ..Default::default() }
}

fn in_thread(path: &str, members: &[(&str, usize)]) -> ParsedDocument {
    ParsedDocument {
        thread: members.iter().map(|(p, depth)| ThreadMember { source_path: (*p).into(), depth: *depth }).collect(),
        ..page(path)
    }
}

fn drawn(doc: &ParsedDocument, all: &[ParsedDocument]) -> ThreadHtml {
    let draw = |pages: &[&ParsedDocument]| pages.iter().map(|p| format!("[{}]", p.source_path.as_deref().unwrap_or(""))).collect::<String>();
    thread_parts(doc, all, &draw)
}

#[test]
fn the_earlier_pages_go_above_and_the_later_ones_below() {
    let members = [("n/a.md", 0), ("n/b.md", 1), ("n/c.md", 2)];
    let all: Vec<_> = members.iter().map(|(p, _)| in_thread(p, &members)).collect();
    let out = drawn(&all[1], &all);
    assert!(out.before.contains("[n/a.md]") && !out.before.contains("[n/c.md]"), "{}", out.before);
    assert!(out.after.contains("[n/c.md]") && !out.after.contains("[n/a.md]"), "{}", out.after);
    assert!(out.before.contains(r#"<data class="u-in-reply-to" value="/n/a/">"#), "the parent is the page's u-in-reply-to");
}

#[test]
fn in_a_branched_thread_the_parent_is_the_one_continued() {
    // b and c both continue a.
    let members = [("n/a.md", 0), ("n/b.md", 1), ("n/c.md", 1)];
    let all: Vec<_> = members.iter().map(|(p, _)| in_thread(p, &members)).collect();
    let out = drawn(&all[2], &all);
    assert!(out.before.contains(r#"value="/n/a/""#), "c answers a, not b: {}", out.before);
    assert_eq!(drawn(&all[0], &all).before, "", "the root has nothing above it");
}

#[test]
fn an_outside_address_draws_one_linked_line_naming_its_host_and_path() {
    let mut doc = page("n/a.md");
    doc.declared.insert(
        IN_REPLY_TO.into(),
        TypedValue::Page(PageLink { target: LinkTarget::Outside("https://example.social/@someone/113402".into()), alias: None }),
    );
    let out = drawn(&doc, std::slice::from_ref(&doc));
    assert_eq!(
        out.before,
        r#"<p class="moss-in-reply-to">In reply to <a class="u-in-reply-to" href="https://example.social/@someone/113402">example.social/@someone/113402</a></p>"#
    );
    assert!(out.after.is_empty());
}

#[test]
fn a_page_in_no_thread_draws_nothing() {
    let doc = page("n/a.md");
    assert_eq!(drawn(&doc, std::slice::from_ref(&doc)), ThreadHtml::default());
}
