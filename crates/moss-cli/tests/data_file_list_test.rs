//! A data file's rows become a page's list: `![[books.csv|style:list]]`
//! draws one list item per row, in the file's order, and `where:` keeps the
//! rows whose column holds a value. Built through the real CLI, so the cells'
//! markdown (links, emphasis) is rendered by the page's own markdown pass.

use std::path::Path;
use std::process::{Command, Stdio};

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

const BOOKS: &str = "title,link,description,group\n\
*Leaves of Grass*,https://books.example/leaves,\"The 1855 text, with a [later preface](https://books.example/preface).\",Poetry\n\
Atlas of the Coast,,No copy online.,Maps\n\
Odes,https://books.example/odes,,Poetry\n";

fn section<'a>(html: &'a str, heading: &str) -> &'a str {
    let start = html.find(heading).unwrap_or_else(|| panic!("no heading {heading} in:\n{html}"));
    let rest = &html[start..];
    let end = rest[1..].find("<h2").map_or(rest.len(), |i| i + 1);
    &rest[..end]
}

#[test]
fn a_data_file_lists_its_rows_and_where_picks_them() {
    let moss_home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();
    write(&site, "books.csv", BOOKS);
    write(
        &site,
        "index.md",
        "---\ntitle: Reading\n---\n\n## Poetry\n\n![[books.csv|style:list,where:group=Poetry]]\n\n## Maps\n\n![style:list|where:group=Maps](books.csv)\n\n## Everything\n\n![[books.csv]]\n",
    );

    let s = site.to_str().unwrap();
    let out = moss(&["build", s, "--no-plugins", "--strict"], moss_home.path());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "build must be clean under --strict; stderr:\n{stderr}");
    let html = std::fs::read_to_string(site.join(".moss/build.nosync/current/index.html")).unwrap();
    // How moss opens outside links is not this feature's business.
    let html = html.replace(r#" target="_blank" rel="noopener""#, "");

    let poetry = section(&html, ">Poetry<");
    assert!(poetry.contains("<ul>"), "rows are a list:\n{poetry}");
    assert!(
        poetry.contains(r#"<a href="https://books.example/leaves"><em>Leaves of Grass</em></a>, The 1855 text, with a <a href="https://books.example/preface">later preface</a>."#),
        "the title links to its row's link, the description follows with its own link:\n{poetry}"
    );
    assert!(poetry.find("Leaves of Grass") < poetry.find("Odes"), "file order is kept:\n{poetry}");
    assert!(!poetry.contains("Atlas"), "where: drops other groups:\n{poetry}");

    let maps = section(&html, ">Maps<");
    assert!(maps.contains("<li>Atlas of the Coast, No copy online.</li>"), "a row with no link is plain text, and the standard spelling takes the same options:\n{maps}");

    let everything = section(&html, ">Everything<");
    assert!(everything.contains("<td>Atlas of the Coast</td>"), "with no options the file is still a table:\n{everything}");
}

#[test]
fn a_filter_that_matches_no_row_fails_a_strict_build() {
    let moss_home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();
    write(&site, "books.csv", BOOKS);
    write(&site, "index.md", "---\ntitle: Reading\n---\n\n![[books.csv|style:list,where:group=Plays]]\n");

    let s = site.to_str().unwrap();
    let out = moss(&["build", s, "--no-plugins", "--strict"], moss_home.path());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("books.csv") && stderr.contains("where:"), "the build names the file and the filter; stderr:\n{stderr}");
    assert!(stderr.contains("index.md"), "the build names the page that holds the embed; stderr:\n{stderr}");
    assert!(!out.status.success(), "a list that silently shows nothing fails --strict; stderr:\n{stderr}");
}

#[test]
fn a_list_embed_that_shares_its_line_stays_a_table_and_says_why() {
    let moss_home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();
    write(&site, "books.csv", BOOKS);
    write(
        &site,
        "index.md",
        "---\ntitle: Reading\n---\n\n> ![[books.csv|style:list]]\n\nCaptions stay quiet: ![[books.csv|Note: draft]]\n\n- Shelf one\n\n  ![[books.csv|style:list]]\n- Shelf two\n",
    );

    let s = site.to_str().unwrap();
    let out = moss(&["build", s, "--no-plugins"], moss_home.path());
    let stderr = String::from_utf8_lossy(&out.stderr);
    let html = std::fs::read_to_string(site.join(".moss/build.nosync/current/index.html")).unwrap();
    let quote = &html[html.find("<blockquote").expect("a blockquote")..html.find("</blockquote>").unwrap()];
    assert!(quote.contains("<td>Atlas of the Coast</td>"), "the table stays inside the quote:\n{html}");
    assert!(!html.contains("<li>Atlas"), "no list leaks out of the quote:\n{html}");
    let item = &html[html.find("Shelf one").unwrap()..html.find("Shelf two").unwrap()];
    assert!(item.contains("<td>Atlas of the Coast</td>"), "an indented embed in a list item stays a table inside it:\n{html}");
    assert_eq!(stderr.matches("line of its own").count(), 2, "one warning each for the quote and the list item; stderr:\n{stderr}");
}
