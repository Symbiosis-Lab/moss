//! An untrusted folder runs no plugin, but its plugins' identity proofs still
//! reach the site: a proof is a value the person wrote in their own
//! `[services]` table, not code.

#[path = "it/site.rs"]
mod site;

use moss_build::plugins::install::registry_client::enforce;
use site::Site;

#[test]
fn an_untrusted_folder_still_writes_its_plugins_identity_proofs() {
    let state = tempfile::tempdir().unwrap();
    enforce::install(None, Some(state.path()));

    let site = Site::with_config("\n[services.net]\ndid = \"did:plc:abc123\"\n", &[("index.md", "---\ntitle: Home\n---\n\nHello.\n")]);
    site.write(".moss/plugins/net/manifest.json", &serde_json::json!({
        "name": "net", "version": "1.0.0",
        "contributes": {
            "site_settings": {"settings": [{"key": "did", "type": "string", "label": "DID"}]},
            "identity": [{"file": ".well-known/atproto-did", "body": "{did}"}]
        }
    }).to_string());
    enforce::set_folder_trust(&site.root, false).unwrap();
    assert!(!enforce::is_trusted(&site.root), "the folder is untrusted");

    site.rebuild();
    assert_eq!(std::fs::read_to_string(site.output(".well-known/atproto-did")).unwrap().trim(), "did:plc:abc123");
}
