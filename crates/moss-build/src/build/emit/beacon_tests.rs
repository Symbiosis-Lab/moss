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
