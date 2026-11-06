//! The search and AI switches reach every file a build writes: robots.txt,
//! the robots meta tag on authored, generated and media pages, llms.txt and
//! /.well-known/tdmrep.json. A site that sets no switch keeps the robots.txt
//! its `ai_policy` has always produced.

#[path = "support/site.rs"]
mod site;

use site::Site;

const PAGES: &[(&str, &str)] = &[
    ("index.md", "---\ntitle: River Notes\n---\n\nThe water came up past the second step.\n"),
    ("about.md", "---\ntitle: About\n---\n\nA small site about the river.\n"),
    ("notes/spring.md", "---\ntitle: Spring\ntags: [flood]\n---\n\nFlood, day three.\n"),
    ("draft.md", "---\ntitle: Draft\ndraft: true\n---\n\nNot yet.\n"),
    ("gallery.md", "---\ntitle: Gallery\n---\n\n![Sunset](sunset.jpg)\n<!-- photography -->\n"),
];

fn robots_tags(html: &str) -> Vec<&str> {
    html.match_indices(r#"<meta name="robots""#).map(|(i, _)| &html[i..i + html[i..].find('>').unwrap() + 1]).collect()
}

#[test]
fn switches_reach_robots_txt_every_page_llms_txt_and_tdmrep() {
    let site = Site::with_config(
        "domain = \"example.com\"\nsearch_engines = false\nai_answers = false\nai_training = false\n",
        PAGES,
    );
    let all_off = r#"<meta name="robots" content="noindex, noai, noimageai">"#;
    for page in ["index.html", "about/index.html", "notes/spring/index.html", "notes/index.html", "photography/index.html", "tags/flood/index.html"] {
        assert_eq!(robots_tags(&site.html(page)), vec![all_off], "{page}");
    }

    let robots = site.html("robots.txt");
    assert!(robots.starts_with("# This site's requests to the programs that read it."), "{robots}");
    assert!(robots.contains("Content-Signal: search=no, ai-input=no, ai-train=no\nAllow: /\n"), "{robots}");
    assert!(robots.contains("User-agent: Claude-User\n") && robots.contains("User-agent: OAI-SearchBot\n"), "{robots}");
    assert!(!robots.contains("Sitemap:") && !robots.contains("Googlebot"), "{robots}");

    assert!(!site.output("llms.txt").exists(), "no llms.txt while AI answers are asked not to");

    assert!(site.html(".well-known/tdmrep.json").contains("\"tdm-reservation\": 1"));
}

#[test]
fn allowing_every_use_keeps_the_files_and_drops_the_tag() {
    let site = Site::with_config(
        "domain = \"example.com\"\nsearch_engines = true\nai_answers = true\nai_training = true\n",
        PAGES,
    );
    assert!(robots_tags(&site.html("about/index.html")).is_empty());
    assert_eq!(robots_tags(&site.html("draft/index.html")), vec![r#"<meta name="robots" content="noindex">"#]);
    assert!(site.html("llms.txt").contains("second step"));
    assert!(site.html(".well-known/tdmrep.json").contains("\"tdm-reservation\": 0"));
    assert!(site.html("robots.txt").contains("Sitemap: https://example.com/sitemap.xml"));
}

#[test]
fn turning_ai_answers_off_removes_llms_txt_on_the_next_build() {
    let site = Site::with_config("domain = \"example.com\"\nai_answers = true\n", PAGES);
    assert!(site.html("llms.txt").contains("second step"));
    site.write(".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\ndomain = \"example.com\"\nai_answers = false\n");
    site.rebuild();
    assert!(!site.output("llms.txt").exists());
}

#[test]
fn a_site_with_no_switch_keeps_the_robots_txt_its_policy_always_wrote() {
    let site = Site::with_config("domain = \"example.com\"\nai_policy = \"restricted\"\n", PAGES);
    assert_eq!(
        site.html("robots.txt"),
        "User-agent: *\nContent-Signal: search=no, ai-input=no, ai-train=no\nDisallow: /\n\nSitemap: https://example.com/sitemap.xml\n"
    );
    assert!(robots_tags(&site.html("about/index.html")).is_empty());
    assert!(robots_tags(&site.html("photography/index.html")).is_empty());
    assert!(!site.output("llms.txt").exists(), "a restricted site asked not to be used for answers");
    assert!(!site.output(".well-known/tdmrep.json").exists());
}

#[test]
fn the_llms_txt_a_site_stops_serving_is_its_own_choice() {
    use moss_build::build::manifest::change_set::{PublishedSnapshot, RemovalReason, WithdrawnBy};
    let site = Site::with_config("domain = \"example.com\"\nai_answers = true\n", PAGES);
    let mut snapshot = PublishedSnapshot { target: "moss:test".into(), published_at: "2026-11-01T00:00:00Z".into(), ..Default::default() };
    snapshot.files.insert("llms.txt".into(), "h".into());
    moss_build::build::manifest::published_record::save(&moss_build::moss_paths::MossPaths::new(&site.root), &snapshot).unwrap();
    site.write(".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\ndomain = \"example.com\"\nai_answers = false\n");
    site.rebuild();
    let removed = moss_build::system::build_records::records().removed_addresses(&site.root.to_string_lossy()).unwrap();
    let llms = removed.iter().find(|r| r.path == "llms.txt").expect("llms.txt is no longer served");
    assert_eq!((llms.reason, llms.withdrawn_by), (RemovalReason::AuthorRemoved, Some(WithdrawnBy::AiAnswers)));
}

#[test]
fn the_tdmrep_json_a_site_stops_serving_when_it_drops_its_switches_is_its_own_choice() {
    use moss_build::build::manifest::change_set::{PublishedSnapshot, RemovalReason, WithdrawnBy};
    let site = Site::with_config("domain = \"example.com\"\nai_training = false\n", PAGES);
    assert!(site.output(".well-known/tdmrep.json").exists());
    let mut snapshot = PublishedSnapshot { target: "moss:test".into(), published_at: "2026-11-01T00:00:00Z".into(), ..Default::default() };
    snapshot.files.insert(".well-known/tdmrep.json".into(), "h".into());
    moss_build::build::manifest::published_record::save(&moss_build::moss_paths::MossPaths::new(&site.root), &snapshot).unwrap();
    site.write(".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\ndomain = \"example.com\"\n");
    site.rebuild();
    let removed = moss_build::system::build_records::records().removed_addresses(&site.root.to_string_lossy()).unwrap();
    let tdmrep = removed.iter().find(|r| r.path == ".well-known/tdmrep.json").expect("tdmrep.json is no longer served");
    assert_eq!((tdmrep.reason, tdmrep.withdrawn_by), (RemovalReason::AuthorRemoved, Some(WithdrawnBy::AiSwitches)));
}
