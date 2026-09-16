use super::*;

fn download(platform: Option<&str>, sha256: Option<&str>) -> StackSource {
    StackSource::Download {
        platform: platform.map(str::to_string),
        url: "https://example.invalid/artifact".to_string(),
        sha256: sha256.map(str::to_string),
        archive_format: "dmg".to_string(),
        executable: None,
    }
}

fn path_source(platform: Option<&str>) -> StackSource {
    StackSource::Path { platform: platform.map(str::to_string), binary: "ipfs".to_string() }
}

fn stack(sources: Vec<StackSource>) -> StackContribution {
    StackContribution {
        id: "onionpress".to_string(),
        display_name: None,
        version: "v1".to_string(),
        sources,
        start: vec![],
        stop: vec![],
        uninstall: vec![],
        recover: vec![],
        self_heal_grace_secs: None,
        preserve_on_uninstall: vec![],
        hooks: StackHooks::default(),
    }
}

#[test]
fn a_download_source_without_a_sha256_is_refused() {
    let s = stack(vec![download(None, None)]);
    let err = s.validate().expect_err("a download without sha256 must be refused");
    assert!(matches!(err, StackDeclarationError::MissingSha256 { .. }), "{err:?}");
}

#[test]
fn a_path_source_needs_no_sha256() {
    let s = stack(vec![path_source(None)]);
    assert_eq!(s.validate(), Ok(()));
}

#[test]
fn a_declaration_with_no_sources_is_refused() {
    let s = stack(vec![]);
    let err = s.validate().expect_err("no sources must be refused");
    assert!(matches!(err, StackDeclarationError::NoSources { .. }), "{err:?}");
}

/// The bytes a plugin author actually writes — asserted against a JSON
/// LITERAL, not a serialize-then-parse round trip, which would stay green
/// under any consistent renaming of the `kind` tag.
#[test]
fn the_shipped_json_shape_parses() {
    let json = r#"{
        "id": "onionpress",
        "display_name": "OnionPress",
        "version": "v2.4.110-moss.2",
        "sources": [
            {
                "kind": "download",
                "platform": "darwin-arm64",
                "url": "https://github.com/guoliu/onionpress/releases/download/v2.4.110-moss.2/onionpress.dmg",
                "sha256": "68e46954f802495844cbd69f42c64571d03bb2ba2797bdd7f90885cd0ac94c88",
                "archive_format": "dmg",
                "executable": "OnionPress.app/Contents/MacOS/onionpress"
            },
            { "kind": "path", "binary": "ipfs" }
        ],
        "start": ["start"],
        "stop": ["quit"],
        "recover": [["start"], ["quit", "start"]],
        "self_heal_grace_secs": 900,
        "preserve_on_uninstall": [".credentials"],
        "hooks": { "health": true, "provision": true }
    }"#;
    let stack: StackContribution = serde_json::from_str(json).expect("the shipped shape parses");
    assert_eq!(stack.id, "onionpress");
    assert_eq!(stack.display_name.as_deref(), Some("OnionPress"));
    assert_eq!(stack.version, "v2.4.110-moss.2");
    assert_eq!(stack.start, vec!["start".to_string()]);
    assert_eq!(stack.stop, vec!["quit".to_string()]);
    assert_eq!(
        stack.recover,
        vec![vec!["start".to_string()], vec!["quit".to_string(), "start".to_string()]]
    );
    assert_eq!(stack.self_heal_grace_secs, Some(900));
    assert_eq!(stack.preserve_on_uninstall, vec![".credentials".to_string()]);
    assert!(stack.hooks.health);
    assert!(stack.hooks.provision);
    match &stack.sources[0] {
        StackSource::Download { platform, url, sha256, archive_format, executable } => {
            assert_eq!(platform.as_deref(), Some("darwin-arm64"));
            assert!(url.contains("onionpress.dmg"));
            assert_eq!(
                sha256.as_deref(),
                Some("68e46954f802495844cbd69f42c64571d03bb2ba2797bdd7f90885cd0ac94c88")
            );
            assert_eq!(archive_format, "dmg");
            assert_eq!(executable.as_deref(), Some("OnionPress.app/Contents/MacOS/onionpress"));
        }
        other => panic!("expected a download source, got {other:?}"),
    }
    assert_eq!(
        stack.sources[1],
        StackSource::Path { platform: None, binary: "ipfs".to_string() },
        "the path shape ADR-080's ruling addition exists for — no source needs a sha256 to be valid"
    );
    stack.validate().expect("the shipped OnionPress declaration is valid, download and path alike");
}

#[test]
fn artifact_source_takes_the_first_source_for_this_platform_and_absent_means_any() {
    let s = stack(vec![
        download(Some("linux-x64"), Some("a".repeat(64).as_str())),
        download(None, Some("b".repeat(64).as_str())),
        download(Some("darwin-arm64"), Some("c".repeat(64).as_str())),
    ]);

    let wildcard = &s.sources[1];
    assert_eq!(artifact_source(&s, Some("darwin-arm64")), Some(wildcard));
    assert_eq!(artifact_source(&s, Some("windows-x64")), Some(wildcard));
    assert_eq!(artifact_source(&s, None), Some(wildcard));

    let without_wildcard = stack(vec![
        download(Some("linux-x64"), Some("a".repeat(64).as_str())),
        download(Some("darwin-arm64"), Some("c".repeat(64).as_str())),
    ]);
    assert_eq!(artifact_source(&without_wildcard, Some("darwin-arm64")), Some(&without_wildcard.sources[1]));
}
