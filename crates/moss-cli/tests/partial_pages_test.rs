//! A page marked `partial: true` is source material for transclusion, not a
//! page: the host page shows its content, but the build writes it no output
//! file and every surface that lists pages leaves it out. Built through the
//! real CLI so the whole pipeline (scan, parse, render, sitemap, llms.txt,
//! the `moss list` inventory) is covered, not one stage of it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const MARK: &str = "partial-art-marker";

fn write(site: &Path, rel: &str, body: &str) {
    let p = site.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn moss(args: &[&str], moss_home: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(args)
        .env("MOSS_HOME", moss_home)
        .env("MOSS_SITE_URL", "https://example.com")
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli")
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            files_under(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn a_partial_is_transcluded_but_never_published() {
    let moss_home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();

    write(&site, "index.md", "---\ntitle: Home\n---\n\nWelcome.\n\n![[figures/fig]]\n\n[Team](/team/)\n");
    write(&site, "about.md", "---\ntitle: About\n---\n\nAbout us.\n");
    // The only file in its folder, so a stray synthetic folder index would show.
    write(
        &site,
        "figures/fig.md",
        &format!("---\ntitle: Figure Source\npartial: true\n---\n\n<svg id=\"{MARK}\"><circle r=\"3\"/></svg>\n"),
    );
    // Shares its address with a real page: the partial must not take the slot
    // or force the real page to a numbered address.
    write(&site, "team.md", "---\ntitle: Team\npartial: true\n---\n\nShared blurb.\n");
    write(&site, "team/index.md", "---\ntitle: Our Team\n---\n\nThe people.\n");
    // Two non-folder pages whose slugs match: the partial sorts first, so if it
    // held the address the real page would be pushed to `-2`.
    write(&site, "Press Kit.md", "---\ntitle: Press Kit Source\npartial: true\n---\n\nPartial press blurb.\n");
    write(&site, "press-kit.md", "---\ntitle: Press Kit\n---\n\nThe real press page.\n");

    let s = site.to_str().unwrap();
    let out = moss(&["build", s, "--no-plugins", "--strict"], moss_home.path());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "build must be clean under --strict; stderr:\n{stderr}");

    let built = site.join(".moss/build.nosync/current");
    let home = std::fs::read_to_string(built.join("index.html")).unwrap();
    assert!(home.contains(MARK), "the host page must contain the partial's content");

    assert!(!built.join("figures").exists(), "a partial has no output directory, and no folder index is made for it");
    assert!(built.join("team/index.html").exists());
    assert!(!built.join("team-2").exists(), "the real page keeps its address");
    assert!(built.join("press-kit/index.html").exists());
    assert!(!built.join("press-kit-2").exists(), "a partial does not push the real page to a numbered address");

    let mut files = Vec::new();
    files_under(&built, &mut files);
    for f in files {
        let rel = f.strip_prefix(&built).unwrap().display().to_string();
        if rel.contains("_moss/og/") || !matches!(f.extension().and_then(|e| e.to_str()), Some("html" | "xml" | "txt" | "json" | "md")) {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap_or_default();
        // The host page's own text legitimately carries the embedded content.
        if !matches!(rel.as_str(), "index.html" | "llms.txt" | "_moss/previews.json") {
            assert!(!text.contains(MARK), "{rel} must not carry the partial's content");
        }
        assert!(!text.contains("figures/"), "{rel} must not mention the partial's address");
        assert!(!text.contains("Figure Source"), "{rel} must not name the partial");
        assert!(!text.contains("Partial press blurb"), "{rel} must not carry the third partial's content");
        assert!(!text.contains("Shared blurb"), "{rel} must not carry the second partial's content");
    }
    let sitemap = std::fs::read_to_string(built.join("sitemap.xml")).expect("sitemap.xml written");
    assert!(sitemap.contains("https://example.com/about/"), "real pages stay in the sitemap: {sitemap}");

    let list = moss(&["list", s], moss_home.path());
    let table = String::from_utf8_lossy(&list.stdout);
    assert!(!table.contains("figures/fig/") && !table.contains("figures/index"), "moss list must not show a URL for the partial:\n{table}");
}
