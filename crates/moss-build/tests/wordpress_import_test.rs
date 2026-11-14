//! A WordPress export through the public import seam, merged with a crawl of
//! the site it names. The export and the site are invented.
use std::{fs, path::Path};
use moss_build::vault::import::{import_source, recognise_source, ImportOptions, ImportResult, SourceKind};

fn fixture(site: &str) -> String {
    let text = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wordpress-export/export.xml")).unwrap();
    text.replace("SITE", site.trim_end_matches('/'))
}

fn runtime() -> tokio::runtime::Runtime {
    moss_build::vault::import::scrape::fetch::allow_loopback();
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

fn page(title: &str, body: &str, links: &str) -> String {
    format!("<html><head><title>{title}</title></head><body><article><h1>{title}</h1><p>{body} {links} Long enough for the content scorer to keep it as the page body.</p></article></body></html>")
}

async fn serve(server: &mut mockito::ServerGuard) -> Vec<mockito::Mock> {
    let base = server.url();
    // The pages link to each other but show no menu of their own, so the export's menu is the nav bar.
    let links = format!("<a href=\"{base}/first-light/\">First light</a> <a href=\"{base}/about/\">About</a>");
    let mut mocks = Vec::new();
    for (path, kind, body) in [
        ("/", "text/html", page("Lantern Studio", "Welcome to the studio.", &links)),
        ("/about/", "text/html", page("About", "Rendered builder text the visitors see.", &links)),
        ("/first-light/", "text/html", page("First light", &format!("The rendered first post, with <a href=\"{base}/wp-content/uploads/2022/01/price-sheet.pdf\">last year's prices</a>."), &links)),
        ("/wp-content/uploads/2022/01/price-sheet.pdf", "application/pdf", "%PDF-1.4 last year".into()),
        ("/wp-content/uploads/2024/03/harbour.jpg", "image/jpeg", "jpeg bytes".into()),
        ("/wp-content/uploads/2024/03/price-sheet.pdf", "application/pdf", "%PDF-1.4".into()),
    ] {
        mocks.push(server.mock("GET", path).with_status(200).with_header("content-type", kind).with_body(body).create_async().await);
    }
    mocks
}

fn import(rt: &tokio::runtime::Runtime, source: &str, folder: &Path, export: Option<&Path>) -> Result<ImportResult, String> {
    let options = ImportOptions { export: export.map(Path::to_path_buf), ..Default::default() };
    let completes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = completes.clone();
    let result = rt.block_on(import_source(source, folder, options, move |p| { if p.complete { count.fetch_add(1, std::sync::atomic::Ordering::Relaxed); } }));
    if result.is_ok() { assert_eq!(completes.load(std::sync::atomic::Ordering::Relaxed), 1, "one import ends once"); }
    result
}

fn read(folder: &Path, relative: &str) -> String {
    fs::read_to_string(folder.join(relative)).unwrap_or_else(|e| panic!("{relative}: {e}"))
}

fn files(folder: &Path) -> Vec<String> {
    let mut all: Vec<String> = walkdir::WalkDir::new(folder).into_iter().filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().strip_prefix(folder).unwrap().to_string_lossy().into_owned()).collect();
    all.sort();
    all
}

#[test]
fn an_export_alone_crawls_its_site_first_and_adds_what_the_crawl_cannot_see() {
    let rt = runtime();
    let mut server = rt.block_on(mockito::Server::new_async());
    let _mocks = rt.block_on(serve(&mut server));
    let temp = tempfile::tempdir().unwrap();
    let export = temp.path().join("lantern-studio.WordPress.2024-03-09.xml");
    fs::write(&export, fixture(&server.url())).unwrap();
    let out = temp.path().join("site");
    fs::create_dir(&out).unwrap();

    let recognised = rt.block_on(recognise_source(export.to_str().unwrap())).unwrap();
    assert!(matches!(recognised.kind, SourceKind::Wordpress));
    assert_eq!(recognised.title.as_deref(), Some("Lantern Studio"));
    assert_eq!(recognised.host.as_deref(), Some("127.0.0.1"));

    let result = import(&rt, export.to_str().unwrap(), &out, None).unwrap();

    // A page-builder page keeps the body the crawl saw; the export adds its menu place.
    let about = read(&out, "about.md");
    assert!(about.contains("Rendered builder text") && !about.contains("vc_row"), "{about}");
    assert!(about.contains("nav: true") && about.contains("weight: 1"), "{about}");
    let post = read(&out, "first-light.md");
    assert!(post.contains("The rendered first post") && !post.contains("stored text"), "{post}");
    for field in ["tags: [\"Workshop\", \"Paper\"]", "date: \"2024-01-09\"", "author: \"Ada Lampwright\"", "nav: true", "weight: 2"] {
        assert!(post.contains(field), "{field} in {post}");
    }
    // What only the export has is written from its stored HTML; a body that
    // spans two CDATA sections comes back whole.
    let draft = read(&out, "unfinished-notes.md");
    assert!(draft.contains("draft: true") && draft.contains(r"A draft keeps \]\]> in its text."), "{draft}");
    assert!(!draft.contains("author:"), "a login is not a name: {draft}");
    assert!(read(&out, "studio-accounts.md").contains("draft: true"));
    assert!(read(&out, "members-prices.md").contains("draft: true"));
    let project = read(&out, "project/harbour-lantern.md");
    assert!(project.contains("A lantern for the harbour."), "{project}");
    assert!(project.contains("![The lantern on the quay](../assets/imported/") && project.contains("cover: \"../assets/imported/"), "{project}");
    // The library's file of the same name lands beside the crawled one, never over it.
    let linked = post.split("[last year's prices](").nth(1).and_then(|r| r.split(')').next()).unwrap();
    assert_eq!(fs::read_to_string(out.join(linked.trim_start_matches("./"))).unwrap(), "%PDF-1.4 last year");
    assert_eq!(fs::read_dir(out.join("assets/imported")).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().contains("price-sheet")).count(), 2);
    let config = read(&out, ".moss/config.toml");
    assert!(config.contains("\"/first-lantern/\" = \"/first-light/\""), "{config}");
    let report = result.report.join("\n");
    assert!(report.contains("Comments counted, not imported: 1.") && report.contains("project 1"), "{report}");

    for file in files(&out) {
        let text = String::from_utf8_lossy(&fs::read(out.join(&file)).unwrap()).into_owned();
        for never in ["-private@mail.example", "192.0.2.7", "guestlogin", "Thrown away", "Lovely glow"] {
            assert!(!text.contains(never), "{never} in {file}");
        }
    }

    assert!(!out.join("wpcf7_contact_form").exists() && report.contains("records), not imported: 1."), "{report}");

    // Importing again writes no copy beside any page, and a field that
    // decides whether a page shows stays as the author left it.
    let published = read(&out, "unfinished-notes.md").replace("draft: true\n", "");
    fs::write(out.join("unfinished-notes.md"), &published).unwrap();
    let before = files(&out);
    let about_before = read(&out, "about.md");
    import(&rt, export.to_str().unwrap(), &out, None).unwrap();
    assert_eq!(files(&out), before);
    assert_eq!(read(&out, "about.md"), about_before);
    assert_eq!(read(&out, "unfinished-notes.md"), published);
}

#[test]
fn an_unreachable_site_is_written_from_the_export_and_said_so() {
    let rt = runtime();
    let temp = tempfile::tempdir().unwrap();
    let export = temp.path().join("export.xml");
    fs::write(&export, fixture("http://127.0.0.1:9")).unwrap();
    let out = temp.path().join("site");
    fs::create_dir(&out).unwrap();
    let result = import(&rt, export.to_str().unwrap(), &out, None).unwrap();
    assert!(result.report.iter().any(|l| l.contains("could not be reached")), "{:?}", result.report);
    assert!(read(&out, "about.md").contains("Builder shortcodes stand here."));
    assert!(read(&out, "first-light.md").contains("The stored text of the first post.\n\nA second paragraph."));
    assert!(files(&out).iter().filter(|f| f.ends_with(".md")).all(|f| !read(&out, f).contains("scrape_error")), "no stub of the unreachable site is left");
}

#[test]
fn an_export_added_to_an_address_must_come_from_that_site() {
    let rt = runtime();
    let temp = tempfile::tempdir().unwrap();
    let export = temp.path().join("export.xml");
    fs::write(&export, fixture("https://lanterns.example")).unwrap();
    let error = import(&rt, "https://other.example/", temp.path(), Some(&export)).unwrap_err();
    assert_eq!(error, "This export is from lanterns.example, not other.example.");
}

#[test]
fn an_export_over_the_size_limit_is_refused_before_it_is_read() {
    let rt = runtime();
    let temp = tempfile::tempdir().unwrap();
    let export = temp.path().join("export.xml");
    fs::write(&export, fixture("https://lanterns.example")).unwrap();
    fs::OpenOptions::new().write(true).open(&export).unwrap().set_len(200 * 1024 * 1024).unwrap();
    let error = import(&rt, export.to_str().unwrap(), temp.path(), None).unwrap_err();
    assert!(error.starts_with("This WordPress export is 200 MB; moss reads exports up to 128 MB."), "{error}");
}
