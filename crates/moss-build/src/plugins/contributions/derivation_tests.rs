use super::*;
use crate::plugins::types::PluginManifest;

fn manifest_json(derivation: &str) -> String {
    format!(
        r#"{{"name":"docs","version":"1.2.0",
            "contributes":{{"derivations":{{"doc-pages":{derivation}}}}}}}"#
    )
}

fn good() -> serde_json::Value {
    serde_json::json!({
        "accepts": ["xyz"],
        "noun": {"one": "doc", "other": "docs"},
        "fragments": {"text": "markdown"},
        "stub": ["#text", ""],
        "run": {"binary": "unpack", "artifact": [
            {"kind": "download", "url": "https://example.invalid/u.tgz",
             "sha256": "ab", "archive_format": "tar.gz"}
        ]}
    })
}

fn parse(v: serde_json::Value) -> Result<PluginManifest, String> {
    PluginManifest::parse(&manifest_json(&v.to_string())).map_err(|e| e.to_string())
}

fn refused(mutate: impl FnOnce(&mut serde_json::Value)) -> String {
    let mut v = good();
    mutate(&mut v);
    parse(v).expect_err("the declaration must be refused")
}

#[test]
fn a_derivation_only_plugin_with_no_entry_parses() {
    let m = parse(good()).expect("parses");
    let d = &m.contributes.unwrap().derivations.unwrap()["doc-pages"];
    assert_eq!(d.accepts, vec!["xyz"]);
    assert_eq!(d.fragments["text"], FragmentType::Markdown);
    assert_eq!(d.noun.other, "docs");
    assert_eq!(d.stub, vec!["#text", ""]);
}

#[test]
fn a_manifest_with_neither_entry_nor_derivations_is_refused() {
    let err = PluginManifest::parse(r#"{"name":"x","version":"1.0.0"}"#).unwrap_err().to_string();
    assert!(err.contains("entry"), "{err}");
}

#[test]
fn a_blank_entry_is_no_entry() {
    let err = PluginManifest::parse(r#"{"name":"x","version":"1.0.0","entry":"  "}"#).unwrap_err().to_string();
    assert!(err.contains("entry"), "{err}");
    let mut v = good();
    v["entry"] = "".into();
    let m = parse(v).unwrap();
    assert!(m.entry.is_none());
}

#[test]
fn a_parsed_manifest_reparses_to_itself() {
    let m = parse(good()).unwrap();
    let again = PluginManifest::parse(&serde_json::to_string(&m).unwrap()).unwrap();
    assert_eq!(m.contributes.unwrap().derivations, again.contributes.unwrap().derivations);
}

#[test]
fn each_invalid_declaration_is_refused_with_its_reason() {
    type Mutation = Box<dyn FnOnce(&mut serde_json::Value)>;
    let cases: Vec<(&str, Mutation, &str)> = vec![
        ("no extension", Box::new(|v| v["accepts"] = serde_json::json!([])), "accepts no file extension"),
        ("dotted extension", Box::new(|v| v["accepts"] = serde_json::json!([".xyz"])), "without a dot"),
        ("uppercase extension", Box::new(|v| v["accepts"] = serde_json::json!(["XYZ"])), "lowercase"),
        ("no fragments", Box::new(|v| v["fragments"] = serde_json::json!({})), "no fragments"),
        (
            "bad fragment name",
            Box::new(|v| v["fragments"] = serde_json::json!({"Text": "markdown"})),
            "[a-z][a-z0-9-]*",
        ),
        (
            "unknown fragment type",
            Box::new(|v| v["fragments"] = serde_json::json!({"text": "html"})),
            "unknown variant",
        ),
        ("stub names an undeclared fragment", Box::new(|v| v["stub"] = serde_json::json!(["#nope"])), "stub line"),
        ("stub has stray prose", Box::new(|v| v["stub"] = serde_json::json!(["hello"])), "stub line"),
        ("binary is a path", Box::new(|v| v["run"]["binary"] = serde_json::json!("../x")), "plain file name"),
        ("no sources", Box::new(|v| v["run"]["artifact"] = serde_json::json!([])), "no artifact sources"),
        (
            "download without a hash",
            Box::new(|v| v["run"]["artifact"][0].as_object_mut().unwrap().remove("sha256").map(|_| ()).unwrap()),
            "sha256",
        ),
        ("empty noun", Box::new(|v| v["noun"]["one"] = serde_json::json!(" ")), "noun"),
    ];
    for (name, mutate, needle) in cases {
        let err = refused(mutate);
        assert!(err.contains(needle), "{name}: expected `{needle}` in `{err}`");
    }
}

#[test]
fn an_id_outside_the_contribution_charset_is_refused() {
    let json = format!(
        r#"{{"name":"docs","version":"1.0.0","contributes":{{"derivations":{{"Doc_Pages":{}}}}}}}"#,
        good()
    );
    let err = PluginManifest::parse(&json).unwrap_err().to_string();
    assert!(err.contains("Doc_Pages"), "{err}");
}

#[test]
fn a_path_source_needs_no_hash_and_a_download_inherits_the_binary_name() {
    let mut v = good();
    v["run"]["artifact"] = serde_json::json!([
        {"kind": "path", "platform": "darwin-arm64", "binary": "/opt/unpack"},
        {"kind": "download", "url": "https://example.invalid/u.tgz", "sha256": "ab", "archive_format": "tar.gz"}
    ]);
    let m = parse(v).expect("a path source is exempt from hashing");
    let d = &m.contributes.unwrap().derivations.unwrap()["doc-pages"];
    let stack = d.as_stack("doc-pages", "1.2.0");
    assert_eq!(stack.version, "1.2.0");
    match &stack.sources[1] {
        StackSource::Download { executable, .. } => assert_eq!(executable.as_deref(), Some("unpack")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_derivation_only_plugin_loads_from_a_folder_and_can_be_consented_to() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("manifest.json"), manifest_json(&good().to_string())).unwrap();

    let plugin = crate::plugins::discovery::load_plugin(dir.path()).expect("no entry file is needed");
    assert!(plugin.manifest.entry.is_none());
    assert!(
        crate::plugins::bundled::code_hash(dir.path(), &plugin.manifest).is_some(),
        "consent is keyed by this hash; None would make the plugin impossible to allow"
    );
}

#[test]
fn changing_a_path_binary_changes_what_the_user_consented_to() {
    let dir = tempfile::TempDir::new().unwrap();
    let binary = dir.path().join("deriver");
    std::fs::write(&binary, "version one").unwrap();
    let mut v = good();
    v["run"]["artifact"] = serde_json::json!([{"kind": "path", "binary": binary}]);
    std::fs::write(dir.path().join("manifest.json"), manifest_json(&v.to_string())).unwrap();
    let plugin = crate::plugins::discovery::load_plugin(dir.path()).unwrap();
    let hash = || crate::plugins::bundled::code_hash(dir.path(), &plugin.manifest).unwrap();

    let before = hash();
    assert_eq!(hash(), before);
    // A different size, so the remembered hash of the old bytes cannot be reused.
    std::fs::write(&binary, "version two, rebuilt").unwrap();
    assert_ne!(hash(), before, "a rebuilt binary must ask for consent again");
}

#[test]
fn a_pinned_download_adds_nothing_to_the_code_hash() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("manifest.json"), manifest_json(&good().to_string())).unwrap();
    let plugin = crate::plugins::discovery::load_plugin(dir.path()).unwrap();
    assert!(crate::system::derive::binary::local_binary_hashes(dir.path(), &plugin.manifest).is_empty());
}
