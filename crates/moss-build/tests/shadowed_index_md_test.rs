//! A source-owned HTML document owns its address; the `index.md` beside it is
//! only metadata, at the site root and in a language edition alike.

#[path = "support/site.rs"]
mod site;

use site::Site;

const CONFIG: &str = "\n[build]\npassthrough = [\"index.html\", \"fr/index.html\"]\n";

fn site() -> Site {
    Site::with_config(
        CONFIG,
        &[
            ("index.md", "---\ntitle: Old\ndescription: Root meta\n---\n# Old headline\n\nShadowed paragraph zebra.\n"),
            ("index.html", "<!doctype html><title>Home</title><p>real home</p>"),
            ("fr/index.md", "---\ntitle: Accueil\ndescription: Meta fr\n---\n# Vieux titre\n\nParagraphe fantome giraffe.\n"),
            ("fr/index.html", "<!doctype html><title>Fr</title><p>fr home</p>"),
            ("fr/about.md", "---\ntitle: A propos\n---\n\nA propos texte.\n"),
        ],
    )
}

#[test]
fn an_edition_index_md_is_its_homes_metadata_not_a_page() {
    let site = site();
    assert!(site.html("fr/index.html").contains("fr home"));
    assert!(!site.logged.iter().any(|l| l.contains("would be the home")), "{:?}", site.logged);
}

#[test]
fn the_old_edition_index_address_forwards_to_the_edition_home() {
    let site = site();
    let stub = site.html("fr/index/index.html");
    assert!(stub.contains(r#"url=/fr/""#) && !stub.contains("giraffe"), "{stub}");
}

#[test]
fn a_shadowed_index_md_body_is_not_published_to_llms_txt() {
    let site = site();
    let llms = site.html("llms.txt");
    assert!(llms.contains("Root meta"), "metadata still serves the site: {llms}");
    assert!(llms.contains("A propos texte."), "other pages are untouched: {llms}");
    for body in ["zebra", "giraffe", "Old headline", "Vieux titre"] {
        assert!(!llms.contains(body), "{body} leaked: {llms}");
    }
    let warned = |file: &str| site.logged.iter().filter(|l| l.contains(file) && l.contains("never published")).count();
    assert_eq!((warned("`index.md`"), warned("`fr/index.md`")), (1, 1), "{:?}", site.logged);
}

#[test]
fn an_undeclared_edition_index_html_still_wins_its_address() {
    let site = Site::new(&[
        ("fr/index.md", "---\ntitle: Accueil\n---\n\nParagraphe giraffe.\n"),
        ("fr/index.html", "<!doctype html><title>Fr</title><p>fr html</p>"),
    ]);
    assert!(site.html("fr/index.html").contains("fr html"));
    assert!(site.html("fr/index/index.html").contains("giraffe"), "the markdown publishes at its own address");
    assert!(site.html("llms.txt").contains("giraffe"));
}
