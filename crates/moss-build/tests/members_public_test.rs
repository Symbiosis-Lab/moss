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
        if !entry.file_type().is_file() || entry.path().starts_with(site.output("_gated")) { continue; }
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
    // A subscribers page has its whole version under the gated prefix, at the
    // same path; a paid page has none, since no host can check a paid reader.
    let whole = site.html("_gated/members/index.html");
    assert!(whole.contains(SECRET) && whole.contains("Visible preview."));
    assert!(!whole.contains("class=\"moss-members\""), "the whole version asks nobody to subscribe");
    assert!(site.html("_gated/no-preview/index.html").contains(SECRET));
    assert!(!site.output("_gated/paid").exists());
    let manifest = manifest_of(&site);
    let gated = moss_build::build::emit::members::gated_entries(&manifest);
    assert_eq!(
        serde_json::to_value(gated.get("_gated/members/index.html")).unwrap(),
        serde_json::json!({ "access": "subscribers", "fallback": "projection" })
    );
    assert!(gated.keys().all(|path| manifest.contains_key(path.strip_prefix("_gated/").unwrap())), "every gated page has its public version");
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
    assert!(site.html("library/article/index.html").contains("Paid members only."));
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
    assert!(html.contains("Nur für zahlende Mitglieder."));
    assert!(!html.contains("Paid members only."));
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

/// The publish door gives a generation that restricted a page the floor an
/// import gives one: a moss without members then refuses the site instead of
/// publishing its members pages whole.
#[test]
fn publishing_a_members_generation_sets_the_folder_floor() {
    use moss_build::vault::config::MEMBERS_MOSS;
    // A moss below a folder's floor refuses to write its config.
    moss_build::system::seed_app_version(MEMBERS_MOSS);
    for config in [Some("schema_version = 6\n\n[site]\nlang = 'en'\n"), None, Some("[site]\nlang = 'en'\n")] {
        let site = site::Site::new(&[("index.md", "Home")]);
        let path = site.root.join(".moss/config.toml");
        match config {
            Some(text) => fs::write(&path, text).unwrap(),
            None => fs::remove_file(&path).unwrap(),
        }
        moss_build::build::site_config::publish_door(&site.root, true).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(moss_build::config::ConfigFile::parse(&text).unwrap().min_moss_version(), Some(MEMBERS_MOSS), "{text}");
        // An unversioned file keeps its number until a migration saves it in the current shape.
        if config.is_none_or(|text| text.starts_with("schema_version")) {
            let raw: toml::Table = toml::from_str(&text).unwrap();
            assert_eq!(raw["schema_version"].as_integer(), Some(7), "the stamp a moss from before the floor refuses: {text}");
        }
    }
}

const ROW: &str = "harbour-row-sentinel-552901";
const PAID_ROW: &str = "ledger-row-sentinel-118734";

/// Every file a visitor can fetch without the gate: the whole output outside
/// the gated prefix, the published source copy included.
fn public_bytes(site: &site::Site) -> Vec<(String, Vec<u8>)> {
    let root = site.output("");
    walkdir::WalkDir::new(&root).into_iter().map(Result::unwrap).filter(|e| e.file_type().is_file())
        .map(|e| (e.path().strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"), fs::read(e.path()).unwrap()))
        .filter(|(rel, _)| !rel.starts_with("_gated/"))
        .collect()
}

fn assert_absent(site: &site::Site, secret: &str) {
    let files = public_bytes(site);
    assert!(files.len() > 10, "must inspect a real site's served files");
    for (rel, bytes) in &files {
        assert!(!bytes.windows(secret.len()).any(|w| w == secret.as_bytes()), "{secret} leaked into {rel}");
    }
    let search = moss_build::build::feeds::search::build_search_index(&site.output("")).unwrap();
    for file in search.files.iter().filter(|f| f.rel_path.starts_with("fragment/")) {
        let mut decoded = String::new();
        flate2::read::GzDecoder::new(&file.bytes[..]).read_to_string(&mut decoded).unwrap();
        assert!(!decoded.contains(secret), "{secret} in search fragment {}", file.rel_path);
    }
}

fn manifest_of(site: &site::Site) -> std::collections::HashMap<String, String> {
    let hashes = fs::read_to_string(site.root.join(".moss/build.nosync/hashes.json")).unwrap();
    let hashes: serde_json::Value = serde_json::from_str(&hashes).unwrap();
    hashes["files"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string())).collect()
}

const MAP_SITE: &[(&str, &str)] = &[
    (".moss/config.toml", CONFIG),
    ("index.md", "---\ntitle: Harbour watch\n---\nWelcome.\n"),
    ("maps/index.md", "---\ntitle: Map\n---\nLevels by region.\n\n![[regions.csv]]\n"),
    ("maps/regions.csv", "name,level\nNorth quay,2\n"),
    ("maps/incidents/index.md", "---\npartial: true\ncascade: { members: subscribers }\n---\n"),
    ("maps/incidents/incidents.csv", "place,date,what\nNorth quay,2026-01-04,harbour-row-sentinel-552901\n"),
    ("maps/incidents/by-region.json", "{\"north-quay\":[\"harbour-row-sentinel-552901\"]}"),
    ("maps/incidents/report.md", "---\ntitle: Report\ndate: 2026-01-05\n---\nWhat happened, row by row.\n\n![[incidents.csv]]\n"),
    ("ledger/index.md", "---\ntitle: Ledger\ncascade: { members: paid }\n---\nLedger.\n"),
    ("ledger/ledger.csv", "entry\nledger-row-sentinel-118734\n"),
];

#[test]
fn a_data_file_under_a_members_cascade_is_published_only_under_the_gated_prefix() {
    let site = site::Site::new(MAP_SITE);
    for name in ["incidents.csv", "by-region.json"] {
        let gated = site.html(&format!("_gated/maps/incidents/{name}"));
        assert!(gated.contains(ROW), "the gated copy holds the file whole");
        assert!(!site.output(&format!("maps/incidents/{name}")).exists(), "no public copy of {name}");
    }
    assert!(site.html("maps/regions.csv").contains("North quay"), "a file outside the cascade is public");
    // A page under the same cascade may embed the gated file: the table is in
    // its members-only version and nowhere else.
    let report = site.html("_gated/maps/incidents/report/index.html");
    assert!(report.contains(ROW) && report.contains("<table"), "the gated page draws the gated table");
    assert!(!site.html("maps/incidents/report/index.html").contains("row by row"));
    assert!(site.html("maps/index.html").contains("North quay"), "a public file still embeds as a table");
    assert_absent(&site, ROW);

    // No host can check a paid reader yet, so a paid file is in no output at all.
    assert_absent(&site, PAID_ROW);
    assert!(!site.output("_gated/ledger").exists());
    assert!(!site.output("ledger/ledger.csv").exists());

    let manifest = manifest_of(&site);
    assert!(!manifest.contains_key("maps/incidents/incidents.csv"));
    let gated = moss_build::build::emit::members::gated_entries(&manifest);
    assert_eq!(
        serde_json::to_value(&gated).unwrap(),
        serde_json::json!({
            "_gated/maps/incidents/by-region.json": { "access": "subscribers", "fallback": "unauthorized" },
            "_gated/maps/incidents/incidents.csv": { "access": "subscribers", "fallback": "unauthorized" },
            "_gated/maps/incidents/report/index.html": { "access": "subscribers", "fallback": "projection" },
        })
    );

    // Renaming the file keeps it gated: the folder is the marker.
    fs::rename(site.root.join("maps/incidents/incidents.csv"), site.root.join("maps/incidents/renamed.csv")).unwrap();
    site.rebuild();
    assert!(site.html("_gated/maps/incidents/renamed.csv").contains(ROW));
    assert!(!site.output("_gated/maps/incidents/incidents.csv").exists());
    assert_absent(&site, ROW);
}

#[test]
fn the_manifest_records_the_bytes_written_for_each_gated_file() {
    let mut files = MAP_SITE.to_vec();
    files.push(("about.md", "---\ntitle: About\n---\nWho we are.\n"));
    let site = site::Site::new(&files);
    assert_gated_manifest_matches_generation(&site, "cold");
    // A carried page is not rendered, so its gated version must be rendered on
    // its own. The first unobserved build after a full one renders in full;
    // the second proves from its seal and carries.
    unobserved_build(&site);
    assert!(unobserved_build(&site) > 0, "the unchanged build carried no page");
    assert_gated_manifest_matches_generation(&site, "unchanged");
    site.write("about.md", "---\ntitle: About\n---\nWho we are, and since when.\n");
    assert!(unobserved_build(&site) > 0, "the edited build carried no page");
    assert_gated_manifest_matches_generation(&site, "public edit");
}

const TIDE: &str = "tide-private-sentinel-430918";

/// A cascade added to or removed from a warm site moves a page it never
/// touched into or out of `_gated/`, and the manifest follows.
#[test]
fn a_cascade_toggled_on_a_warm_site_moves_its_pages_into_and_out_of_gated() {
    let tide = format!("---\ntitle: Tide\ndate: 2026-01-06\n---\nTables for the week.\n<!--members-only-->\n{TIDE}\n");
    let mut files = MAP_SITE.to_vec();
    files.push(("notes/index.md", "---\ntitle: Notes\n---\nNotes.\n"));
    files.push(("notes/tide.md", &tide));
    let site = site::Site::new(&files);
    unobserved_build(&site);
    assert!(unobserved_build(&site) > 0, "the site is warm");
    assert!(site.html("notes/tide/index.html").contains(TIDE), "a public page shows its whole body");

    site.write("notes/index.md", "---\ntitle: Notes\ncascade: { members: subscribers }\n---\nNotes.\n");
    assert_eq!(unobserved_build(&site), 0, "a cascade edit renders every page; if it now carries pages, prove here that a carried page's public version loses its members-only text and its _gated/ entry is written");
    assert!(!site.html("notes/tide/index.html").contains(TIDE), "the cascaded page's public version holds its members-only text");
    assert!(site.html("_gated/notes/tide/index.html").contains(TIDE), "the gated version holds the whole body");
    assert!(manifest_of(&site).contains_key("_gated/notes/tide/index.html"), "the gated page is in the manifest");
    assert_gated_manifest_matches_generation(&site, "cascade added");

    site.write("notes/index.md", "---\ntitle: Notes\n---\nNotes.\n");
    assert_eq!(unobserved_build(&site), 0, "a cascade edit renders every page; if it now carries pages, prove here that a carried page's _gated/ entry is removed and its public version is whole again");
    assert!(!manifest_of(&site).contains_key("_gated/notes/tide/index.html"), "the gated entry outlived its cascade");
    assert!(!site.output("_gated/notes/tide/index.html").exists(), "the gated file outlived its cascade");
    assert!(site.html("notes/tide/index.html").contains(TIDE), "the page is public again");
    assert_gated_manifest_matches_generation(&site, "cascade removed");
}

/// Build as `moss build` does, proving from the last seal; returns how many
/// pages it carried.
fn unobserved_build(site: &site::Site) -> u64 {
    site::build_sync(&site.root.to_string_lossy(), BuildTrigger::Unobserved).unwrap();
    let profile = fs::read_to_string(site.root.join(".moss/build.nosync/profile.jsonl")).unwrap();
    let record: serde_json::Value = serde_json::from_str(profile.lines().last().unwrap()).unwrap();
    record["counts"]["pages_carried"].as_u64().unwrap()
}

fn assert_gated_manifest_matches_generation(site: &site::Site, build_kind: &str) {
    let manifest = manifest_of(site);
    // The bytes a deploy uploads: the sealed generation, not the preview stage.
    let build = site.root.join(".moss/build.nosync");
    let generation = build.join("generations").join(fs::read_to_string(build.join("current.generation")).unwrap().trim());
    let gated: Vec<_> = manifest.iter().filter(|(path, _)| path.starts_with("_gated/")).collect();
    for name in ["_gated/maps/incidents/report/index.html", "_gated/maps/incidents/by-region.json"] {
        assert!(gated.iter().any(|(path, _)| *path == name), "{build_kind} build: {name} is in the manifest");
    }
    for (path, entry) in gated {
        let written = moss_build::build::assets::paths::compute_binary_hash(&fs::read(generation.join(path)).unwrap());
        assert_eq!(entry.rsplit(':').next(), Some(written.as_str()), "{build_kind} build: {path}: the manifest names other bytes than the file holds");
    }
}

#[test]
fn a_public_page_that_embeds_a_gated_file_stops_the_build() {
    let mut files = MAP_SITE.to_vec();
    files.push(("maps/leak.md", "---\ntitle: Leak\n---\n![[incidents/incidents.csv]]\n"));
    let root = std::env::temp_dir().join(format!("moss_gated_embed_{}", uuid::Uuid::new_v4()));
    for (rel, body) in &files {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    // The public part of a members page is as public as any page.
    files.push(("maps/incidents/teaser.md", "---\ntitle: Teaser\n---\n![[incidents.csv]]\n<!--members-only-->\nThe rest.\n"));
    // And a subscribers page cannot show a paid file.
    files.push(("maps/incidents/crossed.md", "---\ntitle: Crossed\n---\n![[ledger/ledger.csv]]\n"));
    for (rel, body) in &files {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    let error = site::build_sync(&root.to_string_lossy(), BuildTrigger::Full).unwrap_err();
    assert!(error.contains("maps/leak.md embeds incidents/incidents.csv"), "{error}");
    assert!(error.contains("maps/incidents/teaser.md embeds incidents.csv"), "{error}");
    assert!(error.contains("maps/incidents/crossed.md embeds ledger/ledger.csv"), "{error}");
    assert!(!error.contains("report.md"), "a page under the cascade may embed its file: {error}");
    let staged = root.join(".moss/build.nosync/staging");
    if staged.exists() {
        for entry in walkdir::WalkDir::new(&staged).into_iter().map(Result::unwrap).filter(|e| e.file_type().is_file()) {
            let rel = entry.path().strip_prefix(&staged).unwrap().to_string_lossy().replace('\\', "/");
            let bytes = fs::read(entry.path()).unwrap();
            assert!(rel.starts_with("_gated/") || !bytes.windows(ROW.len()).any(|w| w == ROW.as_bytes()), "{ROW} leaked into {rel}");
        }
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_members_cascade_on_a_page_that_is_not_a_folder_home_is_reported() {
    let site = site::Site::new(&[
        ("index.md", "---\ntitle: Home\n---\nHome.\n"),
        ("maps/index.md", "---\ntitle: Map\n---\nMap.\n"),
        ("maps/detail.md", "---\ntitle: Detail\ncascade: { members: subscribers }\n---\nDetail.\n"),
    ]);
    assert!(site::problems().iter().any(|line| line.contains("[maps/detail.md]") && line.contains("has no effect")), "{:?}", site.logged);
}

const PARA: &str = "lantern-paragraph-sentinel-770412";

fn field_notes(below_the_line: &str) -> String {
    format!("---\ntitle: Field notes\ndate: 2026-02-01\nmembers: subscribers\n---\nPublic preview.\n<!--members-only-->\n{below_the_line}")
}

/// What a members page holds below its line changes no file anyone can
/// fetch: the build of the whole page and the build of its public part alone
/// write the same public files, whatever the private part draws.
#[test]
fn the_private_part_of_a_members_page_changes_no_public_file() {
    let below = format!(
        "{PARA}\n\n![[shot.png]]\n\n$$e^{{i\\pi}}+1=0$$\n\n```mermaid\ngraph TD; A-->B;\n```\n\n\
         [An outside page](https://outside.example/lantern-private-link)\n\n![[notes/]]\n"
    );
    let site = site::Site::new(&[
        (".moss/config.toml", CONFIG),
        ("index.md", "---\ntitle: Journal\n---\nWelcome.\n"),
        ("post.md", &field_notes("")),
        ("drafted.md", &format!("---\ntitle: Drafted\ndraft: true\nmembers: subscribers\n---\nPreview.\n<!--members-only-->\n{PARA}\n")),
        ("notes/index.md", "---\ntitle: Notes\n---\nNotes.\n"),
        ("notes/first.md", "---\ntitle: First note\ndate: 2026-01-10\n---\nFirst.\n"),
    ]);
    image::RgbImage::from_fn(96, 64, |x, y| image::Rgb([200, (x % 200) as u8, (y % 200) as u8])).save(site.root.join("shot.png")).unwrap();
    site.rebuild();
    // A picture stays public under any cascade, and so do its renditions,
    // which the build makes once a page shows it.
    let public_files = |site: &site::Site| -> std::collections::BTreeMap<String, Vec<u8>> {
        public_bytes(site).into_iter().filter(|(path, _)| !path.starts_with("shot.")).collect()
    };
    let public_part_only = public_files(&site);

    site.write("post.md", &field_notes(&below));
    site.rebuild();
    let whole = public_files(&site);
    assert_eq!(whole.keys().collect::<Vec<_>>(), public_part_only.keys().collect::<Vec<_>>(), "the private part added or removed a public file");
    for (path, bytes) in &whole {
        assert!(bytes == &public_part_only[path], "the private part changed {path}");
    }
    assert_absent(&site, PARA);
    assert_absent(&site, "lantern-private-link");

    // The whole version draws what the private part asks for, and every
    // picture it names is in the output.
    let gated = site.html("_gated/post/index.html");
    assert!(gated.contains(PARA) && gated.contains("First note"), "the private part's listing is drawn");
    let named = gated.split("src=\"").skip(1).filter_map(|rest| rest.split('"').next())
        .chain(gated.split("srcset=\"").skip(1).filter_map(|rest| rest.split('"').next()).flat_map(|set| set.split(',')).filter_map(|one| one.split_whitespace().next()));
    let mut pictures = 0;
    for url in named.filter(|url| url.contains("shot")) {
        let path = url.strip_prefix('/').map_or_else(|| format!("post/{url}"), str::to_string);
        let path: Vec<&str> = path.split('/').fold(Vec::new(), |mut parts, part| { if part == ".." { parts.pop(); } else if part != "." { parts.push(part); } parts });
        assert!(site.output(&path.join("/")).exists(), "the whole version names {url}, which is not in the output");
        pictures += 1;
    }
    assert!(pictures > 0, "the whole version shows its picture");

    // A draft is published nowhere, so it has no members-only version either.
    assert!(!site.output("_gated/drafted").exists());
}
