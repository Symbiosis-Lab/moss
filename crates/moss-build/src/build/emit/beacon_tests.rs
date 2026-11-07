use super::*;
use crate::types::content::SiteHashes;
use tempfile::tempdir;

fn vault_with_state(state: &str) -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".moss")).unwrap();
    std::fs::write(dir.path().join(".moss/state.toml"), state).unwrap();
    dir
}

#[test]
fn moss_hosted_site_emits_the_beacon_file_with_its_endpoint() {
    let vault = vault_with_state("[deployment]\nsite_id = \"blog\"\ndeploy_method = \"moss\"\n");
    let out = tempdir().unwrap();
    let mut pending = PendingManifest::new(SiteHashes::default());

    let p = vault.path().to_str().unwrap();
    let deploy = crate::build::site_config::get_domain_config(p).unwrap();
    emit(&deploy, p, out.path(), &mut pending).unwrap();

    let js = std::fs::read_to_string(out.path().join("_moss/js/beacon.js"))
        .expect("a moss-hosted build writes _moss/js/beacon.js");
    assert!(js.contains("/api/sites/blog/beacon\""), "endpoint missing from:\n{js}");
}

#[test]
fn site_without_a_site_id_emits_no_beacon_file() {
    let vault = vault_with_state("[deployment]\ndeploy_method = \"moss\"\n");
    let out = tempdir().unwrap();
    let mut pending = PendingManifest::new(SiteHashes::default());

    let p = vault.path().to_str().unwrap();
    let deploy = crate::build::site_config::get_domain_config(p).unwrap();
    emit(&deploy, p, out.path(), &mut pending).unwrap();

    assert!(!out.path().join("_moss/js/beacon.js").exists());
}

#[test]
fn script_sends_address_without_fragment_referrer_and_width_and_no_derived_fields() {
    let vault = vault_with_state("[deployment]\nsite_id = \"blog\"\ndeploy_method = \"moss\"\n");
    let out = tempdir().unwrap();
    let mut pending = PendingManifest::new(SiteHashes::default());

    let p = vault.path().to_str().unwrap();
    let deploy = crate::build::site_config::get_domain_config(p).unwrap();
    emit(&deploy, p, out.path(), &mut pending).unwrap();

    let js = std::fs::read_to_string(out.path().join("_moss/js/beacon.js")).unwrap();
    assert!(js.contains("{ url: address(), referrer: document.referrer || null, screen_width: innerWidth }"), "pageview body:\n{js}");
    assert!(js.contains("{ url: address(), event: tracked.getAttribute('data-track') }"), "click body:\n{js}");
    assert!(js.contains("return location.origin + location.pathname + location.search;"), "address without its fragment:\n{js}");
    for derived in ["location.href", "URLSearchParams", "screen_bucket", "utm_source", "utm_medium", "path:"] {
        assert!(!js.contains(derived), "script still derives or sends {derived}:\n{js}");
    }
}

#[test]
fn script_retries_a_bounded_number_of_times_with_one_id_and_no_headers() {
    let vault = vault_with_state("[deployment]\nsite_id = \"blog\"\ndeploy_method = \"moss\"\n");
    let out = tempdir().unwrap();
    let mut pending = PendingManifest::new(SiteHashes::default());

    let p = vault.path().to_str().unwrap();
    let deploy = crate::build::site_config::get_domain_config(p).unwrap();
    emit(&deploy, p, out.path(), &mut pending).unwrap();

    let js = std::fs::read_to_string(out.path().join("_moss/js/beacon.js")).unwrap();
    assert!(js.contains("body.id = newId();\n    send(body, body.id ? 0 : 2);"), "one id per event, set before the first try; no retry without one:\n{js}");
    assert!(js.contains("if (attempt < 2 && waiting < 10)"), "at most two retries, ten waiting:\n{js}");
    assert!(js.contains("Math.min(seconds || 2 << attempt, 30)"), "Retry-After, capped at 30 s:\n{js}");
    // Any request header would make the beacon a preflighted CORS request.
    assert!(!js.contains("headers:"), "the beacon must send no request headers:\n{js}");
    // The id is made fresh for each event and never kept; other parts of the
    // script may use storage for their own keys, so only the id's maker is checked.
    let maker = js.split("function newId() {").nth(1).and_then(|rest| rest.split("\n  }\n").next()).expect("the id maker");
    assert!(maker.contains("crypto.getRandomValues"), "id maker:\n{maker}");
    for stored in ["Storage", "cookie", "indexedDB"] {
        assert!(!maker.contains(stored), "the id must live only in memory, found {stored}:\n{maker}");
    }
}

#[test]
fn script_sends_engagement_links_out_and_downloads_to_their_own_path() {
    let vault = vault_with_state("[deployment]\nsite_id = \"blog\"\ndeploy_method = \"moss\"\n");
    let out = tempdir().unwrap();
    let mut pending = PendingManifest::new(SiteHashes::default());

    let p = vault.path().to_str().unwrap();
    let deploy = crate::build::site_config::get_domain_config(p).unwrap();
    emit(&deploy, p, out.path(), &mut pending).unwrap();

    let js = std::fs::read_to_string(out.path().join("_moss/js/beacon.js")).unwrap();
    // A host that predates these rows answers this path 404 instead of counting them as visits.
    assert!(js.contains("fetch(body.kind ? endpoint + '/v2' : endpoint,"), "rows with a kind go to /v2:\n{js}");
    // One engagement event per pageview: a quarter of the page and a time range, nothing finer.
    assert!(js.contains("kind: 'engage', scroll: Math.max(25, Math.ceil(deepest * 4) * 25), visible: timeRange(onScreenMs / 1e3)"), "engage body:\n{js}");
    // A link out sends only the host, a download only the path.
    assert!(js.contains("kind: 'out', event: link.hostname"), "out body:\n{js}");
    assert!(js.contains("kind: 'download', event: link.pathname"), "download body:\n{js}");
    for leak in ["link.href", "link.search", "innerText", "textContent", "Storage", "cookie"] {
        assert!(!js.contains(leak), "the script must not send or keep {leak}:\n{js}");
    }
}
