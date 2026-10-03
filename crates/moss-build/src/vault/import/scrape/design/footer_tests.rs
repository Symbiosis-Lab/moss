use super::*;

fn footer_of(html: &str, primary_nav: &[&str]) -> Option<String> {
    let doc = Html::parse_document(html);
    let base = Url::parse("https://studio.example/").unwrap();
    let nav: HashSet<String> = primary_nav.iter().map(|s| s.to_string()).collect();
    read(&doc, &base, &nav)
}

#[test]
fn text_links_and_a_list_of_social_links_labelled_by_text_or_host() {
    let md = footer_of(
        r#"<body><footer>
            <p>12 Example Street,<br>Testville</p>
            <p>Questions? <a href="/contact">Write to us</a></p>
            <a href="https://instagram.com/studio" aria-label="Instagram"><svg></svg></a>
            <a href="https://www.facebook.com/studio"><i class="fa"></i></a>
            <a href="https://github.com/studio">Our code</a>
            <a href="https://instagram.com/studio" aria-label="Instagram again"></a>
        </footer></body>"#,
        &[],
    )
    .unwrap();
    assert!(md.contains("12 Example Street,"), "{md}");
    assert!(md.contains("[Write to us](https://studio.example/contact)"), "{md}");
    assert!(md.contains("- [Instagram](https://instagram.com/studio)"), "{md}");
    assert!(md.contains("- [facebook.com](https://www.facebook.com/studio)"), "{md}");
    assert!(md.contains("- [Our code](https://github.com/studio)"), "{md}");
    assert_eq!(md.matches("instagram.com").count(), 1, "one entry per URL: {md}");
}

#[test]
fn forms_scripts_and_a_nested_nav_that_repeats_the_primary_nav_are_dropped() {
    let md = footer_of(
        r#"<body><footer>
            <form><label>Email</label><input name="e"><button>Join</button></form>
            <script>track()</script><style>.x{}</style>
            <nav><a href="/about">About</a> <a href="/press">Press kit</a></nav>
            <p>(c) Studio Example</p>
        </footer></body>"#,
        &["https://studio.example/about"],
    )
    .unwrap();
    assert!(!md.contains("Email") && !md.contains("Join") && !md.contains("track"), "{md}");
    assert!(!md.contains("About"), "a primary nav link is not repeated: {md}");
    assert!(md.contains("[Press kit](https://studio.example/press)"), "{md}");
    assert!(md.contains("(c) Studio Example"), "{md}");
}

#[test]
fn the_last_footer_wins_and_an_empty_one_is_no_footer() {
    let md = footer_of(
        r#"<body><footer><p>First</p></footer><div role="contentinfo"><p>Last</p></div></body>"#,
        &[],
    )
    .unwrap();
    assert!(md.contains("Last") && !md.contains("First"), "{md}");

    assert_eq!(
        footer_of(r##"<body><footer><form><input></form><a href="#top"></a></footer></body>"##, &[]),
        None
    );
    assert_eq!(footer_of("<body><p>No footer</p></body>", &[]), None);
}

#[test]
fn lines_with_only_whitespace_or_zero_width_characters_are_dropped() {
    let md = footer_of(
        "<body><footer><p>First</p><p>\u{200B}</p><p>\u{FEFF} </p><p>Second</p></footer></body>",
        &[],
    )
    .unwrap();
    assert_eq!(md, "First\n\nSecond\n");
}
