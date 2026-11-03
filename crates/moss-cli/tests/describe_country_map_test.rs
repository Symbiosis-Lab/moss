//! `moss describe --json` lists the country-map key and its colour tokens, so
//! an agent finds them without reading the guide first.

use std::process::Command;

fn describe() -> serde_json::Value {
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli")).args(["describe", "--json"]).output().expect("run moss-cli describe");
    assert!(out.status.success(), "{:?}", out.status);
    serde_json::from_slice(&out.stdout).expect("describe --json is JSON")
}

#[test]
fn country_map_is_a_frontmatter_key() {
    let d = describe();
    let key = d["frontmatter"].as_array().unwrap().iter().find(|f| f["name"] == "country_map").expect("country_map listed under .frontmatter");
    assert_eq!(key["type"], "object");
    assert!(key["description"].as_str().unwrap().contains("levels"));
}

#[test]
fn the_colour_tokens_are_listed_as_custom_properties() {
    let d = describe();
    let names: Vec<&str> = d["custom_properties"].as_array().unwrap().iter().filter_map(|p| p["name"].as_str()).collect();
    for token in ["--moss-country-land", "--moss-country-fill-low", "--moss-country-fill-high", "--moss-country-fill-1", "--moss-country-graticule", "--moss-country-chosen"] {
        assert!(names.contains(&token), "{token} missing from describe's custom properties");
    }
}
