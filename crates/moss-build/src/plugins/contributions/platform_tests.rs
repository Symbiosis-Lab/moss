use super::*;
use crate::plugins::types::PluginManifest;

fn keys(k: &[&str]) -> Vec<String> {
    k.iter().map(|s| s.to_string()).collect()
}

fn derivation(platforms: &[Option<&str>]) -> DerivationContribution {
    let artifact: Vec<_> = platforms
        .iter()
        .map(|p| {
            let mut a = serde_json::json!({"kind": "download", "url": "https://example.invalid/u.tgz",
                "sha256": "ab", "archive_format": "tar.gz"});
            if let Some(p) = p {
                a["platform"] = (*p).into();
            }
            a
        })
        .collect();
    serde_json::from_value(serde_json::json!({
        "accepts": ["pdf"], "noun": {"one": "doc", "other": "docs"},
        "fragments": {"text": "markdown"}, "stub": ["#text"],
        "run": {"binary": "ocr", "artifact": artifact}
    }))
    .unwrap()
}

#[test]
fn the_sentence_names_the_computers_in_plain_words() {
    assert_eq!(
        sentence("PDF OCR", &keys(&["darwin-arm64"])),
        "Not available on this computer: PDF OCR runs on Apple silicon Macs only."
    );
    assert_eq!(
        sentence("x", &keys(&["darwin-arm64", "darwin-x64", "linux-x64"])),
        "Not available on this computer: x runs on Apple silicon Macs, Intel Macs and 64-bit Linux computers only."
    );
    assert!(sentence("x", &keys(&["haiku-m68k"])).contains("haiku-m68k"), "a key moss has no words for is shown as written");
}

#[test]
fn a_listing_that_leaves_this_computer_out_is_a_refusal_and_no_listing_is_not() {
    let here = Some("darwin-arm64");
    assert!(refusal_for_listed(&keys(&["linux-x64"]), here).is_some());
    assert!(refusal_for_listed(&keys(&["linux-x64", "darwin-arm64"]), here).is_none());
    assert!(refusal_for_listed(&[], here).is_none(), "an empty list says nothing");
    assert!(refusal_for_listed(&keys(&["linux-x64"]), None).is_some(), "an unknown host matches no listed platform");
}

#[test]
fn a_derivation_is_unserved_only_when_no_artifact_fits() {
    let here = Some("darwin-arm64");
    assert_eq!(
        unserved_on(&derivation(&[Some("linux-x64"), Some("windows-x64")]), here),
        Some(keys(&["linux-x64", "windows-x64"]))
    );
    assert_eq!(unserved_on(&derivation(&[Some("darwin-arm64")]), here), None);
    assert_eq!(unserved_on(&derivation(&[None]), here), None, "no platform means every platform");
    assert_eq!(unserved_on(&derivation(&[Some("linux-x64"), None]), here), None);
}

fn manifest_with(derivation: &DerivationContribution) -> PluginManifest {
    let json = serde_json::json!({"name": "ocr", "version": "1.0.0",
        "contributes": {"derivations": {"pdf-pages": derivation}}});
    PluginManifest::parse(&json.to_string()).unwrap()
}

#[test]
fn a_manifest_with_no_artifact_for_this_computer_is_refused_and_the_others_are_not() {
    let unreachable = manifest_with(&derivation(&[Some("plan9-riscv")]));
    match manifest_refusal(&unreachable) {
        Some(Refusal::Unavailable { platforms }) => assert_eq!(platforms, keys(&["plan9-riscv"])),
        other => panic!("expected Unavailable, got {other:?}"),
    }
    let here = host().expect("a supported test host");
    assert!(manifest_refusal(&manifest_with(&derivation(&[Some(&here)]))).is_none());
    assert!(manifest_refusal(&manifest_with(&derivation(&[None]))).is_none());
    let js = PluginManifest::parse(r#"{"name":"js","version":"1.0.0","entry":"index.js"}"#).unwrap();
    assert!(manifest_refusal(&js).is_none(), "a plugin with no derivations has no program to miss");
}
