//! A `:::subscribe` block directly under a short paragraph names
//! its email input from that text; with no prompt above, the input keeps its
//! `aria-label` alone.

#[path = "support/site.rs"]
mod site;

use site::Site;

/// The email input tag of the nth form on the page, and the page.
fn email_inputs(html: &str) -> Vec<&str> {
    html.match_indices(r#"<input type="email""#)
        .map(|(at, _)| &html[at..at + html[at..].find('>').unwrap()])
        .collect()
}

#[test]
fn the_email_input_is_labelled_by_the_short_paragraph_above_it() {
    let site = Site::new(&[(
        "index.md",
        "---\ntitle: Home\n---\n\n## Get the letter\n\n:::subscribe\n:::\n\nJoin the weekly letter.\n\n:::subscribe\n:::\n\nA long aside that is not a prompt. Aside. Aside. Aside. Aside. Aside. Aside. Aside. Aside. Aside. Aside. Aside. Aside. Aside.\n\n:::subscribe\n:::\n\n---\n\n:::subscribe\n:::\n",
    )]);
    let html = site.html("index.html");
    let inputs = email_inputs(&html);
    assert_eq!(inputs.len(), 4, "{inputs:?}");

    let prompt = labelled_by(inputs[1]);
    assert!(inputs[1].contains(&format!(r#"id="{prompt}-field""#)));
    assert!(html.contains(&format!(r#"<p id="{prompt}""#)), "no paragraph carries {prompt}");

    for bare in [inputs[0], inputs[2], inputs[3]] {
        assert!(!bare.contains("aria-labelledby"), "{bare}");
        assert!(bare.contains(r#"aria-label="Email address""#), "{bare}");
    }
    assert!(inputs[1].contains(r#"aria-label="Email address""#), "the fallback stays: {}", inputs[1]);
}

fn labelled_by(input: &str) -> &str {
    input.split(r#"aria-labelledby=""#).nth(1).and_then(|r| r.split(' ').next()).expect(input)
}

#[test]
fn a_footer_and_a_page_with_the_same_prompt_keep_their_own_ids() {
    let form = "Short prompt.\n\n:::subscribe\n:::\n";
    let site = Site::new(&[
        ("index.md", &format!("---\ntitle: Home\n---\n\n{form}")),
        ("footer.md", form),
    ]);
    let html = site.html("index.html");
    let inputs = email_inputs(&html);
    assert_eq!(inputs.len(), 2, "{inputs:?}");
    let (first, second) = (labelled_by(inputs[0]), labelled_by(inputs[1]));
    assert_ne!(first, second);
    for id in [first, second] {
        assert_eq!(html.matches(&format!(r#" id="{id}""#)).count(), 1, "{id} must be on exactly one element");
    }
    for p in html.match_indices("<p id=\"moss-subscribe-prompt").map(|(at, _)| &html[at..at + 120]) {
        assert!(p.contains("Short prompt."), "{p}");
    }
}
