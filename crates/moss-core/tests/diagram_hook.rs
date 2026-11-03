//! A fenced `mermaid` block reaches `RenderHooks::render_diagram`, carrying its
//! source, its line and the code block every other renderer would emit.

use std::cell::RefCell;

use moss_core::ast::parser::{parse_with_config, ParseConfig};
use moss_core::ast::{has_diagram_recursive, render_document, RenderHooks};

#[derive(Default)]
struct Recorder {
    calls: RefCell<Vec<(String, String, Option<usize>, String)>>,
}

impl RenderHooks for Recorder {
    fn render_diagram(&self, out: &mut String, info: &str, source: &str, source_line: Option<usize>, fallback_html: &str) {
        self.calls.borrow_mut().push((info.to_string(), source.to_string(), source_line, fallback_html.to_string()));
        out.push_str("<svg data-drawn></svg>");
    }
}

fn lines() -> ParseConfig {
    ParseConfig { emit_source_lines: true, ..Default::default() }
}

#[test]
fn a_mermaid_block_reaches_the_hook_with_its_source_line_and_fallback() {
    let md = "Intro\n\n```mermaid {.wide}\nflowchart LR\n  A --> B\n```\n";
    let hooks = Recorder::default();
    let html = render_document(&parse_with_config(md, &lines()), &hooks);

    let calls = hooks.calls.borrow();
    assert_eq!(calls.len(), 1, "{html}");
    let (info, source, line, fallback) = &calls[0];
    assert_eq!(info, "mermaid {.wide}");
    assert_eq!(source, "flowchart LR\n  A --> B\n");
    assert_eq!(*line, Some(3));
    assert_eq!(
        fallback,
        "<pre tabindex=\"0\" data-source-line=\"3\"><code class=\"language-mermaid {.wide}\">flowchart LR\n  A --&gt; B\n</code></pre>\n"
    );
    assert!(html.contains("<svg data-drawn></svg>") && !html.contains("<pre"), "{html}");
}

#[test]
fn the_info_string_is_read_by_its_first_word() {
    let routed = |md: &str| {
        let hooks = Recorder::default();
        render_document(&parse_with_config(md, &ParseConfig::default()), &hooks);
        let n = hooks.calls.borrow().len();
        n == 1
    };
    assert!(routed("~~~mermaid\npie\n~~~\n"), "tilde fence");
    assert!(routed("```mermaid {.wide}\npie\n```\n"), "attributes after the language");
    assert!(!routed("```mermaidjs\npie\n```\n"));
    assert!(!routed("```Mermaid\npie\n```\n"));
    assert!(!routed("```\npie\n```\n"));
    assert!(!routed("    pie\n"), "indented code has no language");
}

#[test]
fn a_nested_block_reaches_the_hook_and_the_gate() {
    for md in ["- item\n\n  ```mermaid\n  pie\n  ```\n", "> [!note]\n> ```mermaid\n> pie\n> ```\n"] {
        let doc = parse_with_config(md, &ParseConfig::default());
        assert!(has_diagram_recursive(&doc), "{md:?}");
        let hooks = Recorder::default();
        render_document(&doc, &hooks);
        assert_eq!(hooks.calls.borrow().len(), 1, "{md:?}");
    }
    assert!(!has_diagram_recursive(&parse_with_config("```rust\nfn x() {}\n```\n", &ParseConfig::default())));
}
