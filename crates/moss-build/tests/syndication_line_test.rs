//! Copy addresses remain data; site output never adds links to them.

#[path = "support/site.rs"]
mod site;

use site::Site;

const COPY: &str = "https://matters.town/a/abc123";

fn page(front: &str) -> String {
    format!("---\ntitle: Harbour log\ndate: 2026-01-01\n{front}---\n\nThe boats came in.\n")
}

#[test]
fn copies_are_absent_from_pages_feeds_and_json_ld_in_every_language() {
    for lang in ["en", "de", "zh-hans", "zh-hant"] {
        let post = page(&format!("syndicated:\n  - {COPY}\n  - https://bsky.app/profile/ana.example/post/3k\n  - javascript:alert(1)\n"));
        let site = Site::in_language(lang, "url = \"https://example.org\"\n", &[
            ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
            ("log.md", &post),
            ("single.md", &page(&format!("syndicated: {COPY}\n"))),
            ("plain.md", &page("")),
            ("folder/index.md", &page(&format!("layout: article\nsyndicated: {COPY}\n"))),
            ("folder/day.md", &page("")),
        ]);
        for output in ["index.html", "log/index.html", "single/index.html", "plain/index.html", "folder/index.html", "rss.xml"] {
            let html = site.html(output);
            for absent in [COPY, "https://bsky.app/profile/ana.example/post/3k", "javascript:alert", "moss-syndication", "u-syndication", "sameAs"] {
                assert!(!html.contains(absent), "{lang}: {output} contains {absent}");
            }
        }
        assert!(std::fs::read_to_string(site.root.join("log.md")).unwrap().contains(COPY));
    }
}

#[test]
fn authored_links_survive_without_an_automatic_duplicate() {
    let post = format!("{}\n[Read the discussion]({COPY})\n", page(&format!("syndicated: {COPY}\n")));
    let site = Site::new(&[("index.md", "# Home\n"), ("log.md", &post)]);
    let html = site.html("log/index.html");
    assert_eq!(html.matches(&format!("href=\"{COPY}\"")).count(), 1, "{html}");
    assert!(html.contains(">Read the discussion</a>"));
    assert!(!html.contains("u-syndication"));
}

#[test]
fn matched_copy_comments_keep_artalk_replies_and_author_links_only() {
    let post = page(&format!("uid: ab12cd34\ncomments: true\nsyndicated: {COPY}\n"));
    let site = Site::with_config("[services.comments]\nenabled = true\n", &[
        ("index.md", "# Home\n"), ("log.md", &post),
        (".moss/data/social/matters.json", r#"{"schemaVersion":"1.1.0","articles":{"ab12cd34":{"comments":[{"id":"m1","source":"matters","content":"Imported thought","createdAt":"2026-01-02","author":{"displayName":"Reader","url":"https://reader.example/"}}]}}}"#),
        (".moss/data/social/comment.json", r#"{"schemaVersion":"1.1.0","articles":{"ab12cd34":{"comments":[{"id":"1","source":"artalk","content":"Local thought","createdAt":"2026-01-02","author":{"displayName":"Reader"}}]}}}"#),
    ]);
    let html = site.html("log/index.html");
    assert!(html.contains("Imported thought") && html.contains("Local thought"), "{html}");
    assert!(html.contains("data-reply-id=\"1\""), "{html}");
    assert!(!html.contains("data-reply-id=\"m1\""));
    assert!(!html.contains("comment-source-link") && !html.contains(COPY));
    assert!(html.contains("href=\"https://reader.example/\""), "{html}");
}

/// Imports before `origin:` existed wrote the source into `syndicated:`.
#[test]
fn an_import_source_moves_to_origin_while_copy_metadata_is_preserved() {
    let old = "https://builder.example/u1/site/log/";
    let site = Site::new(&[
        ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
        ("log.md", &page(&format!("syndicated:\n- {old}\n- https://bsky.app/profile/ana.example/post/3k\n"))),
        ("other.md", &page("syndicated:\n  - https://notes.example/a\n")),
    ]);
    assert!(!site.html("log/index.html").contains(old));
    site.write("sources.json", &format!("[\"{old}\"]"));
    let migrate = || {
        let args: Vec<String> = [site.root.to_str().unwrap(), "--import-sources", site.root.join("sources.json").to_str().unwrap(), "--apply"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(moss_build::cli::doctor::migrate::run(&args), 0);
        std::fs::read_to_string(site.root.join("log.md")).unwrap()
    };
    let migrated = migrate();
    assert!(migrated.contains(&format!("origin: {old}\n")) && !migrated.contains(&format!("- {old}")), "{migrated}");
    assert_eq!(migrate(), migrated, "a second run changes nothing");

    site.rebuild();
    let html = site.html("log/index.html");
    assert!(!html.contains(old) && !html.contains("https://bsky.app/profile/ana.example/post/3k"), "{html}");
    assert!(migrated.contains("https://bsky.app/profile/ana.example/post/3k"));
    assert!(std::fs::read_to_string(site.root.join("other.md")).unwrap().contains("https://notes.example/a"));
    assert!(!site.html("other/index.html").contains("https://notes.example/a"));
}
