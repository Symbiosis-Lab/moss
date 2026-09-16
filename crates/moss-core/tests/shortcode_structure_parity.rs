//! Enforced shortcode-structure parity gate — BUILD side.
//!
//! Runs the shared corpus (`tests/fixtures/shortcode_structure_corpus.json`)
//! through the authoritative build parser (`moss_core::ast::parse`) and asserts
//! the extracted block structure — names + nesting + grid cell counts — matches
//! the corpus.
//!
//! Its TS twin, `frontend/app/editor/__tests__/cm-shortcode-corpus.test.ts`,
//! asserts the EDITOR parser (`collectShortcodeBlocks`) matches the SAME corpus.
//! Two gates, one source of truth: the editor live-view parse cannot silently
//! drift from the build (the `+++` divider / nested `::::buttons` rendering bugs)
//! without turning one of these red. See docs/reference/shortcode-grammar.md.

use moss_core::ast::{parse, Block, Shortcode};
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct ExpectedBlock {
    name: String,
    /// Cell count — asserted for `grid` only (buttons store items, not cells, on
    /// the build side); `null` for every other block.
    #[serde(rename = "gridCells")]
    grid_cells: Option<usize>,
    children: Vec<ExpectedBlock>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    input: String,
    blocks: Vec<ExpectedBlock>,
}

#[derive(Debug, Deserialize)]
struct Corpus {
    cases: Vec<Case>,
}

fn simple(name: &str) -> ExpectedBlock {
    ExpectedBlock { name: name.into(), grid_cells: None, children: vec![] }
}

/// Structural summary (name + grid cell count + nested blocks) of a parsed block
/// list, in the SAME shape as the corpus. Nested shortcodes inside a grid cell
/// (`::::buttons` in `:::grid`) surface as `children` (the build extracts them
/// recursively into `GridShortcode.cells`).
fn summarize(blocks: &[Block]) -> Vec<ExpectedBlock> {
    let mut out = Vec::new();
    for b in blocks {
        let Block::Shortcode(sc) = b else { continue };
        let node = match sc {
            Shortcode::Grid(g) => ExpectedBlock {
                name: "grid".into(),
                grid_cells: Some(g.cells.len()),
                children: g.cells.iter().flat_map(|cell| summarize(cell)).collect(),
            },
            Shortcode::Buttons(_) => simple("buttons"),
            Shortcode::Hero(_) => simple("hero"),
            Shortcode::Gallery(_) => simple("gallery"),
            Shortcode::Subscribe(_) => simple("subscribe"),
            Shortcode::Recent(_) => simple("recent"),
            Shortcode::Apply(_) => simple("apply"),
        };
        out.push(node);
    }
    out
}

#[test]
fn build_parse_matches_shortcode_structure_corpus() {
    let corpus: Corpus =
        serde_json::from_str(include_str!("fixtures/shortcode_structure_corpus.json"))
            .expect("corpus JSON parses");
    assert!(!corpus.cases.is_empty(), "corpus must not be empty");
    for case in &corpus.cases {
        let doc = parse(&case.input);
        let got = summarize(&doc.blocks);
        assert_eq!(
            got, case.blocks,
            "BUILD parse structure diverged from corpus for case '{}'\ninput:\n{}",
            case.name, case.input
        );
    }
}
