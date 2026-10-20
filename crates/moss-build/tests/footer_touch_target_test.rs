//! A link in a footer paragraph has a 44px hit area on a phone, whatever its
//! label length, without changing its painted size.

#[path = "support/site.rs"]
mod site;

use site::Site;

/// The declarations of the built stylesheet's rule that draws a footer
/// anchor's hit area, after the minifier.
fn hit_area_rule(site: &Site) -> String {
    let dir = site.output("_moss");
    let sheet = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|x| x == "css") && p.file_name().unwrap().to_string_lossy().starts_with("style"))
        .expect("a built stylesheet");
    let css = std::fs::read_to_string(sheet).unwrap();
    let at = css.find("footer.container a:where(:not(.moss-colophon a))::after{").expect("the footer hit-area rule ships");
    let body = &css[at..];
    body[..body.find('}').unwrap()].to_string()
}

#[test]
fn a_footer_md_link_row_keeps_a_44px_hit_area_on_both_axes() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Hall\n---\n\nWelcome.\n"),
        ("footer.md", "Follow: [X](https://example.test/x) · [Email](mailto:hi@example.test)\n"),
    ]);
    let html = site.html("index.html");
    assert!(html.contains("<footer") && html.contains(">X</a>"), "footer.md renders as plain anchors: {html}");
    let rule = hit_area_rule(&site);
    assert!(rule.contains("inset-block:-16px"), "block axis: {rule}");
    assert!(rule.contains("min-inline-size:44px"), "a short label still reaches 44px wide: {rule}");
    assert!(rule.contains("margin-inline:auto"), "the widened area stays centred on the link: {rule}");
}
