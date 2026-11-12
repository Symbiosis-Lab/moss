//! Invented restricted content must never reach any served artifact.
use std::fs;
use std::io::Read;
use moss_build::build::BuildTrigger;

#[path = "support/site.rs"]
mod site;

const SECRET: &str = "quartz-private-sentinel-837461";
const CONFIG: &str = "schema_version = 6\n[site]\nlang = 'en'\nurl = 'https://journal.example/'\nsearch = true\narchival = true\n[ai]\nllms_txt = true\n";

fn assert_public(site: &site::Site) {
    let mut count = 0;
    for entry in walkdir::WalkDir::new(site.output("")) {
        let entry = entry.unwrap();
        if !entry.file_type().is_file() { continue; }
        let bytes = fs::read(entry.path()).unwrap();
        assert!(!bytes.windows(SECRET.len()).any(|w| w == SECRET.as_bytes()),
            "private body leaked into {}", entry.path().display());
        count += 1;
    }
    // Search is generated asynchronously in production; exercise its real
    // indexer synchronously here and inspect decompressed fragment contents.
    let search = moss_build::build::feeds::search::build_search_index(&site.output("")).unwrap();
    assert!(search.pages > 0);
    let mut fragments = 0;
    for file in search.files.iter().filter(|f| f.rel_path.starts_with("fragment/")) {
        let mut decoded = String::new();
        flate2::read::GzDecoder::new(&file.bytes[..]).read_to_string(&mut decoded).unwrap();
        assert!(!decoded.contains(SECRET), "private search fragment: {}", file.rel_path);
        fragments += 1;
    }
    assert!(fragments > 0);
    assert!(count > 10, "must inspect a real site's served files");
    for name in ["index.html", "rss.xml", "_moss/previews.json", "llms.txt", "_moss/members.json"] {
        assert!(site.output(name).exists(), "missing {name}");
    }
}

#[test]
fn members_bodies_never_reach_public_outputs_or_transclusions() {
    let marked = format!("---\ntitle: A public title\ndate: 2026-01-01\ntags: [Journal]\nmembers: subscribers\n---\nVisible preview.\n<!--members-only-->\n{SECRET}\n");
    let no_marker = format!("---\ntitle: No preview\ndate: 2026-01-02\ndescription: A public description\nmembers: subscribers\n---\n{SECRET}\n");
    let paid = format!("members: paid\ntitle: Paid page\ndate: 2026-01-03\n---\nPaid public preview.\n<!--members-only-->\n{SECRET}\n");
    let invalid = format!("---\ntitle: Invalid declaration\nmembers: false\nweight: wrong\n---\n{SECRET}\n");
    let broken = format!("---\nmembers: [invalid\n---\n{SECRET}\n");
    let site = site::Site::new(&[
        (".moss/config.toml", CONFIG),
        ("index.md", "---\ntitle: Journal\n---\nWelcome.\n\n![[members]]\n\n![[no-preview]]\n\n![[paid]]\n\n![[invalid]]\n\n![[broken]]\n"),
        ("members.md", &marked), ("no-preview.md", &no_marker), ("paid.md", &paid),
        ("invalid.md", &invalid), ("broken.md", &broken),
    ]);
    assert_public(&site);
    assert!(site.html("members/index.html").contains("Visible preview."));
    assert!(site.html("index.html").contains("Visible preview."), "public transclusion keeps the preview");
    assert!(site.html("paid/index.html").contains("Paid public preview."));
    assert!(site.html("no-preview/index.html").contains("A public description"));
    assert!(site.html("members/index.html").contains("class=\"moss-members\""));
    assert!(site.html("members/index.html").contains("moss-subscribe-form"));
    assert_eq!(site.html("_moss/members.json"), "{\"join\":\"open\"}");
    assert!(!site.output("_gated").exists());
    assert!(site.html("_moss/source/members.md").contains("Visible preview."));
    assert!(site.html("_moss/source/paid.md").contains("Paid public preview."));
    // The author's source retains the full body.
    assert!(fs::read_to_string(site.root.join("members.md")).unwrap().contains(SECRET));
}

#[test]
fn a_public_page_becomes_restricted_on_cached_rebuild_and_join_policy_changes() {
    let public = format!("---\ntitle: Changing page\ndate: 2026-01-01\n---\n{SECRET}\n");
    let site = site::Site::new(&[
        (".moss/config.toml", CONFIG), ("index.md", "---\ntitle: Home\n---\n![[changing]]\n"),
        ("changing.md", &public),
    ]);
    assert!(site.html("changing/index.html").contains(SECRET));
    // Warm the in-process parse cache, including the transclusion host.
    site::build_sync(&site.root.to_string_lossy(), BuildTrigger::ContentOnly(vec![site.root.join("changing.md")])).unwrap();
    site.write("changing.md", &public.replacen("date:", "members: paid\ndate:", 1));
    site::build_sync(&site.root.to_string_lossy(), BuildTrigger::ContentOnly(vec![site.root.join("changing.md")])).unwrap();
    assert_public(&site);
    site.write(".moss/config.toml", &format!("{CONFIG}\n[members]\njoin = 'invite'\n"));
    site.rebuild();
    assert_public(&site);
    assert_eq!(site.html("_moss/members.json"), "{\"join\":\"invite\"}");
    site.write(".moss/config.toml", CONFIG);
    site.rebuild();
    assert_eq!(site.html("_moss/members.json"), "{\"join\":\"open\"}");
}

#[test]
fn an_invalid_join_policy_does_not_fall_back_to_open() {
    let site = site::Site::new(&[("index.md", "Home")]);
    site.write(".moss/config.toml", &format!("{CONFIG}\n[members]\njoin = 'invte'\n"));
    let error = site::build_sync(&site.root.to_string_lossy(), BuildTrigger::Full).unwrap_err();
    assert!(error.contains("[members]"), "{error}");
}

#[test]
fn every_markdown_spelling_projects_archives_and_transclusions() {
    let names = ["secret.MD", "Secret.Markdown", "third.mDoWn", "fourth.MkD"];
    let source = format!("---\ntitle: Restricted source\ndate: 2026-01-01\nmembers: subscribers\n---\nVisible mixed-case preview.\n<!--members-only-->\n{SECRET}\n");
    let home = format!("---\ntitle: Home\n---\n{}", names.iter().map(|name| format!("![[{name}]]\n\n")).collect::<String>());
    let mut files = vec![(".moss/config.toml", CONFIG), ("index.md", home.as_str())];
    for name in names { files.push((name, source.as_str())); }
    let site = site::Site::new(&files);
    assert_public(&site);
    for name in names {
        let archive = site.html(&format!("_moss/source/{name}"));
        assert!(archive.contains("Visible mixed-case preview."), "{name}");
        assert_eq!(fs::read_to_string(site.root.join(name)).unwrap(), source);
    }
    assert!(site.html("index.html").matches("Visible mixed-case preview.").count() >= 4);
}

#[test]
fn uncertain_members_frontmatter_exposes_no_metadata_or_preview() {
    let heads = [
        "--- \nmembers: paid\n",
        "members\n---\n",
        "'members' : paid\n---\n",
        "members:paid\n---\n",
        "MEMBERS: public\n---\n",
        "\u{feff}--- \r\nmembers: paid\r\n",
        "  ---\n 'members' : public\nbroken: [\n---\n",
        "---\nmembers paid\n---\n",
        "---\ntitle: Public title\nmembers: null\n---\n",
        "---\ntitle: Public title\nmembers: 'unknown'\n---\n",
        "---\n'members': public\nbroken: [\n---\n",
    ];
    let sources: Vec<_> = heads.iter().enumerate().map(|(i, head)| {
        (format!("uncertain-{i}.MD"), format!("{head}{SECRET} preview\n<!--members-only-->\n{SECRET}"))
    }).collect();
    let home = sources.iter().map(|(path, _)| format!("![[{path}]]\n\n")).collect::<String>();
    let mut files = vec![(".moss/config.toml", CONFIG), ("index.md", home.as_str())];
    files.extend(sources.iter().map(|(path, source)| (path.as_str(), source.as_str())));
    let site = site::Site::new(&files);
    assert_public(&site);
    for (path, source) in &sources {
        assert_eq!(fs::read_to_string(site.root.join(path)).unwrap(), *source);
        let archived = site.html(&format!("_moss/source/{path}"));
        assert!(archived.contains("members: subscribers"));
    }
}

#[test]
fn inherited_members_access_precedes_every_public_derivation_and_cached_rebuild() {
    let root_public = "---\ntitle: Home\ncascade: {members: public}\n---\nWelcome to the public journal.\n\n![[library/article.MD]]\n\n![[library/deeper/paid.mkd]]\n";
    let root_private = root_public.replace("members: public", "members: subscribers");
    let article = format!("---\ntitle: Article\ndate: 2026-02-01\n---\nVisible inherited preview.\n<!--members-only-->\n{SECRET}\n");
    let paid = format!("---\ntitle: Paid\nmembers: paid\n---\n{SECRET}\n");
    let secret_language = format!("这是一个中文故事，讲述春天的山川河流和人们的生活。{} {SECRET}", "我们每天都在这里写文章，分享自己的想法和经历。".repeat(30));
    let config = CONFIG.replace("lang = 'en'\n", "");
    let site = site::Site::new(&[
        (".moss/config.toml", &config), ("index.md", root_public),
        ("library/article.MD", &article), ("library/language.markdown", &secret_language),
        ("library/free.md", "---\nmembers: public\n---\nAlways public body."),
        ("library/deeper/index.md", "---\ncascade: {members: public}\n---\nDeeper visible preview.\n<!--members-only-->\nprivate descendant parent"),
        ("library/deeper/free.md", "Public through nearest ancestor."),
        ("library/deeper/paid.mkd", &paid),
    ]);
    assert!(site.html("library/article/index.html").contains(SECRET));
    site::build_sync(&site.root.to_string_lossy(), BuildTrigger::ContentOnly(vec![site.root.join("index.md")])).unwrap();
    site.write("index.md", &root_private);
    site::build_sync(&site.root.to_string_lossy(), BuildTrigger::ContentOnly(vec![site.root.join("index.md")])).unwrap();
    assert_public(&site);
    assert!(site.html("index.html").contains("Visible inherited preview."));
    assert!(site.html("library/free/index.html").contains("Always public body."));
    assert!(site.html("library/deeper/free/index.html").contains("Public through nearest ancestor."));
    assert!(!site.html("library/deeper/index.html").contains("private descendant parent"));
    assert!(site.html("library/article/index.html").contains("lang=\"en\""));
    assert!(site.html("_moss/source/library/article.MD").contains("Visible inherited preview."));
    assert_eq!(fs::read_to_string(site.root.join("library/article.MD")).unwrap(), article);
    site.write("index.md", root_public);
    site::build_sync(&site.root.to_string_lossy(), BuildTrigger::ContentOnly(vec![site.root.join("index.md")])).unwrap();
    assert!(site.html("library/article/index.html").contains(SECRET));
    site.write("index.md", &root_private.replace("members: subscribers", "members: paid"));
    site::build_sync(&site.root.to_string_lossy(), BuildTrigger::ContentOnly(vec![site.root.join("index.md")])).unwrap();
    assert_public(&site);
    assert!(site.html("library/article/index.html").contains("paid members"));
}

#[test]
fn malformed_ancestor_members_never_expose_descendant_bodies() {
    let article = format!("---\ntitle: Inherited article\ndate: 2026-02-01\n---\nVisible inherited preview.\n<!--members-only-->\n{SECRET}\n");
    let site = site::Site::new(&[
        (".moss/config.toml", CONFIG),
        ("index.md", "---\ntitle: Home\n---\nWelcome."),
        ("library/index.md", "---\nbroken: [\ncascade: {members: subscribers}\n---\nLibrary"),
        ("library/article.md", &article),
        ("library/free.md", "---\ntitle: Free article\ndate: 2026-02-02\nmembers: public\n---\nPublic override preview.\n<!--members-only-->\nExplicit public body."),
    ]);
    for path in ["library/article/index.html", "rss.xml", "llms.txt", "_moss/source/library/article.md"] {
        let output = site.html(path);
        assert!(!output.contains(SECRET), "private body leaked into {path}");
        assert!(output.contains("Visible inherited preview."), "missing public preview in {path}");
    }
    for path in ["library/free/index.html", "rss.xml", "llms.txt", "_moss/source/library/free.md"] {
        assert!(site.html(path).contains("Explicit public body."), "missing public override in {path}");
    }
    assert_public(&site);
    assert_eq!(fs::read_to_string(site.root.join("library/article.md")).unwrap(), article);
}

#[test]
fn the_members_notice_speaks_the_site_language() {
    let config = CONFIG.replace("lang = 'en'", "lang = 'de'");
    let site = site::Site::new(&[
        (".moss/config.toml", &config),
        ("index.md", "---\ntitle: Start\n---\nWillkommen."),
        ("artikel.md", &format!("---\ntitle: Artikel\nmembers: paid\n---\nVorschau.\n<!--members-only-->\n{SECRET}\n")),
    ]);
    let html = site.html("artikel/index.html");
    assert!(html.contains("Dieser Beitrag ist für zahlende Mitglieder."));
    assert!(!html.contains("This post is for"));
    assert!(html.contains("<p id=\"moss-members-lead-"));
    assert_public(&site);
}

/// Each members notice names its own field, so ids stay unique on a page
/// that shows several: a stream listing, or a page writing the notice twice.
#[test]
fn members_notices_keep_unique_ids_where_a_page_shows_several() {
    let post = |title: &str| format!("---\ntitle: {title}\ndate: 2026-03-01\nmembers: subscribers\n---\nPreview of {title}.\n<!--members-only-->\n{SECRET}\n");
    let twice = ":::subscribe {members=\"paid\"}\n:::\n\nBetween.\n\n:::subscribe {members=\"paid\"}\n:::\n";
    let site = site::Site::new(&[
        (".moss/config.toml", CONFIG),
        ("index.md", "---\ntitle: Journal\n---\n![[posts/|style:stream]]\n"),
        ("posts/first.md", &post("First")), ("posts/second.md", &post("Second")),
        ("twice.md", twice),
    ]);
    let html = site.html("index.html");
    assert!(html.contains("Preview of First.") && html.contains("Preview of Second."), "the stream shows both posts");
    for (page, count) in [(html, 2), (site.html("twice/index.html"), 2)] {
        let ids: Vec<&str> = page.split("<p id=\"").skip(1).filter_map(|rest| rest.split('"').next())
            .filter(|id| id.contains("moss-members-lead")).collect();
        assert_eq!(ids.len(), count, "{ids:?}");
        assert_ne!(ids[0], ids[1]);
        for id in ids {
            assert!(page.contains(&format!("aria-labelledby=\"{id} {id}-field\"")), "{id} names a field");
        }
    }
    assert_public(&site);
}

/// A site with language sections re-renders each inline form with its
/// section's scope; the members notice above the form survives that.
#[test]
fn the_members_notice_survives_the_language_section_scope() {
    let post = format!("---\ntitle: Post\nmembers: subscribers\n---\nPreview.\n<!--members-only-->\n{SECRET}\n");
    let site = site::Site::new(&[
        (".moss/config.toml", CONFIG),
        ("index.md", "---\ntitle: Journal\n---\nWelcome."),
        ("post.md", &post),
        ("zh-hans/index.md", "---\ntitle: 日志\n---\n欢迎。"),
        ("zh-hans/post.md", &post.replace("Preview.", "预览。")),
    ]);
    for path in ["post/index.html", "zh-hans/post/index.html"] {
        let html = site.html(path);
        let id = html.split("<p id=\"").nth(1).and_then(|rest| rest.split('"').next()).unwrap_or("");
        assert!(id.starts_with("moss-members-lead-"), "{path} keeps its notice");
        assert!(html.contains(&format!("aria-labelledby=\"{id} {id}-field\"")), "{path} names its field");
        assert!(html.contains("name=\"scope\" value=\""), "{path}");
    }
    assert_public(&site);
}
