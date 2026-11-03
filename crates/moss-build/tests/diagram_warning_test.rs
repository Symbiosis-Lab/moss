//! A diagram that cannot be drawn is a build problem: it warns with its page
//! and fails `--strict`, while the page keeps its source.
//!
//! Its own test binary: `log::set_logger` is process-global and one-shot.

use moss_build::build::cli_output::take_cli_problems;
use moss_build::build::markdown::{process_markdown_file, PageContext, SiteMarkdown};
use std::collections::HashMap;
use std::sync::Mutex;

struct Capture(Mutex<Vec<String>>);

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        self.0.lock().unwrap().push(record.args().to_string());
    }
    fn flush(&self) {}
}

static CAPTURE: Capture = Capture(Mutex::new(Vec::new()));

fn build(md: &str) -> (String, Vec<String>) {
    CAPTURE.0.lock().unwrap().clear();
    let map = HashMap::new();
    let page = process_markdown_file(
        "notes/flow.md",
        md,
        "site",
        &map,
        false,
        moss_build::i18n::Language::En,
        None,
        SiteMarkdown::default(),
        None,
        None,
        None,
        PageContext::default(),
    )
    .expect("parses");
    (page.html_content, CAPTURE.0.lock().unwrap().clone())
}

#[test]
fn a_broken_diagram_warns_with_its_page_and_counts_for_strict() {
    log::set_logger(&CAPTURE).unwrap();
    log::set_max_level(log::LevelFilter::Warn);
    let _ = take_cli_problems();

    let (html, warnings) = build("# Flow\n\n```mermaid\nflowchart TD\n  A --> B\n  B -->\n```\n");
    assert!(html.contains("moss-diagram-error") && html.contains("language-mermaid"), "{html}");
    assert!(
        warnings.iter().any(|w| w == "[notes/flow.md] diagram “flowchart TD” not drawn: line 3: syntax error near “B -->”"),
        "{warnings:?}"
    );
    assert_eq!(take_cli_problems(), 1);

    let (html, warnings) = build("```mermaid\nflowchart TD\n  A --> B\n```\n");
    assert!(html.contains("<svg") && !html.contains("moss-diagram-error"), "{html}");
    assert!(warnings.iter().all(|w| !w.contains("diagram")), "{warnings:?}");
    assert_eq!(take_cli_problems(), 0);
}

/// A diagram's note link is a dependency edge, as a prose `[[link]]` is, so a
/// rename of the note re-renders the page that draws the link.
#[test]
fn a_diagram_note_link_is_a_dependency_of_its_page() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("test-tmp");
    std::fs::create_dir_all(&base).unwrap();
    let dir = tempfile::Builder::new().prefix("diagram-edges").tempdir_in(&base).unwrap();
    let flow = "```mermaid\nflowchart LR\n  A[field-notes] --> B\n  class A internal-link;\n```\n";
    std::fs::create_dir_all(dir.path().join("notes")).unwrap();
    std::fs::write(dir.path().join("notes/flow.md"), flow).unwrap();
    std::fs::write(dir.path().join("notes/field-notes.md"), "# Field notes\n").unwrap();
    let scanned = moss_build::build::scan::scan::scan_folder(&dir.path().to_string_lossy()).expect("scan");
    let graph = moss_build::build::scan::scan::build_content_graph(&scanned);

    let map = HashMap::new();
    let page = process_markdown_file(
        "notes/flow.md",
        flow,
        "site",
        &map,
        false,
        moss_build::i18n::Language::En,
        None,
        SiteMarkdown::default(),
        None,
        None,
        Some(&graph),
        PageContext::default(),
    )
    .expect("parses");
    assert!(page.html_content.contains("<a class=\"internal-link\" href=\""), "{}", page.html_content);
    let targets: Vec<&str> = page.outgoing_links.iter().map(|l| l.target_path.as_str()).collect();
    assert!(targets.contains(&"notes/field-notes.md"), "{targets:?}");
}
