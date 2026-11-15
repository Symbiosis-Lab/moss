//! The person's own site through the import entry points.
use super::*;
use crate::vault::import::scrape::fetch::{private_pair, test_server};

fn home(generator: &str) -> String {
    format!("<html><head><title>Home</title><meta name=\"generator\" content=\"{generator}\"></head><body>Home</body></html>")
}

#[tokio::test]
async fn a_site_on_the_own_network_is_recognised_as_local_when_the_person_gave_its_address() {
    let mut p = private_pair().await;
    let _home = p.own.mock("GET", "/").with_header("content-type", "text/html").with_body(home("Ghost 6")).expect(1).create_async().await;
    let own = p.own.url();

    let recognised = recognise_source(&own, Some(&own)).await.unwrap();

    assert!(matches!(recognised.kind, SourceKind::Ghost));
    assert!(recognised.local);
    assert_eq!(recognise_source(&own, None).await.unwrap_err(), "URL must not target a private-network address (127.0.0.1) other than the one you gave");
    let other = p.other.url();
    assert!(recognise_source(&own, Some(&other)).await.is_err(), "another private address is not the one given");
}

#[tokio::test]
async fn a_public_own_site_or_none_is_not_local() {
    let mut site = test_server().await;
    let _home = site.mock("GET", "/").with_header("content-type", "text/html").with_body(home("WordPress 7")).create_async().await;
    let address = site.url();

    assert!(!recognise_source(&address, Some(&address)).await.unwrap().local);
    assert!(!recognise_source(&address, None).await.unwrap().local);
}

#[tokio::test]
async fn an_import_says_the_site_is_on_the_network_and_lists_the_private_addresses_it_did_not_fetch() {
    let mut p = private_pair().await;
    let (own, other) = (p.own.url(), p.other.url());
    let _page = p.own.mock("GET", "/page").with_header("content-type", "text/html")
        .with_body(format!("<html><body><article><p>Page body, long enough to be extracted as content by the generic scorer.</p><img src=\"{other}/x.png\" alt=\"x\"></article></body></html>"))
        .create_async().await;
    let image = p.other.mock("GET", "/x.png").with_header("content-type", "image/png").with_body("png").expect(0).create_async().await;
    let tmp = tempfile::tempdir().unwrap();

    let result = import_source(&format!("{own}/page"), tmp.path(), ImportOptions { own_site: Some(own.clone()), ..Default::default() }, |_| {}).await.unwrap();

    assert_eq!(result.report, ["127.0.0.1 is on your network.".to_string(), format!("Not fetched: {other}/x.png")]);
    image.assert_async().await;
}

#[tokio::test]
async fn a_ghost_image_missing_from_the_export_is_fetched_from_the_own_site_only() {
    crate::system::seed_app_version("0.15.23");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ghost-bundle");
    for given in [true, false] {
        let mut p = private_pair().await;
        let own = p.own.url();
        let images = p.own.mock("GET", mockito::Matcher::Regex("^/content/images/".into()))
            .with_header("content-type", "image/svg+xml").with_body("<svg xmlns=\"http://www.w3.org/2000/svg\"/>")
            .expect(if given { 1 } else { 0 }).create_async().await;
        let tmp = tempfile::tempdir().unwrap();
        let bundle = tmp.path().join("bundle");
        for entry in walkdir::WalkDir::new(&fixture).into_iter().filter_map(Result::ok).filter(|e| e.file_type().is_file()) {
            let to = bundle.join(entry.path().strip_prefix(&fixture).unwrap());
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(entry.path(), to).unwrap();
        }
        std::fs::write(bundle.join("site.json"), serde_json::json!({ "url": own }).to_string()).unwrap();
        std::fs::remove_file(bundle.join("content/images/cover.svg")).unwrap();
        let out = tmp.path().join("site");
        std::fs::create_dir(&out).unwrap();
        let options = ImportOptions { private_members_dir: Some(tmp.path().join("private")), own_site: given.then(|| own.clone()), ..Default::default() };

        let result = import_source(bundle.to_str().unwrap(), &out, options, |_| {}).await.unwrap();

        images.assert_async().await;
        let listed = result.report.iter().any(|l| l.starts_with(&format!("Not fetched: {own}/content/images/")));
        assert_eq!(listed, !given, "{:?}", result.report);
        assert_eq!(result.report.first().is_some_and(|l| l == "127.0.0.1 is on your network."), given, "{:?}", result.report);
    }
}
