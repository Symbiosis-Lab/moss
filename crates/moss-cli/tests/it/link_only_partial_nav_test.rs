//! A `partial: true` file with `nav: true` and `external_url:` is no page and
//! is a header link to that address: an anchor on the home page needs no stub
//! page behind it. Built through the real CLI, so every surface that lists
//! pages is checked, not one stage of the build.

use std::path::Path;
use std::process::{Command, Stdio};

fn write(site: &Path, rel: &str, body: &str) {
    let p = site.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn build(site: &Path, moss_home: &Path) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["build", site.to_str().unwrap(), "--no-plugins"])
        .env("MOSS_HOME", moss_home)
        .env("MOSS_SITE_URL", "https://example.com")
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli");
    assert!(out.status.success(), "stderr:\n{}", String::from_utf8_lossy(&out.stderr));
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

#[test]
fn a_partial_with_nav_and_external_url_is_a_header_link_and_nothing_else() {
    let moss_home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();
    write(&site, "index.md", "---\ntitle: Home\n---\n\n## Menu\n\nBread and cakes.\n");
    write(&site, "about.md", "---\ntitle: About\nnav: true\nweight: 1\n---\n\nAbout us.\n");
    write(
        &site,
        "Menu link.md",
        "---\ntitle: Menu\npartial: true\nnav: true\nweight: 2\nexternal_url: \"/#menu\"\n---\n\nanchor-only-body.\n",
    );
    write(
        &site,
        "Order link.md",
        "---\ntitle: Order\nnav_label: Order now\npartial: true\nnav: true\nweight: 3\nexternal_url: \"mailto:orders@example.com\"\n---\n\nmail-only-body.\n",
    );
    write(&site, "Dead link.md", "---\ntitle: Dead end\npartial: true\nnav: true\nweight: 4\n---\n\ndead-body.\n");

    let log = build(&site, moss_home.path());
    let built = site.join(".moss/build.nosync/current");
    let home = std::fs::read_to_string(built.join("index.html")).unwrap();

    let about = home.find(r#"<a href="/about/">About</a>"#).expect("page entry");
    let menu = home.find(r##"<a href="/#menu">Menu</a>"##).expect("anchor entry");
    let order = home.find(r#"<a href="mailto:orders@example.com">Order now</a>"#).expect("mailto entry labelled by nav_label");
    assert!(about < menu && menu < order, "entries follow weight order");
    assert!(!home.contains("Dead end"), "a partial without external_url adds nothing");

    // Never a page, and absent from every surface that lists pages.
    for gone in ["menu-link", "order-link", "dead-link"] {
        assert!(!built.join(gone).exists(), "{gone} must not be built");
    }
    for surface in ["sitemap.xml", "rss.xml", "llms.txt"] {
        let text = std::fs::read_to_string(built.join(surface)).unwrap();
        assert!(!text.contains("only-body") && !text.contains("Menu link") && !text.contains("mailto:hello"), "{surface}:\n{text}");
    }
    assert!(!home.contains("anchor-only-body"));

    assert!(log.contains("Dead link.md") && log.contains("external_url"), "a warning names the file:\n{log}");
    assert!(!log.contains("Menu link.md"), "no warning for a partial that has its address:\n{log}");
}
