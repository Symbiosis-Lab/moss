//! Synthetic export coverage through the public native/CLI import seam.
use std::{fs, path::Path, io::Write};
use moss_build::vault::import::{import_source, recognise_source, ImportOptions, SourceKind};
#[path = "support/site.rs"] mod site;
const PRIVATE: &str = "copper-private-body-928441";
fn fixture() -> std::path::PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ghost-bundle") }
fn copy_fixture(to: &Path) {
    for entry in walkdir::WalkDir::new(fixture()) {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            let path = to.join(entry.path().strip_prefix(fixture()).unwrap());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::copy(entry.path(), path).unwrap();
        }
    }
}
fn zip_fixture(path: &Path, unsafe_entry: bool) {
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for entry in walkdir::WalkDir::new(fixture()) {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            zip.start_file(format!("export/{}", entry.path().strip_prefix(fixture()).unwrap().display()), zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(&fs::read(entry.path()).unwrap()).unwrap();
        }
    }
    if unsafe_entry { zip.start_file("../escape.md", zip::write::SimpleFileOptions::default()).unwrap(); zip.write_all(b"bad").unwrap(); }
    zip.finish().unwrap();
}
/// The moss the importer names as a members site's floor; a moss below a
/// folder's floor refuses to write its config, so the tests run as this one.
const MEMBERS_MOSS: &str = "0.15.23";
fn runtime() -> tokio::runtime::Runtime {
    moss_build::system::seed_app_version(MEMBERS_MOSS);
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

#[test]
fn directory_and_zip_import_keep_content_and_retain_readers_privately() {
    for archive in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join(if archive { "arbitrary-name.data" } else { "bundle" });
        if archive { zip_fixture(&input, false); } else { copy_fixture(&input); }
        let out = temp.path().join("site"); fs::create_dir(&out).unwrap();
        let private = temp.path().join("private");
        let rt = runtime();
        let recognised = rt.block_on(recognise_source(input.to_str().unwrap())).unwrap();
        assert!(matches!(recognised.kind, SourceKind::Ghost));
        assert_eq!(recognised.host.as_deref(), Some("journal.example"));
        assert_eq!(recognised.title.as_deref(), Some("Copper Journal"));
        let result = rt.block_on(import_source(input.to_str().unwrap(), &out, ImportOptions { private_members_dir: Some(private.clone()), ..Default::default() }, |_| {})).unwrap();
        assert_eq!(result.scrape.total_pages, 7);
        assert!(fs::read_to_string(out.join("members.md")).unwrap().contains("<!--members-only-->"));
        assert!(fs::read_to_string(out.join("members.md")).unwrap().contains(PRIVATE));
        assert_eq!(moss_core::frontmatter::frontmatter_map(&fs::read_to_string(out.join("paid.md")).unwrap())["members"].as_str(), Some("paid"));
        assert!(fs::read_to_string(out.join("draft.md")).unwrap().contains("draft: true"));
        assert!(out.join("tag/field-notes.md").exists());
        assert!(fs::read_dir(out.join("assets/imported")).unwrap().next().is_some());
        let public = fs::read_to_string(out.join("public.md")).unwrap();
        assert!(public.contains("cover: \"./assets/imported/") && public.contains("](./assets/imported/"), "{public}");
        let config = fs::read_to_string(out.join(".moss/config.toml")).unwrap();
        assert!(config.contains("/former/")); assert!(config.contains("/author/journal-editor/"));
        assert!(!config.contains("?page")); assert!(!config.contains("elsewhere.example"));
        for entry in walkdir::WalkDir::new(&out) {
            let entry = entry.unwrap(); if !entry.file_type().is_file() || entry.path().components().any(|c| c.as_os_str() == "_gated") { continue; }
            let bytes = fs::read(entry.path()).unwrap(); let text = String::from_utf8_lossy(&bytes);
            for secret in ["private-reader@", "muted-reader@", "never-copy-"] { assert!(!text.contains(secret), "{}", entry.path().display()); }
        }
        let handoff = result.subscribers.unwrap(); assert_eq!(handoff.files.len(), 3);
        if archive { fs::remove_file(&input).unwrap(); } else { fs::remove_dir_all(&input).unwrap(); }
        let private = fs::canonicalize(&private).unwrap();
        for path in handoff.files {
            assert!(path.starts_with(&private)); assert!(fs::read(path.clone()).is_ok());
            #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600); assert_eq!(fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700); }
        }
        drop(rt);
        let config = format!("{config}\n[site]\nlang = 'en'\nurl = 'https://journal.example/'\narchival = true\n[ai]\nllms_txt = true\n");
        fs::write(out.join(".moss/config.toml"), config).unwrap();
        site::build_sync(out.to_str().unwrap(), moss_build::build::BuildTrigger::Full).unwrap();
        let stage = out.join(".moss/build.nosync/staging");
        for entry in walkdir::WalkDir::new(&stage) {
            let entry = entry.unwrap(); if !entry.file_type().is_file() || entry.path().components().any(|c| c.as_os_str() == "_gated") { continue; }
            let bytes = fs::read(entry.path()).unwrap();
            assert!(!bytes.windows(PRIVATE.len()).any(|v| v == PRIVATE.as_bytes()), "{}", entry.path().display());
        }
        assert!(fs::read_to_string(stage.join("draft/index.html")).unwrap().contains("noindex"));
        assert!(!fs::read_to_string(stage.join("rss.xml")).unwrap().contains("draft-not-published-827351"));
        assert!(fs::read_to_string(stage.join("rss.xml")).unwrap().contains("<guid isPermaLink=\"false\">65a1b2c3d4e5f60718293a4b</guid>"));
        assert!(stage.join("former/index.html").exists());
    }
}

#[test]
fn rejects_archive_traversal_and_private_storage_inside_project() {
    let temp = tempfile::tempdir().unwrap();
    let zip = temp.path().join("unsafe.zip"); zip_fixture(&zip, true);
    let out = temp.path().join("site"); fs::create_dir(&out).unwrap();
    let rt = runtime();
    assert!(rt.block_on(import_source(zip.to_str().unwrap(), &out, Default::default(), |_| {})).unwrap_err().contains("Unsafe path"));
    let error = rt.block_on(import_source(fixture().to_str().unwrap(), &out, ImportOptions { private_members_dir: Some(out.join("readers")), ..Default::default() }, |_| {})).unwrap_err();
    assert!(error.contains("outside the project")); assert!(!out.join("readers").exists());
}

#[test]
fn collision_links_follow_the_shared_reserved_destination() {
    let temp = tempfile::tempdir().unwrap(); let out = temp.path().join("site"); fs::create_dir(&out).unwrap();
    fs::write(out.join("members.md"), "An author's existing page").unwrap();
    runtime().block_on(import_source(fixture().to_str().unwrap(), &out, ImportOptions { private_members_dir: Some(temp.path().join("private")), ..Default::default() }, |_| {})).unwrap();
    assert_eq!(fs::read_to_string(out.join("members.md")).unwrap(), "An author's existing page");
    assert!(out.join("members 2.md").exists());
    let config = fs::read_to_string(out.join(".moss/config.toml")).unwrap();
    assert!(config.contains("/members-2/")); assert!(config.contains("/members-2/#details"));
    assert!(!config.contains("javascript:")); assert!(!config.contains("/pattern/"));
    assert!(fs::read_to_string(out.join("public.md")).unwrap().contains("members%202"));
}

/// A moss without members would publish members pages whole, so an import
/// that writes one names the first moss with them as the folder's floor: a
/// moss below it, or from before the floor existed, refuses to publish.
#[test]
fn an_import_with_members_pages_sets_the_folder_floor() {
    let cli = include_str!("../../moss-cli/Cargo.toml");
    let version = cli.lines().find_map(|l| l.strip_prefix("version = ")).unwrap().trim_matches('"');
    assert_eq!(moss_build::system::floor_met(MEMBERS_MOSS, version), Some(true), "the floor names a released or current moss");
    for (existing, floor) in [
        (None, MEMBERS_MOSS),
        (Some("schema_version = 6\n\n[site]\nlang = 'en'\n"), MEMBERS_MOSS),
        (Some("schema_version = 7\nmin_moss_version = \"0.15.1\"\n"), MEMBERS_MOSS),
    ] {
        let temp = tempfile::tempdir().unwrap(); let out = temp.path().join("site"); fs::create_dir_all(out.join(".moss")).unwrap();
        if let Some(text) = existing { fs::write(out.join(".moss/config.toml"), text).unwrap(); }
        runtime().block_on(import_source(fixture().to_str().unwrap(), &out, ImportOptions { private_members_dir: Some(temp.path().join("private")), ..Default::default() }, |_| {})).unwrap();
        let config = moss_build::config::ConfigFile::parse(&fs::read_to_string(out.join(".moss/config.toml")).unwrap()).unwrap();
        assert_eq!(config.min_moss_version(), Some(floor));
        assert_eq!(config.schema_version_ahead(), None);
        let raw: toml::Table = toml::from_str(&fs::read_to_string(out.join(".moss/config.toml")).unwrap()).unwrap();
        assert_eq!(raw["schema_version"].as_integer(), Some(7), "the stamp a moss from before the floor refuses");
    }
}

#[test]
fn saved_ghost_lock_page_becomes_an_empty_restricted_source_with_a_report() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("locked.html"); let out = temp.path().join("site"); fs::create_dir(&out).unwrap();
    fs::write(&source, r#"<html><head><title>Locked article</title><meta name="generator" content="Ghost 6"></head><body><article><aside class="gh-post-upgrade-cta"><h2>This post is for subscribers only</h2><a data-portal="signup">Subscribe now</a></aside></article></body></html>"#).unwrap();
    let result = runtime().block_on(import_source(source.to_str().unwrap(), &out, Default::default(), |_| {})).unwrap();
    assert_eq!(result.scrape.restricted_pages, 1); assert!(!result.report.is_empty());
    let page = fs::read_to_string(out.join("Locked article.md")).unwrap();
    let parsed = moss_core::frontmatter::parse(&page);
    assert_eq!(parsed.frontmatter["members"].as_str(), Some("subscribers"));
    assert!(parsed.body.trim().is_empty());
    assert!(!page.contains("Subscribe now"));
}

#[test]
fn shared_url_entry_recognises_a_lock_page_and_downloads_missing_export_media() {
    use std::{io::Read, net::TcpListener, time::{Duration, Instant}};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut served = 0;
        let mut requests = Vec::new();
        while served < 5 && Instant::now() < deadline {
            let (mut stream, _) = match listener.accept() {
                Ok(pair) => pair,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => { std::thread::sleep(Duration::from_millis(10)); continue; }
                Err(e) => panic!("{e}"),
            };
            // macOS hands the listener's non-blocking mode to the accepted socket,
            // which would make the read below return before the request arrives.
            stream.set_nonblocking(false).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut request = [0; 2048]; let size = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..size]); assert!(request.starts_with("GET "));
            let image = request.contains("/content/images/");
            requests.push(request.lines().next().unwrap().to_string());
            let (body, mime) = if image { (fs::read(fixture().join("content/images/cover.svg")).unwrap(), "image/svg+xml") }
            else if served == 3 { (br#"<html><head><title>WordPress homepage</title><meta name="generator" content="WordPress 7"></head><body>Home</body></html>"#.to_vec(), "text/html") }
            else if served == 4 || !request.starts_with("GET / HTTP/") { (b"<html><head><title>Ordinary page</title></head><body>Article</body></html>".to_vec(), "text/html") }
            else { (br#"<html><head><title>Locked article</title><meta name="generator" content="Ghost 6"></head><body><aside class="gh-post-upgrade-cta">Locked content</aside></body></html>"#.to_vec(), "text/html") };
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
            stream.write_all(&body).unwrap(); served += 1;
        }
        (served, requests)
    });
    let temp = tempfile::tempdir().unwrap(); let out = temp.path().join("site"); fs::create_dir(&out).unwrap();
    moss_build::vault::import::scrape::fetch::allow_loopback();
    let rt = runtime();
    let recognised = rt.block_on(recognise_source(&format!("{base}deep/article/?view=full#section")));
    let imported = rt.block_on(import_source(&base, &out, Default::default(), |_| {}));
    let input = temp.path().join("bundle"); copy_fixture(&input);
    fs::write(input.join("site.json"), serde_json::json!({"url": base}).to_string()).unwrap();
    fs::remove_file(input.join("content/images/cover.svg")).unwrap();
    let media = rt.block_on(import_source(input.to_str().unwrap(), &out, ImportOptions { private_members_dir: Some(temp.path().join("private")), ..Default::default() }, |_| {}));
    let wordpress = rt.block_on(recognise_source(&format!("{base}another/post/")));
    let ordinary = rt.block_on(recognise_source(&format!("{base}plain/article/?q=1")));
    let (served, requests) = server.join().unwrap();
    let recognised = recognised.unwrap();
    assert!(matches!(recognised.kind, SourceKind::Ghost));
    assert_eq!(recognised.title.as_deref(), Some("Locked article"));
    assert_eq!(recognised.host.as_deref(), Some("127.0.0.1"));
    assert!(matches!(wordpress.unwrap().kind, SourceKind::Wordpress));
    assert!(matches!(ordinary.unwrap().kind, SourceKind::Site));
    for i in [0, 3, 4] { assert_eq!(requests[i], "GET / HTTP/1.1"); }
    assert_eq!(imported.unwrap().scrape.restricted_pages, 1);
    assert!(media.unwrap().scrape.success); assert_eq!(served, 5);
    assert!(fs::read_dir(out.join("assets/imported")).unwrap().next().is_some());
    let page = fs::read_to_string(out.join("public.md")).unwrap();
    assert!(page.contains("](./assets/imported/"), "a downloaded image is named like a bundled one: {page}");
}

#[test]
fn saved_html_recognition_remains_a_file_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("saved.html");
    fs::write(&file, "<html><head><title>Saved page</title><meta name=generator content=Ghost></head></html>").unwrap();
    let recognised = runtime().block_on(recognise_source(file.to_str().unwrap())).unwrap();
    assert!(matches!(recognised.kind, SourceKind::File));
    assert_eq!(recognised.title.as_deref(), Some("saved.html"));
    assert_eq!(recognised.host, None);
}

/// The desktop flow: an address import first, where a members-only post
/// shows only Ghost's lock box, then the export dropped on the folder later.
/// The post's page takes the export's text, keeps it members-only, the
/// folder's moss floor is set, and no built file shows the private text.
#[test]
fn an_address_import_then_the_export_keeps_the_members_text_private() {
    let rt = runtime();
    moss_build::vault::import::scrape::fetch::allow_loopback();
    let mut server = rt.block_on(mockito::Server::new_async());
    let base = server.url();
    let page = |body: &str| format!("<html><head><title>Copper Journal</title></head><body><article><h1>Members</h1>{body}</article></body></html>");
    let _home = rt.block_on(server.mock("GET", "/").with_header("content-type", "text/html")
        .with_body(page(&format!("<p><a href=\"{base}/members/\">Members</a> and enough words for the content scorer to keep this page body.</p>"))).create_async());
    let _members = rt.block_on(server.mock("GET", "/members/").with_header("content-type", "text/html")
        .with_body(page("<p>The public opening.</p><aside class=\"gh-post-upgrade-cta\">Subscribe to read</aside>")).create_async());
    let temp = tempfile::tempdir().unwrap();
    let bundle = temp.path().join("bundle"); copy_fixture(&bundle);
    fs::write(bundle.join("site.json"), format!("{{\"url\":\"{base}/\",\"title\":\"Copper Journal\"}}")).unwrap();
    let out = temp.path().join("site"); fs::create_dir(&out).unwrap();

    let options = || ImportOptions { recursive: true, private_members_dir: Some(temp.path().join("private")), ..Default::default() };
    rt.block_on(import_source(&format!("{base}/"), &out, options(), |_| {})).unwrap();
    assert!(!fs::read_to_string(out.join("members.md")).unwrap().contains(PRIVATE));
    rt.block_on(import_source(bundle.to_str().unwrap(), &out, options(), |_| {})).unwrap();

    let members = fs::read_to_string(out.join("members.md")).unwrap();
    assert!(members.contains(PRIVATE) && members.contains("members: \"subscribers\""), "{members}");
    assert!(!out.join("members 2.md").exists());
    let config = fs::read_to_string(out.join(".moss/config.toml")).unwrap();
    assert!(config.contains("min_moss_version"), "{config}");
    drop(rt);
    fs::write(out.join(".moss/config.toml"), format!("{config}\n[site]\nlang = 'en'\n")).unwrap();
    site::build_sync(out.to_str().unwrap(), moss_build::build::BuildTrigger::Full).unwrap();
    for entry in walkdir::WalkDir::new(out.join(".moss/build.nosync/staging")) {
        let entry = entry.unwrap(); if !entry.file_type().is_file() || entry.path().components().any(|c| c.as_os_str() == "_gated") { continue; }
        let bytes = fs::read(entry.path()).unwrap();
        assert!(!bytes.windows(PRIVATE.len()).any(|v| v == PRIVATE.as_bytes()), "{}", entry.path().display());
    }
}

/// A page written empty that takes an export's text takes the export's
/// visibility with it, over its own: an empty `draft: false` page whose post
/// is a draft stays unpublished, and an empty `members: public` page whose
/// post is members-only becomes restricted, with the folder's floor set.
#[test]
fn an_empty_page_takes_the_exports_visibility_with_its_text() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("site"); fs::create_dir(&out).unwrap();
    for (slug, own) in [("draft", "draft: false"), ("members", "members: \"public\"")] {
        fs::write(out.join(format!("{slug}.md")), format!("---\ntitle: \"Old\"\norigin: \"https://journal.example/{slug}/\"\n{own}\n---\n\n")).unwrap();
    }
    let rt = runtime();
    rt.block_on(import_source(fixture().to_str().unwrap(), &out, ImportOptions { private_members_dir: Some(temp.path().join("private")), ..Default::default() }, |_| {})).unwrap();
    let draft = fs::read_to_string(out.join("draft.md")).unwrap();
    assert!(draft.contains("draft: true") && draft.contains("draft-not-published-827351"), "{draft}");
    let members = fs::read_to_string(out.join("members.md")).unwrap();
    assert_eq!(moss_core::frontmatter::frontmatter_map(&members)["members"].as_str(), Some("subscribers"), "{members}");
    assert!(members.contains(PRIVATE), "{members}");
    let config = fs::read_to_string(out.join(".moss/config.toml")).unwrap();
    assert!(config.contains("min_moss_version"), "{config}");
    drop(rt);
    fs::write(out.join(".moss/config.toml"), format!("{config}\n[site]\nlang = 'en'\nurl = 'https://journal.example/'\n")).unwrap();
    site::build_sync(out.to_str().unwrap(), moss_build::build::BuildTrigger::Full).unwrap();
    let stage = out.join(".moss/build.nosync/staging");
    for entry in walkdir::WalkDir::new(&stage) {
        let entry = entry.unwrap(); if !entry.file_type().is_file() || entry.path().components().any(|c| c.as_os_str() == "_gated") { continue; }
        let text = String::from_utf8_lossy(&fs::read(entry.path()).unwrap()).into_owned();
        assert!(!text.contains(PRIVATE), "{}", entry.path().display());
    }
    // A draft is built for preview only: kept out of search engines and the feed, never published.
    assert!(fs::read_to_string(stage.join("draft/index.html")).unwrap().contains("noindex"));
    assert!(!fs::read_to_string(stage.join("rss.xml")).unwrap().contains("draft-not-published-827351"));
}
