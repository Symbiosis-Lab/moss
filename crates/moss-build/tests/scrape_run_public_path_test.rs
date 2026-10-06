//! A downstream crate runs its own SSRF pre-check before calling
//! `scrape_to_folder`, importing `refuse_unsafe_scrape_url` from this old
//! path. The function itself moved into a sibling, crate-private module;
//! this test pins the re-export so a future reorganization of the scrape
//! module can't silently drop it again.

use moss_build::vault::import::scrape::run::refuse_unsafe_scrape_url;

#[test]
fn refuses_a_loopback_url_at_its_old_public_path() {
    let result = refuse_unsafe_scrape_url("http://127.0.0.1/");
    assert!(result.is_err(), "loopback URL must be refused, got {:?}", result);
}
