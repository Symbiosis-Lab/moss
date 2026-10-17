//! Golden-vector test for the image-options words.
//!
//! `tests/fixtures/position.vectors.json` is the cross-language contract for
//! where an image is cropped to, how it is fitted and which side it floats on.
//! This side proves the vectors are what the real parser answers; a mirror of
//! the grammar (the editor's) loads the same file and must give the same
//! answers.
//!
//! A failure after a `media.rs` grammar change is real: re-measure by running
//! the parser, then update the vectors and every mirror.

use moss_core::media::{parse_media_attrs, AlignSide, Fit, Position};
use serde_json::Value;

fn load() -> Value {
    let raw = include_str!("fixtures/position.vectors.json");
    serde_json::from_str(raw).expect("position.vectors.json is valid JSON")
}

fn rows(v: &Value, key: &str) -> Vec<Value> {
    v[key].as_array().unwrap_or_else(|| panic!("{key} is an array")).clone()
}

fn position_json(p: Option<Position>) -> Value {
    match p {
        None => Value::Null,
        Some(Position::Focus(x, y)) => serde_json::json!({ "focus": [x, y] }),
        Some(Position::Center) => "center".into(),
        Some(Position::Left) => "left".into(),
        Some(Position::Right) => "right".into(),
        Some(Position::Top) => "top".into(),
        Some(Position::Bottom) => "bottom".into(),
        Some(Position::TopLeft) => "top-left".into(),
        Some(Position::TopRight) => "top-right".into(),
        Some(Position::BottomLeft) => "bottom-left".into(),
        Some(Position::BottomRight) => "bottom-right".into(),
    }
}

fn fit_json(f: Option<Fit>) -> Value {
    f.map_or(Value::Null, |f| f.to_css_value().into())
}

fn align_json(a: Option<AlignSide>) -> Value {
    a.map_or(Value::Null, |a| match a {
        AlignSide::Left => "left".into(),
        AlignSide::Right => "right".into(),
    })
}

#[test]
fn every_position_word_parses_to_its_vector() {
    let v = load();
    let rows = rows(&v, "position_words");
    assert!(rows.len() > 30, "an empty or truncated fixture would pass vacuously");
    for r in rows {
        let input = r["input"].as_str().unwrap();
        assert_eq!(
            position_json(Position::from_keyword(input)),
            r["position"],
            "{}: `{input}`",
            r["note"]
        );
    }
}

#[test]
fn every_frame_segment_parses_to_its_vector() {
    let v = load();
    let rows = rows(&v, "frame_segments");
    assert!(rows.len() > 20, "an empty or truncated fixture would pass vacuously");
    for r in rows {
        let input = r["input"].as_str().unwrap();
        let attrs = parse_media_attrs(input);
        let note = &r["note"];
        assert_eq!(fit_json(attrs.fit), r["fit"], "{note}: fit of `{input}`");
        assert_eq!(position_json(attrs.position), r["position"], "{note}: position of `{input}`");
        assert_eq!(align_json(attrs.align), r["align"], "{note}: align of `{input}`");
    }
}

#[test]
fn the_package_copy_matches() {
    let copy = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../packages/moss-syntax/fixtures/position.vectors.json"
    );
    if let Ok(text) = std::fs::read_to_string(copy) {
        assert!(
            text == include_str!("fixtures/position.vectors.json"),
            "packages/moss-syntax/fixtures/position.vectors.json differs from the crate fixture; copy it over"
        );
    }
}
