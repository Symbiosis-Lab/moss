//! Which pages carry the full-screen picture viewer, through the real binary.
//! A page gets the viewer's markup when it has any picture the viewer might
//! open (a figure, an image outside a link, a gallery); `fullscreen.js` decides which of them
//! do. The built `<main>` of each page is also committed as a fixture, so the
//! script's tests run against real build output: `fixtures_match_the_build`
//! fails when they go stale, and `SNAPSHOTS=overwrite` rewrites them. All
//! names and text are invented.

use std::path::{Path, PathBuf};
use std::process::Command;

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

// 1x1 transparent PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xff, 0xff, 0x3f,
    0x00, 0x05, 0xfe, 0x02, 0xfe, 0xa7, 0x35, 0x81, 0x84, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

struct Site {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

fn write(root: &Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// A site with one page per case, each holding the pictures its name says.
fn site() -> Site {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("site");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    write(&root, ".moss/config.toml", b"[site]\ndomain = \"example.com\"\n");
    write(&root, "index.md", b"---\ntitle: Home\n---\n\nWords only.\n");
    for png in ["a.png", "b.png"] {
        write(&root, png, PNG);
    }
    let pages: &[(&str, &str)] = &[
        ("figure", "![Harbour](a.png)\n*Dusk, [the quay](index.md)*\n"),
        ("caption-only", "![Dusk at the quay](a.png)\n"),
        ("lone-alt", "![Two dancers|](a.png)\n"),
        ("grid-cell", ":::grid 2\n![One](a.png)\n+++\n![Two](b.png)\n:::\n"),
        ("gallery", ":::gallery\na.png\nb.png\n:::\n"),
        (
            "gallery-captions",
            ":::gallery rows\n![Harbour](a.png)\n*Dusk, [the quay](index.md), 1931*\nb.png\n:::\n",
        ),
        ("linked", "[![Harbour](a.png)](index.md)\n"),
        ("hero", ":::hero {image=a.png}\n# Title\n:::\n"),
        ("decorative", "![](a.png)\n\n![[b.png|cover top]]\n"),
        ("opted-out", ":::{.no-viewer}\n![Harbour](a.png)\n:::\n"),
        ("opted-out-grid", ":::grid 2 {.no-viewer}\n![One](a.png)\n+++\n![Two](b.png)\n:::\n"),
    ];
    for (name, body) in pages {
        write(&root, &format!("{name}.md"), format!("---\ntitle: {name}\n---\n\n{body}").as_bytes());
    }
    write(&root, "cards/index.md", b"---\ntitle: Cards\nchildren_style: grid\n---\n\nCovers.\n");
    write(&root, "cards/one.md", b"---\ntitle: One\ncover: one.png\n---\n\nText.\n");
    write(&root, "cards/one.png", PNG);
    let out = Command::new(MOSS)
        .arg("build")
        .arg(&root)
        .env("MOSS_HOME", &home)
        .env_remove("MOSS_ENV")
        .output()
        .expect("spawn moss-cli");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    Site { _tmp: tmp, root }
}

impl Site {
    fn page(&self, url: &str) -> String {
        let file = self.root.join(".moss/build.nosync/current").join(url).join("index.html");
        std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()))
    }

    fn has_viewer(&self, url: &str) -> bool {
        self.page(url).contains(r#"id="lightbox""#)
    }
}

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../moss-build/src/js-src/site/__tests__/fixtures/picture-viewer"
);
const PAGES: &[&str] =
    &["figure", "caption-only", "lone-alt", "grid-cell", "gallery", "linked", "hero", "decorative", "opted-out", "opted-out-grid"];

#[test]
fn pages_with_a_picture_carry_the_viewer() {
    let site = site();
    for url in PAGES.iter().filter(|u| !["hero", "linked"].contains(u)) {
        assert!(site.has_viewer(url), "{url} should carry the viewer:\n{}", site.page(url));
    }
    let page = site.page("figure");
    assert!(page.contains("/_moss/js/fullscreen."), "the viewer's script tag is missing:\n{page}");
    assert!(page.contains(r#"<a href="../">the quay</a>"#), "the caption keeps its link:\n{page}");
}

#[test]
fn pages_without_a_picture_carry_no_viewer() {
    let site = site();
    for url in ["hero", "linked", "cards", "."] {
        assert!(!site.has_viewer(url), "{url} should not carry the viewer:\n{}", site.page(url));
    }
}

#[test]
fn the_opt_out_class_reaches_the_page() {
    let page = site().page("opted-out");
    assert!(page.contains(r#"<div class="no-viewer">"#), "{page}");
}

#[test]
fn fixtures_match_the_build() {
    let site = site();
    let overwrite = std::env::var("SNAPSHOTS").is_ok_and(|v| v == "overwrite");
    for name in PAGES {
        let page = site.page(name);
        let start = page.find("<main").unwrap_or_else(|| panic!("{name} has no <main>"));
        let end = page.find("</main>").unwrap_or_else(|| panic!("{name} has no </main>")) + "</main>".len();
        let main = format!("{}\n", &page[start..end]);
        let file = Path::new(FIXTURES).join(format!("{name}.html"));
        if overwrite {
            std::fs::create_dir_all(FIXTURES).unwrap();
            std::fs::write(&file, &main).unwrap();
        }
        let committed = std::fs::read_to_string(&file).unwrap_or_default();
        assert_eq!(committed, main, "{name}.html is stale; rerun with SNAPSHOTS=overwrite");
    }
}
