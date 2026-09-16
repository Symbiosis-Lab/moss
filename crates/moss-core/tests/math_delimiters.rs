//! Golden-vector test for pulldown-cmark's `$` open/close rules.
//!
//! `tests/fixtures/math-delimiters.vectors.json` is the cross-language
//! contract for where math starts and stops. This test holds the Rust side
//! of it; the future CM6 editor grammar (design H20) reads the same file so
//! the editor cannot highlight a span the build would not parse, or vice
//! versa. Adding a vector is how you record a `$`-behavior question once,
//! for both languages.
//!
//! Every expectation in the fixture was derived by RUNNING the parser.
//! If a vector fails after a pulldown-cmark bump, the upgrade changed
//! delimiter semantics — re-measure, update the fixture and the `note`,
//! and check the CM6 grammar against the new file.

use pulldown_cmark::{Event, Parser};
use serde::Deserialize;

#[derive(Deserialize)]
struct Vectors {
    vectors: Vec<Vector>,
}

#[derive(Deserialize)]
struct Vector {
    input: String,
    expect_math_spans: Vec<Span>,
    note: String,
}

#[derive(Deserialize, PartialEq, Debug)]
struct Span {
    mode: String,
    tex: String,
}

fn math_spans(input: &str) -> Vec<Span> {
    Parser::new_ext(input, moss_core::ast::parser_options(true))
        .filter_map(|ev| match ev {
            Event::InlineMath(tex) => Some(Span {
                mode: "inline".into(),
                tex: tex.to_string(),
            }),
            Event::DisplayMath(tex) => Some(Span {
                mode: "display".into(),
                tex: tex.to_string(),
            }),
            _ => None,
        })
        .collect()
}

#[test]
fn parser_agrees_with_every_golden_vector() {
    let raw = include_str!("fixtures/math-delimiters.vectors.json");
    let fixture: Vectors = serde_json::from_str(raw).expect("vectors fixture must parse");

    assert!(
        fixture.vectors.len() >= 14,
        "vectors were removed — this fixture is a contract, extend it rather than shrink it"
    );

    for v in &fixture.vectors {
        let actual = math_spans(&v.input);
        assert_eq!(
            actual, v.expect_math_spans,
            "delimiter behavior changed for {:?}\n  note: {}",
            v.input, v.note
        );
    }
}

/// The vectors describe *events*; this asserts the AST honors them end to
/// end, so a vector can never pass while the equation is dropped downstream.
#[test]
fn every_vector_with_math_survives_to_rendered_html() {
    let raw = include_str!("fixtures/math-delimiters.vectors.json");
    let fixture: Vectors = serde_json::from_str(raw).expect("vectors fixture must parse");

    let config = moss_core::ast::ParseConfig {
        math: true,
        ..Default::default()
    };

    for v in &fixture.vectors {
        let doc = moss_core::ast::parse_with_config(&v.input, &config);
        let html = moss_core::ast::render_document(&doc, &moss_core::ast::DefaultHooks::new());

        assert_eq!(
            html.matches(r#"class="moss-math""#).count(),
            v.expect_math_spans.len(),
            "wrong number of math spans in HTML for {:?}\n  html: {html}",
            v.input
        );

        for span in &v.expect_math_spans {
            // The TeX is escaped on the way in, so compare against the
            // escaped form.
            let escaped = span
                .tex
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            assert!(
                html.contains(&escaped),
                "TeX {:?} lost between events and HTML for input {:?}\n  html: {html}",
                span.tex,
                v.input
            );
        }
    }
}
