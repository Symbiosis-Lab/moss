//! A card's title is never cut off with an ellipsis, and its description keeps
//! three lines, in the stylesheet a CLI build ships.

#[path = "support/site.rs"]
mod site;

use site::Site;
use std::fs;

/// The declarations of the first shipped rule whose selector is exactly `selector`.
fn shipped_rule(css: &str, selector: &str) -> String {
    let open = format!("{selector}{{");
    let start = css
        .match_indices(&open)
        .map(|(i, _)| i)
        .find(|&i| i == 0 || matches!(css.as_bytes()[i - 1], b'}' | b'\n'))
        .unwrap_or_else(|| panic!("shipped css has no rule for {selector}"));
    let body = &css[start + open.len()..];
    body[..body.find('}').unwrap()].to_string()
}

fn shipped_css() -> String {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Hall\n---\n\nWelcome.\n"),
        (
            "shows/return.md",
            "---\ntitle: \"Return of the Example: A Very Long Subtitle That Needs Room\"\ndescription: A long enough description to run past several lines on a narrow phone screen.\n---\n\nBody.\n",
        ),
    ]);
    let dir = site.output("_moss");
    let name = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .find(|n| n.starts_with("style.") && n.ends_with(".css"))
        .expect("the build ships a stylesheet");
    fs::read_to_string(dir.join(name)).unwrap()
}

#[test]
fn list_card_titles_wrap_instead_of_ending_in_an_ellipsis() {
    let css = shipped_css();
    let rule = shipped_rule(&css, ".moss-cards[data-layout=\"list\"] .moss-card-title");
    assert!(!rule.contains("text-overflow:ellipsis"), "title still ellipsizes: {rule}");
    assert!(!rule.contains("white-space:nowrap"), "title still on one line: {rule}");
    assert!(rule.contains("overflow-wrap:anywhere"), "a long unbroken word must still wrap: {rule}");
}

#[test]
fn card_descriptions_clamp_to_three_lines() {
    let css = shipped_css();
    let rule = shipped_rule(&css, ".moss-card-description");
    assert!(rule.contains("-webkit-line-clamp:3"), "{rule}");
    assert!(rule.contains("line-clamp:3"), "{rule}");
    assert!(rule.contains("max-block-size:calc(1.5em * 3)"), "{rule}");
}
