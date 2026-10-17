//! Term pages are per language edition: a page's terms, claims, links and nav
//! entry stay inside the language tree it sits in, and the addresses terms had
//! before editions keep forwarding.

#[path = "support/site.rs"]
mod site;

use site::{hrefs_in, Site};

const PARTNERS: &str = "\n[terms.partners]\nfields = [\"partners\"]\ntitle = \"Partners\"\nnav = true\n";

fn partner_site(extra: &[(&str, &str)]) -> Site {
    let mut files: Vec<(&str, &str)> = vec![
        ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
        ("de/index.md", "---\ntitle: Start\n---\n\nWillkommen.\n"),
        (
            "Concert.md",
            "---\ntitle: Concert\ndate: 2026-01-01\npartners: [Ada Example, Only English]\nbyline: [With Ada Example and Only English]\n---\n\nText.\n",
        ),
        (
            "de/Konzert.md",
            "---\ntitle: Konzert\ndate: 2026-01-02\npartners: [Ada Example, Nur Deutsch]\nbyline: [Mit Ada Example und Nur Deutsch]\n---\n\nText.\n",
        ),
    ];
    files.extend_from_slice(extra);
    Site::with_config(&format!("domain = \"example.org\"\n{PARTNERS}"), &files)
}

#[test]
fn a_term_page_lists_only_the_pages_of_its_own_edition() {
    let site = partner_site(&[]);

    let en = site.html("partners/ada-example/index.html");
    assert!(en.contains("/concert/") && !en.contains("/de/konzert/"), "{en}");
    let de = site.html("de/partners/ada-example/index.html");
    assert!(de.contains("/de/konzert/") && !de.contains(r#"href="/concert/""#), "{de}");

    // A name only one edition uses has a page there and nowhere else.
    assert!(site.html("de/partners/nur-deutsch/index.html").contains("/de/konzert/"));
    assert!(!site.output("de/partners/only-english").exists());

    // The kind's root is per edition too.
    let root = site.html("de/partners/index.html");
    assert!(root.contains("/de/partners/nur-deutsch/") && !root.contains("/partners/only-english/"), "{root}");
}

#[test]
fn a_pages_term_links_point_into_its_own_edition() {
    let site = partner_site(&[]);
    assert!(hrefs_in(&site.html("concert/index.html")).contains(&"/partners/ada-example/"));
    let de = site.html("de/konzert/index.html");
    assert!(hrefs_in(&de).contains(&"/de/partners/ada-example/"), "{de}");
    assert!(!hrefs_in(&de).contains(&"/partners/ada-example/"), "{de}");
}

#[test]
fn the_pages_of_one_term_in_two_editions_are_translations() {
    let site = partner_site(&[]);
    let en = site.html("partners/ada-example/index.html");
    assert!(en.contains(r#"<link rel="alternate" hreflang="de" href="https://example.org/de/partners/ada-example/">"#), "{en}");
    assert!(hrefs_in(&en).contains(&"/de/partners/ada-example/"), "the switcher: {en}");
    let de = site.html("de/partners/ada-example/index.html");
    assert!(de.contains(r#"<link rel="alternate" hreflang="en" href="https://example.org/partners/ada-example/">"#), "{de}");
    // The roots pair up as well.
    let root = site.html("de/partners/index.html");
    assert!(root.contains(r#"<link rel="alternate" hreflang="en" href="https://example.org/partners/">"#), "{root}");
}

#[test]
fn a_claim_is_per_edition() {
    let site = partner_site(&[
        ("Ada.md", "---\ntitle: Ada Example\npartners_page: true\n---\n\nEnglish bio.\n"),
        ("de/Ada.md", "---\ntitle: Ada Example\npartners_page: true\n---\n\nDeutsche Biografie.\n"),
    ]);
    let duplicate = site.logged.iter().any(|m| m.contains("duplicate term claim"));
    assert!(!duplicate, "one claim per edition is no duplicate: {:?}", site.logged);

    // Each claim hosts its own edition's listing, and the byline links to it.
    let en = site.html("ada/index.html");
    assert!(en.contains("English bio") && en.contains("/concert/") && !en.contains("/de/konzert/"), "{en}");
    let de = site.html("de/ada/index.html");
    assert!(de.contains("Deutsche Biografie") && de.contains("/de/konzert/") && !de.contains(r#"href="/concert/""#), "{de}");
    assert!(hrefs_in(&site.html("de/konzert/index.html")).contains(&"/de/ada/"));
    assert!(hrefs_in(&site.html("concert/index.html")).contains(&"/ada/"));

    // Claimed pages are translations of each other even with different file names.
    assert!(de.contains(r#"<link rel="alternate" hreflang="en" href="https://example.org/ada/">"#), "{de}");
}

#[test]
fn a_duplicate_claim_inside_one_edition_still_warns() {
    let site = partner_site(&[
        ("de/Ada1.md", "---\ntitle: Dupe Beispiel\npartners_page: true\n---\n\nA\n"),
        ("de/Ada2.md", "---\ntitle: Dupe Beispiel\npartners_page: true\n---\n\nB\n"),
    ]);
    assert!(site.logged.iter().any(|m| m.contains("duplicate term claim") && m.contains("Dupe Beispiel")), "{:?}", site.logged);
}

#[test]
fn the_kind_gets_its_nav_entry_in_every_edition_that_has_it() {
    let site = partner_site(&[]);
    let (en_nav, _) = site.links("index.html");
    assert!(en_nav.contains(&"/partners/".to_string()) && !en_nav.contains(&"/de/partners/".to_string()), "{en_nav:?}");
    let (de_nav, _) = site.links("de/index.html");
    assert!(de_nav.contains(&"/de/partners/".to_string()) && !de_nav.contains(&"/partners/".to_string()), "{de_nav:?}");
    // A generated German page shows the German entry and not the default one.
    let (deep_nav, _) = site.links("de/partners/ada-example/index.html");
    assert!(deep_nav.contains(&"/de/partners/".to_string()) && !deep_nav.contains(&"/partners/".to_string()), "{deep_nav:?}");
}

#[test]
fn a_name_only_one_non_default_edition_uses_keeps_its_old_address() {
    let site = partner_site(&[]);

    // `/partners/nur-deutsch/` was the page before terms were per edition.
    let old = site.html("partners/nur-deutsch/index.html");
    assert!(old.contains("http-equiv=\"refresh\"") && old.contains("/de/partners/nur-deutsch/"), "{old}");
    // A name the default edition has keeps its own page there, not a stub.
    let kept = site.html("partners/ada-example/index.html");
    assert!(!kept.contains("http-equiv=\"refresh\""), "{kept}");

    let table = site.html("_moss/redirects.json");
    assert!(table.contains("partners/nur-deutsch/"), "{table}");
    assert!(!table.contains("\"partners/ada-example/\"") && !table.contains("partners/only-english"), "{table}");
}

#[test]
fn paired_edition_homes_keep_the_editions_term_pages() {
    let site = partner_site(&[
        ("index.md", "---\ntitle: Home\ntranslationKey: home\n---\n\nText.\n"),
        ("de/index.md", "---\ntitle: Start\ntranslationKey: home\n---\n\nWillkommen.\n"),
    ]);
    assert!(site.html("de/partners/nur-deutsch/index.html").contains("/de/konzert/"));
    assert!(site.html("de/partners/index.html").contains("/de/partners/nur-deutsch/"));
    let old = site.html("partners/nur-deutsch/index.html");
    assert!(old.contains("http-equiv=\"refresh\"") && old.contains("/de/partners/nur-deutsch/"), "{old}");
}

#[test]
fn a_tree_in_another_language_is_an_edition_on_a_site_in_a_language_moss_has_no_interface_for() {
    let site = Site::in_language(
        "fr",
        PARTNERS,
        &[
            ("index.md", "---\ntitle: Accueil\n---\n\nSalut.\n"),
            ("Posts/Un.md", "---\ntitle: Un\ndate: 2026-01-01\npartners: [Ada]\n---\n\nTexte.\n"),
            ("en/index.md", "---\ntitle: Home\n---\n\nHi.\n"),
            ("en/About.md", "---\ntitle: About\n---\n\nText.\n"),
            ("en/Posts/One.md", "---\ntitle: One\ndate: 2026-01-01\npartners: [Ada]\n---\n\nText.\n"),
            ("en/Contact.md", "---\ntitle: Contact\nfooter: true\nnav: false\n---\n\nText.\n"),
        ],
    );
    let footer = |page: &str| {
        let html = site.html(page);
        let from = html.find("<footer").unwrap_or(html.len());
        hrefs_in(&html[from..]).into_iter().map(str::to_string).collect::<Vec<_>>()
    };
    assert!(footer("en/posts/one/index.html").contains(&"/en/contact/".to_string()));
    assert!(!footer("posts/un/index.html").contains(&"/en/contact/".to_string()), "the French footer");
    let (_, home) = site.links("index.html");
    assert!(home.iter().all(|h| !h.starts_with("/en/")), "{home:?}");
    let (_, ada) = site.links("partners/ada/index.html");
    assert!(ada.contains(&"/posts/un/".to_string()) && !ada.contains(&"/en/posts/one/".to_string()), "{ada:?}");
    let (_, en_ada) = site.links("en/partners/ada/index.html");
    assert!(en_ada.contains(&"/en/posts/one/".to_string()), "{en_ada:?}");
    let (en_nav, _) = site.links("en/posts/one/index.html");
    assert!(en_nav.contains(&"/en/about/".to_string()), "a top-level page joins its edition's nav: {en_nav:?}");
    let (fr_nav, _) = site.links("posts/un/index.html");
    assert!(fr_nav.iter().all(|h| !h.starts_with("/en/about")), "{fr_nav:?}");
}

#[test]
fn a_tree_in_a_regional_form_of_the_site_language_is_part_of_the_default_edition() {
    for (lang, tree) in [("en-US", "en"), ("zh-CN", "zh-hans")] {
        let site = Site::in_language(
            lang,
            PARTNERS,
            &[
                ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
                ("About.md", "---\ntitle: About\n---\n\nText.\n"),
                (&format!("{tree}/Posts/One.md"), "---\ntitle: One\ndate: 2026-01-01\npartners: [Ada]\n---\n\nText.\n"),
                ("de/index.md", "---\ntitle: Start\n---\n\nWillkommen.\n"),
                ("de/Posts/Eins.md", "---\ntitle: Eins\ndate: 2026-01-01\npartners: [Ada]\n---\n\nText.\n"),
            ],
        );
        let ada = site.html("partners/ada/index.html");
        assert!(!ada.contains("http-equiv=\"refresh\"") && ada.contains(&format!("/{tree}/posts/one/")), "{lang}: {ada}");
        assert!(!site.output(&format!("{tree}/partners")).exists(), "{lang}");
    }
}

#[test]
fn a_built_in_kind_in_an_edition_with_no_interface_strings_takes_the_site_languages_title() {
    let site = Site::in_language(
        "de",
        "",
        &[
            ("index.md", "---\ntitle: Start\n---\n\nHallo.\n"),
            ("Posts/Eins.md", "---\ntitle: Eins\ndate: 2026-01-01\nauthor: Ada\n---\n\nText.\n"),
            ("ja/index.md", "---\ntitle: ホーム\n---\n\nこんにちは。\n"),
            ("ja/Posts/Ichi.md", "---\ntitle: 一\ndate: 2026-01-01\nauthor: Ada\n---\n\n本文。\n"),
        ],
    );
    assert!(site.html("ja/authors/index.html").contains(">Autorinnen und Autoren</h1>"));
}

#[test]
fn a_kind_is_named_in_each_editions_language_in_its_nav_and_root_alike() {
    let site = Site::with_config(
        "\n[terms.partners]\nfields = [\"partners\"]\nnav = true\n",
        &[
            ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
            ("Concert.md", "---\ntitle: Concert\ndate: 2026-01-01\npartners: [Ada]\nauthor: Ben\n---\n\nText.\n"),
            ("de/index.md", "---\ntitle: Start\n---\n\nWillkommen.\n"),
            ("de/Konzert.md", "---\ntitle: Konzert\ndate: 2026-01-02\npartners: [Ada]\nauthor: Ben\n---\n\nText.\n"),
        ],
    );
    for root in ["partners/index.html", "de/partners/index.html"] {
        assert!(site.html(root).contains(">Partners</h1>"), "a declared kind's key, capitalized, as in its nav entry: {root}");
    }
    assert!(site.html("authors/index.html").contains(">Authors</h1>"));
    assert!(site.html("de/authors/index.html").contains(">Autorinnen und Autoren</h1>"), "a built-in kind in the edition's own language");
}

#[test]
fn a_kind_only_one_non_default_edition_uses_forwards_its_root() {
    let site = Site::with_config(
        &format!("domain = \"example.org\"\n{PARTNERS}"),
        &[
            ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
            ("de/index.md", "---\ntitle: Start\n---\n\nWillkommen.\n"),
            ("de/Konzert.md", "---\ntitle: Konzert\ndate: 2026-01-02\npartners: [Nur Deutsch]\n---\n\nText.\n"),
        ],
    );
    assert!(site.html("de/partners/index.html").contains("Nur Deutsch"));
    let old_root = site.html("partners/index.html");
    assert!(old_root.contains("http-equiv=\"refresh\"") && old_root.contains("/de/partners/"), "{old_root}");
}

#[test]
fn a_tree_for_the_site_language_shares_the_default_editions_term_pages() {
    let site = Site::with_config(
        PARTNERS,
        &[
            ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
            ("Concert.md", "---\ntitle: Concert\ndate: 2026-01-01\npartners: [Ada Example]\n---\n\nText.\n"),
            ("en/Show.md", "---\ntitle: Show\ndate: 2026-01-02\npartners: [Ada Example]\n---\n\nText.\n"),
        ],
    );
    let page = site.html("partners/ada-example/index.html");
    assert!(page.contains("/concert/") && page.contains("/en/show/"), "{page}");
    assert!(!site.output("en/partners").exists());
    // The root home lists that tree's pages as its own.
    let (_, home) = site.links("index.html");
    assert!(home.contains(&"/en/show/".to_string()), "{home:?}");
}

#[test]
fn a_name_only_one_edition_uses_forwards_without_a_domain_too() {
    let site = Site::with_config(
        PARTNERS,
        &[
            ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
            ("de/index.md", "---\ntitle: Start\n---\n\nWillkommen.\n"),
            ("de/Konzert.md", "---\ntitle: Konzert\ndate: 2026-01-02\npartners: [Nur Deutsch]\n---\n\nText.\n"),
        ],
    );
    let old = site.html("partners/nur-deutsch/index.html");
    assert!(old.contains("http-equiv=\"refresh\"") && old.contains("/de/partners/nur-deutsch/"), "{old}");
    assert!(!site.output("_moss/redirects.json").exists(), "no table without a domain");
}

#[test]
fn a_moved_term_pages_qr_code_keeps_its_old_address() {
    let site = partner_site(&[]);
    let (old_qr, new_qr) = ("qr/partners/nur-deutsch.svg", "qr/de/partners/nur-deutsch.svg");
    assert!(site.output(new_qr).exists(), "the German page has a code");

    // The last publish served the old code: the next build must keep serving it.
    let mut snapshot = moss_build::build::manifest::change_set::PublishedSnapshot {
        target: "moss:test".into(),
        published_at: "2026-01-01T00:00:00Z".into(),
        ..Default::default()
    };
    snapshot.files.insert(old_qr.into(), "h".into());
    let paths = moss_build::moss_paths::MossPaths::new(&site.root);
    moss_build::build::manifest::published_record::save(&paths, &snapshot).unwrap();
    site.rebuild();

    assert!(site.output(old_qr).exists());
    assert_eq!(std::fs::read(site.output(old_qr)).ok(), std::fs::read(site.output(new_qr)).ok());
}

const TITLES: &str = "\n[terms.partners]\nfields = [\"partners\"]\ntitle = { en = \"Partners\", de = \"Partner\" }\nnav = true\n";

fn titled_site() -> Site {
    Site::with_config(
        TITLES,
        &[
            ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
            ("Concert.md", "---\ntitle: Concert\ndate: 2026-01-01\npartners: [Ada Example]\n---\n\nText.\n"),
            ("de/index.md", "---\ntitle: Start\n---\n\nWillkommen.\n"),
            ("de/Konzert.md", "---\ntitle: Konzert\ndate: 2026-01-02\npartners: [Ada Example]\n---\n\nText.\n"),
            ("fr/index.md", "---\ntitle: Accueil\n---\n\nBonjour.\n"),
            ("fr/Concert.md", "---\ntitle: Concert\ndate: 2026-01-02\npartners: [Ada Example]\n---\n\nTexte.\n"),
        ],
    )
}

#[test]
fn a_title_table_labels_the_nav_and_the_root_page_in_each_edition() {
    let site = titled_site();
    let nav_label = |page: &str, href: &str| {
        let html = site.html(page);
        let nav = &html[html.find("<nav").unwrap()..html.find("</nav>").unwrap()];
        let at = nav.find(&format!("href=\"{href}\"")).unwrap_or_else(|| panic!("no {href} in {page}'s nav"));
        nav[at..].split('>').nth(1).unwrap().split('<').next().unwrap().to_string()
    };
    assert_eq!(nav_label("index.html", "/partners/"), "Partners");
    assert_eq!(nav_label("de/index.html", "/de/partners/"), "Partner");
    // No entry for `fr`: the `en` one stands in.
    assert_eq!(nav_label("fr/index.html", "/fr/partners/"), "Partners");
    // A tree with no interface translation shows its own entry only.
    for page in ["fr/concert/index.html", "fr/partners/ada-example/index.html"] {
        let (nav, _) = site.links(page);
        assert!(nav.contains(&"/fr/partners/".to_string()) && !nav.contains(&"/partners/".to_string()), "{page}: {nav:?}");
    }

    let de_root = site.html("de/partners/index.html");
    assert!(de_root.contains("<title>Partner") && de_root.contains(">Partner</h1>"), "{de_root}");
    let en_root = site.html("partners/index.html");
    assert!(en_root.contains("<title>Partners") && en_root.contains(">Partners</h1>"), "{en_root}");
}

#[test]
fn a_kind_keyed_like_a_language_code_keeps_its_nav_entry_in_a_multilingual_site() {
    let site = Site::with_config(
        "\n[terms.he]\nfields = [\"partners\"]\nnav = true\n",
        &[
            ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
            ("About.md", "---\ntitle: About\n---\n\nText.\n"),
            ("Concerts/Concert.md", "---\ntitle: Concert\ndate: 2026-01-01\npartners: [Ada Example]\n---\n\nText.\n"),
            ("de/index.md", "---\ntitle: Start\n---\n\nWillkommen.\n"),
        ],
    );
    // `he/` is the kind's namespace here, not a Hebrew edition.
    for page in ["index.html", "he/index.html", "he/ada-example/index.html"] {
        let (nav, _) = site.links(page);
        assert!(nav.contains(&"/he/".to_string()), "{page}: {nav:?}");
    }
}
