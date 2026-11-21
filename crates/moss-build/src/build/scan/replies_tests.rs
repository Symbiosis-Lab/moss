use super::*;
use moss_core::fields::{LinkTarget, PageLink};

fn note(path: &str, date: &str, reply: Option<LinkTarget>) -> ParsedDocument {
    let mut doc = ParsedDocument { source_path: Some(path.into()), date: Some(date.into()), ..Default::default() };
    if let Some(target) = reply {
        doc.declared.insert(IN_REPLY_TO.into(), TypedValue::Page(PageLink { target, alias: None }));
    }
    doc
}

fn page(name: &str) -> Option<LinkTarget> {
    Some(LinkTarget::Page(name.into()))
}

fn graph(docs: &[ParsedDocument]) -> ContentGraph {
    let paths: Vec<&str> = docs.iter().filter_map(|d| d.source_path.as_deref()).collect();
    ContentGraph::from_paths(&paths)
}

/// The thread `mark_threads` stores on `docs[index]`, as (document index, depth).
fn thread_of(docs: &[ParsedDocument], index: usize) -> Vec<(usize, usize)> {
    let mut marked = docs.to_vec();
    mark_threads(&mut marked, &graph(docs));
    let at = |source: &str| docs.iter().position(|d| d.source_path.as_deref() == Some(source)).unwrap();
    marked[index].thread.iter().map(|m| (at(&m.source_path), m.depth)).collect()
}

#[test]
fn a_continuation_is_found_from_its_parent_though_only_the_child_names_it() {
    let docs = vec![note("notes/a.md", "2026-01-01", None), note("notes/b.md", "2026-01-02", page("a"))];
    let links = Replies::build(&docs, &graph(&docs));
    assert_eq!(links.children.get(&0), Some(&vec![1]));
    assert_eq!(links.parent(1), Some(0));
}

#[test]
fn every_member_reads_the_same_thread_root_first_continuations_oldest_first() {
    // c and d both continue b; d is older, so it comes first.
    let docs = vec![
        note("notes/a.md", "2026-01-01", None),
        note("notes/b.md", "2026-01-02", page("a")),
        note("notes/c.md", "2026-01-05", page("b")),
        note("notes/d.md", "2026-01-03", page("b")),
        note("notes/e.md", "2026-01-06", page("d")),
    ];
    let whole = vec![(0, 0), (1, 1), (3, 2), (4, 3), (2, 2)];
    for i in 0..docs.len() {
        assert_eq!(thread_of(&docs, i), whole, "from member {i}");
    }
}

#[test]
fn an_outside_address_joins_no_thread_and_is_never_in_the_inverse() {
    let docs = vec![
        note("notes/a.md", "2026-01-01", None),
        // Read as a page name, the address would resolve to a by its last segment.
        note("notes/b.md", "2026-01-02", Some(LinkTarget::Outside("https://example.social/notes/a".into()))),
    ];
    let links = Replies::build(&docs, &graph(&docs));
    assert_eq!(links.parent(1), None);
    assert!(links.children.is_empty());
    assert!(thread_of(&docs, 1).is_empty());
}

#[test]
fn a_page_alone_or_naming_a_missing_page_has_no_thread() {
    let docs = vec![note("notes/a.md", "2026-01-01", None), note("notes/b.md", "2026-01-02", page("gone"))];
    assert!(thread_of(&docs, 0).is_empty());
    assert!(thread_of(&docs, 1).is_empty());
}

#[test]
fn a_loop_of_replies_ends_where_it_closes() {
    let docs = vec![note("notes/a.md", "2026-01-01", page("b")), note("notes/b.md", "2026-01-02", page("a"))];
    assert_eq!(thread_of(&docs, 0), vec![(0, 0), (1, 1)]);
    assert_eq!(thread_of(&docs, 1), thread_of(&docs, 0), "one thread, one order");
}
