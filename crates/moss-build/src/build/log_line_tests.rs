use super::*;

#[test]
fn the_site_fingerprint_is_logged_once_per_change_per_site() {
    let (one, two) = ("/sites/log-line-tests-one", "/sites/log-line-tests-two");
    let a = "site fingerprint: pages=1";
    assert!(fingerprint_changed(one, a));
    assert!(!fingerprint_changed(one, a), "the same fingerprint twice logs once");
    assert!(fingerprint_changed(two, a), "another site with the same fingerprint still logs");
    assert!(fingerprint_changed(one, "site fingerprint: pages=2"));
    assert!(fingerprint_changed(one, a), "a change back logs again");
}
