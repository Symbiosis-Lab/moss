use super::*;
use crate::ast::parse;

/// Section word counts (in half-words) measured on 24 real sites, every one agreeing with the rule; only counts, never text, are kept.
const MEASURED_SHAPES: &[(&[u32], bool)] = &[
    (&[1171, 5426, 2790, 3200, 2738, 1031, 2366, 1515, 2246, 1008, 2096, 1341, 1635, 1247, 3395, 2337, 2182, 1624, 1520, 2230, 1372, 337, 0], true),
    (&[379, 687, 1338, 483, 1213, 1017, 1239, 934], true),
    (&[134, 334, 116, 528, 172, 150, 146, 124, 178, 140, 166, 164, 158, 192, 168], true),
    (&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], false),
    (&[0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 101, 0, 9, 115, 14], false),
    (&[0, 450, 42, 42, 52, 0, 306, 38], true),
    (&[0, 0, 40, 0, 0, 0, 0], false),
    (&[510, 60, 376, 136, 190, 18, 32, 356, 1056, 1110, 178, 174, 476, 158, 340, 280, 540, 0, 150, 0, 0, 104, 316, 844, 558, 86, 116, 34, 38], true),
    (&[34, 0, 0, 0], false),
    (&[8, 12, 0, 0], false),
    (&[0, 0, 0, 0, 0, 0, 0, 0], false),
    (&[94, 46, 640, 66, 540, 354, 482, 496, 492, 1126, 166, 70, 92, 42, 42, 18, 40], true),
    (&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], false),
    (&[0, 4424], false),
    (&[0, 0, 0], false),
    (&[0, 0, 0, 0, 0, 0, 0], false),
    (&[0, 262, 314, 310, 280, 236, 134, 1238, 226, 216, 224, 224, 162, 226, 340, 348, 210, 1374, 846, 1000, 68, 898, 1642, 4916, 1494, 1092, 984, 278, 590, 420, 1482, 748, 3110, 0, 94, 184], true),
    (&[], false),
    (&[0, 274, 2, 2, 572], true),
    (&[], false),
    (&[58, 170, 98], false),
    (&[28, 42], false),
    (&[354, 510, 699, 6666], true),
    (&[78, 0, 0, 0, 60, 0, 34, 0], false),
];

#[test]
fn the_rule_agrees_with_every_measured_shape() {
    for (i, (sections, expected)) in MEASURED_SHAPES.iter().enumerate() {
        assert_eq!(shows_heading_anchors(sections), *expected, "shape {} {:?}", i + 1, sections);
    }
}

#[test]
fn threshold_is_eighty_words_in_two_sections() {
    assert!(shows_heading_anchors(&[160, 160]));
    assert!(!shows_heading_anchors(&[160, 159]));
    assert!(!shows_heading_anchors(&[4000]));
    assert!(!shows_heading_anchors(&[]));
}

fn words(n: usize) -> String {
    vec!["word"; n].join(" ")
}

fn wants(md: &str) -> bool {
    wants_heading_anchors(&parse(md).blocks)
}

#[test]
fn two_eighty_word_sections_say_yes_and_seventy_nine_says_no() {
    assert!(wants(&format!("## A\n\n{}\n\n## B\n\n{}\n", words(80), words(80))));
    assert!(!wants(&format!("## A\n\n{}\n\n## B\n\n{}\n", words(80), words(79))));
}

#[test]
fn a_page_without_subheadings_says_no() {
    assert!(!wants(&format!("{}\n\n{}\n", words(500), words(500))));
    assert!(!wants(&format!("# Title\n\n{}\n\n{}\n", words(500), words(500))));
}

#[test]
fn text_before_the_first_subheading_counts_nothing() {
    assert!(!wants(&format!("{}\n\n## A\n\n{}\n\n## B\n\n{}\n", words(500), words(80), words(79))));
}

#[test]
fn h3_and_h4_start_sections_and_h5_ends_one() {
    assert!(wants(&format!("### A\n\n{}\n\n#### B\n\n{}\n", words(80), words(80))));
    assert!(!wants(&format!("## A\n\n{}\n\n##### Aside\n\n{}\n\n## B\n\n{}\n", words(40), words(200), words(80))));
}

#[test]
fn a_section_spans_blocks_up_to_the_next_heading() {
    assert!(wants(&format!("## A\n\n{}\n\n{}\n\n## B\n\n{}\n", words(40), words(40), words(80))));
}

#[test]
fn cjk_characters_count_half_a_word() {
    let han = "字".repeat(160);
    assert!(wants(&format!("## A\n\n{han}\n\n## B\n\n{han}\n")));
    let short = "字".repeat(159);
    assert!(!wants(&format!("## A\n\n{han}\n\n## B\n\n{short}\n")));
    // Kana and Hangul count the same.
    assert_eq!(half_words("かたカナ한글"), 6);
    assert_eq!(half_words("one, two-three 4"), 8);
}

#[test]
fn only_paragraphs_and_quotes_count() {
    let w = words(200);
    let not_text = [
        format!("- {w}\n- {w}"),
        format!("| a | b |\n|---|---|\n| {w} | x |"),
        format!(":::grid\n{w}\n:::"),
        "![".to_string() + &w + "](x.png)",
        format!("```\n{w}\n```"),
    ];
    for block in not_text {
        assert!(!wants(&format!("## A\n\n{block}\n\n## B\n\n{block}\n")), "{block}");
    }
    let quote = format!("> {w}");
    assert!(wants(&format!("## A\n\n{quote}\n\n## B\n\n{quote}\n")));
    let link = format!("[{w}](https://example.com)");
    assert!(wants(&format!("## A\n\n{link}\n\n## B\n\n{link}\n")));
}
