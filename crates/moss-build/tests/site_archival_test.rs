//! `[site] archival = true`: every page names its build, says how to cite it, and
//! the site publishes a copy of its source under `_moss/source/`.

#[path = "support/site.rs"]
mod site;

use std::fs;
use std::path::{Path, PathBuf};

use site::Site;

const FILES: &[(&str, &str)] = &[
    ("index.md", "---\ntitle: Garden Notes\n---\n\nWelcome.\n"),
    ("essays/slow-web.md", "---\ntitle: The Slow Web\nauthor: Ada Example\ndate: 2026-03-02\nmood: calm\n---\n\nText.\n"),
    ("data/plants.csv", "name,height\nfern,30\n"),
    ("data/.secret.csv", "hidden,1\n"),
    (".drafts/never.md", "---\ntitle: Never\n---\n\nNo.\n"),
    ("de/index.md", "---\ntitle: Gartennotizen\n---\n\nWillkommen.\n"),
];

fn html_pages(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "html") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

fn is_redirect_stub(html: &str) -> bool {
    html.contains(r#"http-equiv="refresh""#)
}

fn files_under(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                out.push(path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    if dir.exists() {
        walk(dir, dir, &mut out);
    }
    out.sort();
    out
}

#[test]
fn a_site_without_the_option_is_built_as_before() {
    let site = Site::new(FILES);
    for page in html_pages(&site.output("")) {
        let html = fs::read_to_string(&page).unwrap();
        assert!(!html.contains("moss:edition"), "{}", page.display());
        assert!(!html.contains("moss-edition"), "{}", page.display());
        assert!(!html.contains("dcterms.modified"), "{}", page.display());
    }
    assert!(!site.output("_moss/source").exists(), "no source copy without the option");
}

#[test]
fn every_page_carries_the_edition_line_and_its_meta_tags() {
    let site = Site::with_config("archival = true\n", FILES);
    let pages = html_pages(&site.output(""));
    assert!(pages.len() >= 4, "{pages:?}");
    let mut seen = 0;
    for page in pages {
        let html = fs::read_to_string(&page).unwrap();
        if is_redirect_stub(&html) {
            continue;
        }
        seen += 1;
        let name = page.display();
        assert!(html.contains(r#"<meta name="dcterms.modified" content=""#), "{name}");
        let meta = html.split(r#"<meta name="moss:edition" content=""#).nth(1).unwrap_or_else(|| panic!("{name}: no moss:edition meta"));
        let content = meta.split('"').next().unwrap();
        let (date, id) = content.split_once(' ').unwrap_or_else(|| panic!("{name}: {content}"));
        assert_eq!(date.len(), 10, "{name}: {content}");
        assert!(date.as_bytes()[4] == b'-' && date.as_bytes()[7] == b'-', "{name}: {content}");
        assert_eq!(id.len(), 7, "{name}: {content}");
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "{name}: {content}");
        assert!(html.contains(&format!("{date} · {id}")), "{name}: the visible edition line");
        assert!(html.contains(r#"href="/_moss/source/README.md""#), "{name}: link to the source copy");
        // A footer <section> is a footer.md column, which turns the footer into a column grid.
        assert!(html.contains(r#"<div class="moss-edition">"#), "{name}");
        assert!(!html.contains(r#"<section class="moss-edition""#), "{name}");
    }
    assert!(seen >= 4);
}

#[test]
fn every_page_has_a_cite_block_naming_author_page_site_and_edition() {
    let site = Site::with_config("archival = true\nauthor = \"Site Keeper\"\n", FILES);
    let html = site.html("essays/slow-web/index.html");
    let cite = html.split(r#"class="moss-cite""#).nth(1).expect("cite block");
    let cite = cite.split("</div>").next().unwrap();
    assert!(cite.contains("Cite this page"), "{cite}");
    assert!(cite.contains("The Slow Web"), "page title: {cite}");
    assert!(cite.contains("Ada Example. \u{201c}The Slow Web"), "the page's own author leads: {cite}");
    assert!(!cite.contains("Site Keeper"), "{cite}");
    assert!(cite.contains("Garden Notes"), "site title: {cite}");
    assert!(cite.contains("Edition 20"), "edition date: {cite}");
    assert!(!cite.contains("/essays/slow-web/"), "a build with no published address cites none: {cite}");

    let home = site.html("index.html");
    let home_cite = home.split(r#"class="moss-cite-text">"#).nth(1).expect("home page has the block too");
    assert!(home_cite.starts_with("Site Keeper. "), "a page without an author is cited to the site's: {home_cite}");

    let german = site.html("de/index.html");
    assert!(german.contains("Diese Seite zitieren"), "German label: {german}");
    assert!(!german.contains("Cite this page"));
}

#[test]
fn the_source_copy_holds_the_authors_files_and_a_readme() {
    let site = Site::with_config("archival = true\nlicense = \"CC BY 4.0\"\n", FILES);
    let copied = files_under(&site.output("_moss/source"));
    assert!(copied.contains(&"README.md".to_string()), "{copied:?}");
    assert!(copied.contains(&"index.md".to_string()), "{copied:?}");
    assert!(copied.contains(&"essays/slow-web.md".to_string()), "{copied:?}");
    assert!(copied.contains(&"data/plants.csv".to_string()), "{copied:?}");
    assert!(copied.contains(&"de/index.md".to_string()), "{copied:?}");
    assert!(!copied.iter().any(|f| f.contains("secret") || f.contains("never")), "hidden files stay out: {copied:?}");
    assert!(!copied.iter().any(|f| f.starts_with(".moss") || f.contains("config.toml")), "{copied:?}");

    let original = fs::read_to_string(site.root.join("essays/slow-web.md")).unwrap();
    assert_eq!(fs::read_to_string(site.output("_moss/source/essays/slow-web.md")).unwrap(), original);

    let readme = fs::read_to_string(site.output("_moss/source/README.md")).unwrap();
    for field in ["title", "author", "date", "mood"] {
        assert!(readme.contains(&format!("`{field}`")), "front-matter field {field}: {readme}");
    }
    assert!(readme.contains("CC BY 4.0"), "licence: {readme}");
    assert!(readme.contains("essays/slow-web.md"), "{readme}");

    for page in ["index.html", "essays/slow-web/index.html", "de/index.html"] {
        assert!(site.html(page).contains(r#"<p class="moss-edition-rights">CC BY 4.0</p>"#), "{page} states the licence");
    }
}

#[test]
fn the_readme_omits_the_licence_line_when_the_site_declares_none() {
    let site = Site::with_config("archival = true\n", FILES);
    let readme = fs::read_to_string(site.output("_moss/source/README.md")).unwrap();
    assert!(!readme.to_lowercase().contains("licence"), "{readme}");
    assert!(!site.html("index.html").contains("moss-edition-rights"));
}

#[test]
fn the_edition_id_follows_the_source_not_the_clock() {
    let site = Site::with_config("archival = true\n", FILES);
    let id_of = |site: &Site| {
        let html = site.html("index.html");
        let rest = html.split(r#"<meta name="moss:edition" content=""#).nth(1).unwrap().to_string();
        rest.split('"').next().unwrap().split_once(' ').unwrap().1.to_string()
    };
    let first = id_of(&site);
    site.rebuild();
    assert_eq!(id_of(&site), first, "same source, same id");
    site.write("essays/slow-web.md", "---\ntitle: The Slow Web\n---\n\nChanged.\n");
    site.rebuild();
    assert_ne!(id_of(&site), first, "a changed source is a new edition");
}
