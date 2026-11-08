//! `::: {accordion}` gives the folded callouts directly inside it one shared
//! `name`, so the browser opens them one at a time.

#[path = "support/site.rs"]
mod site;

use site::Site;

const PAGE: &str = "---\ntitle: Questions\n---\n\n\
::: {accordion}\n\n> [!faq]- One\n> a\n\n> [!faq]- Two\n> b\n\n:::\n\n\
> [!faq]- Loose\n> c\n\n\
::: {accordion}\n\n> [!faq]- Three\n> d\n\n:::\n";

fn names(html: &str) -> Vec<String> {
    html.match_indices("<details class=\"callout\"")
        .map(|(i, _)| {
            let tag = &html[i..i + html[i..].find('>').unwrap()];
            tag.split(" name=\"").nth(1).map(|s| s.split('"').next().unwrap().to_string()).unwrap_or_default()
        })
        .collect()
}

#[test]
fn one_group_shares_a_name_two_groups_have_two_and_a_loose_callout_has_none() {
    let site = Site::new(&[("index.md", PAGE)]);
    let names = names(&site.html("index.html"));
    assert_eq!(names.len(), 4, "{names:?}");
    assert!(!names[0].is_empty() && names[0] == names[1], "{names:?}");
    assert!(names[2].is_empty(), "{names:?}");
    assert!(!names[3].is_empty() && names[3] != names[0], "{names:?}");
}
