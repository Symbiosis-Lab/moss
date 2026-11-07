use super::*;
use crate::ast::Url;
use proptest::prelude::*;

fn site(files: &[&str]) -> SiteInputs {
    SiteInputs {
        notes: HashMap::new(),
        files: files.iter().map(|path| path.to_string()).collect(),
        schema: crate::schema::builtin_schema(),
        lang: "en".into(),
    }
}

#[test]
fn wrong_typed_field_points_to_its_own_line() {
    let diagnostics = check_page("a.md", "---\ntitle: Fine\n\ndraft: [yes, no]\n---\nBody", &site(&[]));
    let error = diagnostics.iter().find(|d| d.path.as_deref() == Some("draft")).unwrap();
    assert_eq!(error.severity, Severity::Error);
    assert_eq!(error.line, Some(4));
}

#[test]
fn malformed_yaml_reports_one_error_at_the_parser_line() {
    let text = "---\ntitle: Fine\ndraft: [\n---\nBody";
    let diagnostics = check_page("a.md", text, &site(&[]));
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Error);
    let span = frontmatter::frontmatter_span(text).unwrap();
    let error = serde_yaml::from_str::<std::collections::HashMap<String, serde_yaml::Value>>(
        &text[span.fields],
    ).unwrap_err();
    assert_eq!(diagnostics[0].line, Some(error.location().unwrap().line() + 1));
}

#[test]
fn quoted_keys_and_parser_locations_are_not_confused_by_key_text() {
    let mut inputs = site(&[]);
    let field = inputs.schema.frontmatter.fields["draft"].clone();
    inputs.schema.frontmatter.fields.insert("plugin:flag".into(), field);
    let diagnostics = check_page("a.md", "---\ntitle: Fine\n'plugin:flag': [bad]\n---", &inputs);
    assert!(diagnostics.iter().any(|d| d.path.as_deref() == Some("plugin:flag") && d.line == Some(3)));

    let text = "---\nline 18446744073709551615: {a: x, a: y}\n---";
    let diagnostics = check_page("a.md", text, &inputs);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert!(diagnostics[0].line.is_some_and(|line| (2..=4).contains(&line)));
}

#[test]
fn unknown_shortcode_warns_at_each_opening_fence() {
    let diagnostics = check_page("a.md", "---\ntitle: Fine\n---\n\n:::gallry\nFirst\n:::\n\n:::gallry\nSecond\n:::", &site(&[]));
    let unknown: Vec<_> = diagnostics.iter().filter(|d| d.message.contains("unknown shortcode")).collect();
    assert_eq!(unknown.len(), 2);
    assert_eq!(unknown[0].severity, Severity::Warning);
    assert_eq!(unknown[0].line, Some(5));
    assert_eq!(unknown[1].line, Some(9));
}

#[test]
fn builtin_and_schema_shortcodes_are_known() {
    let mut inputs = site(&[]);
    inputs.schema.shortcodes.as_mut().unwrap().definitions.insert("notice".into(), crate::schema::ShortcodeDefinition {
        name: "notice".into(), has_content: true, params: Vec::new(),
    });
    let text = "---\ntitle: Fine\n---\n:::grid 2\nLeft\n+++\nRight\n:::\n\n:::notice\nWords\n:::";
    let diagnostics = check_page("a.md", text, &inputs);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(check_page("a.md", text, &site(&[])).iter().any(|d| d.message.contains("notice")));
}

#[test]
fn syntax_warnings_keep_whole_file_lines() {
    let text = "---\ntitle: Fine\n---\n:::grid 2\nLeft\n---\nRight\n---\nLast\n:::\n\n:::gallery {image=a.jpg|cover}\n:::\n\n:::grid 2\nUnclosed";
    let diagnostics = check_page("a.md", text, &site(&[]));
    let hints: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Hint).map(|d| d.line).collect();
    assert_eq!(hints, vec![Some(6), Some(8)]);
    assert!(diagnostics.iter().any(|d| d.line == Some(12) && d.message.contains("attributes")));
    assert!(diagnostics.iter().any(|d| d.line == Some(15) && d.message.contains("never closed")));
}

#[test]
fn nested_unknown_shortcode_has_its_own_line() {
    let diagnostics = check_page("a.md", ":::grid 1\n:::unknown\nWords\n:::\n:::", &site(&[]));
    assert!(diagnostics.iter().any(|d| d.message.contains("unknown shortcode") && d.line == Some(2)));
}

#[test]
fn broken_wikilinks_embeds_and_markdown_links_warn() {
    let text = "---\ntitle: Fine\n---\n[[missing]]\n![[missing-note]]\n[Missing](missing.md#part)\n[[b]]\n![[b]]\n[Found](b.md)";
    let diagnostics = check_page("notes/a.md", text, &site(&["notes/b.md"]));
    assert_eq!(diagnostics.len(), 3, "{diagnostics:?}");
    for (diagnostic, line) in diagnostics.iter().zip([4, 5, 6]) {
        assert_eq!(diagnostic.severity, Severity::Warning);
        assert_eq!(diagnostic.line, Some(line));
    }
}

#[test]
fn external_anchor_and_inert_links_are_not_checked() {
    let text = "---\ntitle: Fine\n---\n[Web](https://example.com/no.md) [Mail](mailto:a@example.com) [Anchor](#part) [[#part]]\n`[[missing]]`\n\n```\n[[missing]]\n:::unknown\n```\n<!-- [[missing]] -->";
    let diagnostics = check_page("a.md", text, &site(&[]));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn renders_wikilinks_as_pretty_site_paths() {
    let inputs = site(&["a.md", "b.md", "notes/index.md"]);
    let page = render_page("a.md", "[[b]] [[notes/index.md]]", &inputs);
    assert!(page.body_html.contains("href=\"b/\""), "{}", page.body_html);
    assert!(page.body_html.contains("href=\"notes/\""), "{}", page.body_html);
    let without = render_page("a.md", "[[b]]", &site(&[]));
    assert!(!without.body_html.contains("href=\"b/\""));
}

#[test]
fn markdown_targets_resolve_from_the_source_folder() {
    let inputs = site(&["notes/b.md", "notes/index.md", "index.md"]);
    let page = render_page("notes/a.md", "[B](b.md#part) [Notes](index.md) [Home](../index.md)", &inputs);
    assert!(page.body_html.contains("href=\"notes/b/#part\""), "{}", page.body_html);
    assert!(page.body_html.contains("href=\"notes/\""), "{}", page.body_html);
    assert!(page.body_html.contains("href=\"./\""), "{}", page.body_html);
}

#[test]
fn images_use_site_relative_paths() {
    let inputs = site(&["img/x.jpg"]);
    let page = render_page("a.md", "![](img/x.jpg)", &inputs);
    assert!(page.body_html.contains("src=\"/img/x.jpg\""), "{}", page.body_html);
    let nested = render_page("notes/a.md", "![](../img/x.jpg)", &inputs);
    assert!(nested.body_html.contains("src=\"/img/x.jpg\""), "{}", nested.body_html);
}

#[test]
fn title_is_frontmatter_and_frontmatter_is_not_body() {
    let page = render_page("a.md", "---\ntitle: A title\ndescription: Hidden metadata\n---\nVisible body", &site(&[]));
    assert_eq!(page.title, "A title");
    assert!(page.body_html.contains("Visible body"));
    assert!(!page.body_html.contains("Hidden metadata"));
    assert!(!page.body_html.contains("A title"));
    assert_eq!(render_page("a.md", "# Heading", &site(&[])).title, "a");
    let malformed = render_page("a.md", "---\ntitle: [\n---\nVisible body", &site(&[]));
    assert_eq!(malformed.title, "a");
    assert!(!malformed.body_html.contains("title:"));
    assert!(malformed.body_html.contains("Visible body"));
}

#[test]
fn simplified_frontmatter_and_crlf_keep_their_lines() {
    let text = "\u{feff}title: Example\r\ndraft: wrong\r\n---\r\n[[missing]]";
    let diagnostics = check_page("a.md", text, &site(&[]));
    assert!(diagnostics.iter().any(|d| d.path.as_deref() == Some("draft") && d.line == Some(2)));
    assert!(diagnostics.iter().any(|d| d.message.contains("missing") && d.line == Some(4)));
    let page = render_page("a.md", text, &site(&[]));
    assert_eq!(page.title, "Example");
    assert!(!page.body_html.contains("draft:"));
}

#[test]
fn preview_uses_the_published_article_wrapper() {
    let page = RenderedPage { title: "A <title>".into(), title_outcome: TitleOutcome::Visible, diagnostics: Vec::new(), body_html: "<p>Body</p>".into() };
    let html = preview_document(&page, "zh-Hant", "article { color: red; }", "https://example.com/files/");
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("<html lang=\"zh-Hant\">"));
    assert!(html.contains("<base href=\"https://example.com/files/\">"));
    assert!(html.contains("<style>article { color: red; }</style>"));
    assert!(html.contains("<article class=\"container\"><h1 class=\"moss-article-title\">A &lt;title&gt;</h1>\n<p>Body</p></article>"));
    let escaped = preview_document(&page, "en\"", "", "files/?a=1&b=2\"");
    assert!(escaped.contains("lang=\"en&quot;\""));
    assert!(escaped.contains("href=\"files/?a=1&amp;b=2&quot;\""));
}

#[test]
fn standalone_flags_enable_math_and_hard_breaks_without_source_lines() {
    let page = render_page("a.md", "First\nSecond\n\n$x+y$\n\n```mermaid\ngraph TD\nA-->B\n```", &site(&[]));
    assert!(page.body_html.contains("<br"));
    assert!(page.body_html.contains("moss-math"));
    assert!(page.body_html.contains("language-mermaid"));
    assert!(!page.body_html.contains("data-source-line"));
}

#[test]
fn production_url_classification_preserves_url_kinds() {
    use ast::UrlKind;
    for (raw, kind) in [("mailto:a@example.com", UrlKind::Mailto), ("tel:123", UrlKind::Tel), ("#part", UrlKind::Anchor), ("https://example.com", UrlKind::External), ("//example.com", UrlKind::External), ("data:image/png,x", UrlKind::External)] {
        let url = ast::classify_url(raw, &|_| panic!("non-local URL reached resolver"));
        assert_eq!(url, Url::Resolved(ast::ResolvedUrl::new(raw, kind)));
    }
    for (mapped, href, kind) in [("wikilink:b/", "b/", UrlKind::Wikilink), ("moss-newtab:img/x.jpg", "img/x.jpg", UrlKind::AssetNewtab), ("b/", "b/", UrlKind::Internal)] {
        assert_eq!(ast::classify_url("b", &|_| mapped.into()), Url::Resolved(ast::ResolvedUrl::new(href, kind)));
    }
}

proptest! {
    #[test]
    fn page_functions_never_panic(text in any::<String>(), source in any::<String>(), files in prop::collection::vec(any::<String>(), 0..5)) {
        let mut inputs = site(&[]);
        inputs.files = files;
        let _ = check_page(&source, &text, &inputs);
        let _ = render_page(&source, &text, &inputs);
    }
}

#[test]
fn render_prepares_source_and_reports_preparation_and_parser_warnings() {
    let page = render_page("a.md", "Keep {--deleted--} %%comment%% text.\n\n:::unknown\nWords\n:::\n\n%% unpaired\n<!-- unclosed", &site(&[]));
    assert!(page.body_html.contains("Keep"));
    assert!(!page.body_html.contains("deleted"));
    assert!(!page.body_html.contains("comment%%"));
    assert!(page.diagnostics.iter().any(|d| d.message.contains("no closing `%%`")));
    assert!(page.diagnostics.iter().any(|d| d.message.contains("unterminated HTML comment")));
    assert!(page.diagnostics.iter().any(|d| d.message.contains("unknown shortcode")));
}

#[test]
fn transclusions_use_supplied_notes_and_missing_notes_are_visible() {
    let mut inputs = site(&["a.md", "b.md"]);
    assert_eq!(embedded_notes("a.md", "![[b]]", &inputs), vec!["b.md"]);
    let missing = render_page("a.md", "![[b]]", &inputs);
    assert!(missing.diagnostics.iter().any(|d| d.severity == Severity::Warning && d.message.contains("b.md")));
    assert!(missing.body_html.contains("<div class=\"moss-embed-missing\">Embed unavailable: b.md</div>"), "{}", missing.body_html);
    inputs.notes.insert("b.md".into(), "Text from the embedded note.".into());
    let supplied = render_page("a.md", "![[b]]", &inputs);
    assert!(supplied.body_html.contains("Text from the embedded note."));
    assert!(supplied.diagnostics.is_empty(), "{:?}", supplied.diagnostics);
    assert!(!supplied.body_html.contains("moss-embed-missing"));
}

#[test]
fn embedded_note_discovery_resolves_sections_deduplicates_and_skips_inert_media_and_links() {
    let inputs = site(&["notes/b.md", "img.png"]);
    assert_eq!(embedded_notes("notes/a.md", "![[b#Section]]\n![[b|Alias]]\n[[b]] ![[img.png]]\n`![[b]]`\n{--![[deleted]]--}", &inputs), vec!["notes/b.md"]);
    assert!(embedded_notes("notes/a.md", "[[b]] ![[img.png]]\n`![[b]]`", &inputs).is_empty());
}

#[test]
fn rendering_with_exactly_the_discovered_notes_never_misses_one() {
    let texts = ["![[b]]", "{++![[b]]++}", "{--![[b]]--}", "%% ![[b]] %%", "%%\n![[b]]\n%%", "> ![[b]]", "- ![[b#Part]]"];
    for text in texts {
        let mut inputs = site(&["a.md", "b.md"]);
        for path in embedded_notes("a.md", text, &inputs) {
            inputs.notes.insert(path, "# Part\n\nNote text.".into());
        }
        let page = render_page("a.md", text, &inputs);
        // Rendering asks for a note discovery did not name: a missing-note diagnostic.
        assert!(!page.diagnostics.iter().any(|d| d.message.contains("b.md")), "{text}: {:?}", page.diagnostics);
        assert!(!page.body_html.contains("moss-embed-missing"), "{text}: {}", page.body_html);
    }
}

#[test]
fn preview_title_uses_the_publication_heading_decision() {
    let inputs = site(&[]);
    let page = render_page("a.md", "---\ntitle: Custom\n---\n# Custom\n\nBody", &inputs);
    let html = preview_document(&page, "en", "", "");
    assert_eq!(page.title_outcome, TitleOutcome::None);
    assert_eq!(html.matches("<h1").count(), 1);
    let page = render_page("my-note.md", "Body", &inputs);
    assert_eq!(page.title, "my note");
    assert_eq!(page.title_outcome, TitleOutcome::Visible);
    assert!(preview_document(&page, "en", "", "").contains("<h1 class=\"moss-article-title\">my note</h1>"));
    let index = render_page("notes/index.md", "Body", &inputs);
    assert_eq!(index.title, "notes");
    assert_eq!(index.title_outcome, TitleOutcome::None);
    let hero = render_page("a.md", ":::hero\nOverlay words\n:::", &inputs);
    assert_eq!(hero.title_outcome, TitleOutcome::Hidden);
    assert!(preview_document(&hero, "en", "", "").contains("moss-article-title visually-hidden"));
}

#[test]
fn duplicate_nested_unknown_shortcodes_have_distinct_lines() {
    let diagnostics = check_page("a.md", ":::grid 1\n:::unknown\nFirst\n:::\n:::unknown\nSecond\n:::\n:::", &site(&[]));
    let lines: Vec<_> = diagnostics.iter().filter(|d| d.message.contains("unknown shortcode")).map(|d| d.line).collect();
    assert_eq!(lines, vec![Some(2), Some(5)]);
}

#[test]
fn render_dispatches_typed_embeds_and_reports_ast_resolution_diagnostics() {
    let inputs = site(&["clip.mp4"]);
    let page = render_page("a.md", "![[clip.mp4]]\n\n[Missing](missing.md)", &inputs);
    assert!(page.body_html.contains("<video"), "{}", page.body_html);
    assert!(page.diagnostics.iter().any(|d| d.message.contains("missing.md")), "{:?}", page.diagnostics);
}
