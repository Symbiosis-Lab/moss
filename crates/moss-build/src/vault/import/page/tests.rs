use super::*;

#[test]
fn typed_page_preserves_each_readers_order_escaping_and_body_bytes() {
    let page = ImportedPage {
        frontmatter: FrontMatter {
            title: Some("  A\"B\n---\nC\u{001c}  ".into()),
            date: Some("2024-02-03".into()),
            origin: Some("https://example.test/post/".into()),
            tags: Some(vec![" space ".into(), "a\\b".into()]),
            rating: Some(4),
            ..Default::default()
        },
        body: "\n Body  \n".into(),
    };
    let prefix = "---\ntitle: \"A\\\"B\\n---\\nC\"\ndate: \"2024-02-03\"\n";
    let html = page.render(PageStyle::Html);
    assert_eq!(html, format!("{prefix}tags: [\" space \", \"a\\\\b\"]\norigin: \"https://example.test/post/\"\nrating: 4\n---\n\n\n Body  \n"));
    let backup = page.render(PageStyle::Backup);
    assert_eq!(backup, format!("{prefix}origin: \"https://example.test/post/\"\nrating: 4\ntags: [\" space \", \"a\\\\b\"]\n---\n\nBody\n"));
}

#[test]
fn typed_title_replacement_consumes_multiline_value_without_leaving_old_text() {
    let source = "---\norigin: 'https://example.test/'\ntitle: |\n  Old title\n  stale continuation\ndescription: 'keep: this'\n---\n\nbody\n";
    let updated = with_title(source, "New\n---\ntitle").unwrap();
    assert_eq!(updated, "---\norigin: 'https://example.test/'\ntitle: \"New\\n---\\ntitle\"\ndescription: 'keep: this'\n---\n\nbody\n");
    let fields = moss_core::frontmatter::frontmatter_map(&updated);
    assert_eq!(fields["title"].as_str(), Some("New\n---\ntitle"));
    assert_eq!(fields["description"].as_str(), Some("keep: this"));
}

#[test]
fn typed_additions_preserve_existing_fields_and_body() {
    let source = "---\ntitle: 'as written'\nnav: false\n---\n\nBody  \n";
    let updated = with_fields_absent(source, &FrontMatter {
        nav: Some(true), children: Some(false), ..Default::default()
    }).unwrap();
    assert_eq!(updated, "---\ntitle: 'as written'\nnav: false\nchildren: false\n---\n\nBody  \n");
}
