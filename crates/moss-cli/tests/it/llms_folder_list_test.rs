//! A folder-list embed reaches llms.txt as the pages it lists, as markdown
//! links in the listing's order, never as the build's internal marker.

use std::path::Path;
use std::process::{Command, Stdio};

fn write(site: &Path, rel: &str, body: &str) {
    let p = site.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

#[test]
fn a_folder_list_embed_is_written_to_llms_txt_as_links() {
    let moss_home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();

    write(&site, "index.md", "---\ntitle: Home\n---\n\nWelcome.\n\n::: {.directory}\n![[/notes/|sort:title]]\n:::\n\n## Recent\n\n![[/notes/|limit:1]]\n");
    write(&site, "notes/index.md", "---\ntitle: Notes\nchildren: false\n---\n\nAll notes.\n");
    write(&site, "notes/beta.md", "---\ntitle: Beta\ndate: 2024-01-01\n---\n\nSecond.\n");
    write(&site, "notes/alpha.md", "---\ntitle: Alpha [draft]\ndate: 2024-06-01\n---\n\nFirst.\n");

    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["build", site.to_str().unwrap(), "--no-plugins", "--strict"])
        .env("MOSS_HOME", moss_home.path())
        .env("MOSS_SITE_URL", "https://example.com")
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli");
    assert!(out.status.success(), "build failed:\n{}", String::from_utf8_lossy(&out.stderr));

    let llms = std::fs::read_to_string(site.join(".moss/build.nosync/current/llms.txt")).unwrap();
    assert!(!llms.contains("MOSS_MARKER"), "internal marker leaked:\n{llms}");

    let home = llms.split("## Home").nth(1).unwrap().split("\n---\n").next().unwrap();
    let alpha = "- [Alpha \\[draft\\]](/notes/alpha/)";
    let beta = "- [Beta](/notes/beta/)";
    let by_title = home.split("## Recent").next().unwrap();
    assert!(by_title.find(alpha).unwrap() < by_title.find(beta).unwrap(), "sort:title order:\n{home}");
    let recent = home.split("## Recent").nth(1).unwrap();
    assert!(recent.contains(alpha) && !recent.contains(beta), "limit:1 keeps the newest only:\n{home}");
}
