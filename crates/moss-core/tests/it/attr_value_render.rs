//! Golden-vector test for how the EDITOR must render a shortcode attr value.
//!
//! `tests/fixtures/attr-value.vectors.json` is the cross-language contract.
//! This side proves the claim that matters: every `rendered` form, fed to the
//! real `parse_attrs` grammar, yields exactly `raw` back. The TS side
//! (`cm-completion-core.test.ts`) proves `renderShortcodeAttrValue` produces
//! that same `rendered` form.
//!
//! So the editor cannot write an `{image=…}` value the build would misparse —
//! which is the whole point, since accepting an asset completion with a CJK or
//! space-bearing filename used to emit an unquoted value that `parse_attrs`
//! rejected outright.
//!
//! A failure here after an `attrs.rs` grammar change is real: re-measure by
//! running the parser, then update BOTH the fixture and
//! `renderShortcodeAttrValue`.

use moss_core::ast::attrs::parse_attrs;
use serde::Deserialize;

#[derive(Deserialize)]
struct Vectors {
    vectors: Vec<Vector>,
}

#[derive(Deserialize)]
struct Vector {
    raw: String,
    rendered: String,
    note: String,
}

fn load() -> Vec<Vector> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/attr-value.vectors.json"
    );
    let text = std::fs::read_to_string(path).expect("fixture readable");
    let v: Vectors = serde_json::from_str(&text).expect("fixture parses");
    v.vectors
}

#[test]
fn every_rendered_value_round_trips_through_the_real_attr_grammar() {
    for v in load() {
        let block = parse_attrs(&format!("{{image={}}}", v.rendered))
            .unwrap_or_else(|e| panic!("{}: parse_attrs rejected `{}`: {e:?}", v.note, v.rendered));
        assert_eq!(
            block.get("image"),
            Some(v.raw.as_str()),
            "{}: `{}` did not round-trip",
            v.note,
            v.rendered
        );
    }
}

#[test]
fn a_bareword_rendering_is_only_used_when_the_grammar_allows_it() {
    // Guards the other direction: if a vector claims an UNQUOTED rendering, the
    // raw value must contain nothing outside `is_bareword`. Without this, a
    // renderer bug that under-quotes could still pass the round-trip test by
    // accident (the grammar would truncate at the first illegal char and the
    // fixture would have been updated to match the truncation).
    for v in load() {
        if v.rendered.starts_with('"') {
            continue;
        }
        assert!(
            v.raw
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '/' | '.' | '-' | '_')),
            "{}: `{}` is rendered bare but is not a bareword",
            v.note,
            v.raw
        );
        assert_eq!(v.raw, v.rendered, "{}: a bare rendering must be verbatim", v.note);
    }
}
