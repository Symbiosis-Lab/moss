use super::*;
use std::path::Path;

fn entry(id: &str, version: &str) -> IndexEntry {
    serde_json::from_value(serde_json::json!({
        "type": "plugin",
        "id": id,
        "display_name": id,
        "version": version,
        "download_url": "https://example.invalid/x.zip",
        "sha256": "0".repeat(64),
    }))
    .unwrap()
}

/// S2's one-way door, characterized before the type moved: a plugin receipt
/// on disk today has exactly these five keys, nothing more. A unified type
/// that adds `source`/`receipt_on` and forgets to skip them when absent
/// would still deserialize fine here — and would still be a format change.
#[test]
fn a_plugin_receipt_keeps_its_exact_on_disk_keys() {
    let dir = tempfile::tempdir().unwrap();
    let entry = entry("stranger", "1.0.0");
    write(dir.path(), &entry, vec!["a.js".to_string()]).unwrap();
    let raw = fs::read_to_string(dir.path().join(RECEIPT)).unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let obj = value.as_object().unwrap();
    let keys: std::collections::BTreeSet<&str> = obj.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        ["id", "version", "sha256", "download_url", "files"]
            .into_iter()
            .collect(),
        "unexpected key set: {raw}"
    );
}

/// A receipt from before `files` existed must fall back to "did not record",
/// which is a different value from "recorded nothing" — the first keeps
/// everything on the next update, the second would delete it all.
#[test]
fn a_receipt_from_before_files_existed_reads_as_did_not_record() {
    let parsed: Receipt = serde_json::from_str(
        r#"{"id":"stranger","version":"1.0.0","sha256":"0","download_url":"https://example.invalid/x.zip"}"#,
    )
    .unwrap();
    assert_eq!(parsed.files, None, "no files key at all must not read as an empty list");
    assert_ne!(
        parsed.files,
        Some(vec![]),
        "did-not-record and recorded-nothing must stay distinguishable"
    );
}

/// The load-bearing default: an install with no receipt is exactly
/// the population the pin never reached, so "unknown" must read as stale.
#[test]
fn a_missing_or_mismatched_stamp_is_stale_against_a_real_pin() {
    let (pin_version, pin_sha256) = ("v2.4.110-moss.2", "a".repeat(64));
    assert!(
        stale_against_pin(None, pin_version, &pin_sha256),
        "no stamp must count as stale"
    );
    let old = Receipt::at_bringup("v2.4.110-moss.1".into(), "b".repeat(64), Origin::Pinned);
    assert!(stale_against_pin(Some(&old), pin_version, &pin_sha256));
    // Same version string but different bytes — a re-pinned DMG — is stale.
    let repinned = Receipt::at_bringup(pin_version.to_string(), "c".repeat(64), Origin::Pinned);
    assert!(stale_against_pin(Some(&repinned), pin_version, &pin_sha256));
    let current =
        Receipt::at_bringup(pin_version.to_string(), pin_sha256.clone(), Origin::Pinned);
    assert!(!stale_against_pin(Some(&current), pin_version, &pin_sha256));
}

/// A local-DMG install was a deliberate dev act; the pin never replaces it
/// behind the dev's back — whatever environment later reads the stamp.
#[test]
fn a_local_install_is_never_stale_against_the_pin() {
    let (pin_version, pin_sha256) = ("v2.4.110-moss.2", "a".repeat(64));
    let local = Receipt::at_bringup("local".into(), String::new(), Origin::Local);
    assert!(!stale_against_pin(Some(&local), pin_version, &pin_sha256));
}

/// A stamp written before `source` existed deserializes with `origin: None`
/// — "not recorded" rather than `Some(Origin::Pinned)` — and
/// [`stale_against_pin`] judges the two identically.
#[test]
fn a_sourceless_stamp_reads_as_not_recorded() {
    let parsed: Receipt = serde_json::from_str(r#"{"version":"v1","sha256":"aa"}"#).unwrap();
    assert_eq!(parsed.origin, None);
}

/// A placeholder pin has nothing installable behind it — reporting stale
/// against one would offer an update that can only fail.
#[test]
fn a_placeholder_pin_never_marks_an_install_stale() {
    let (pin_version, pin_sha256) = ("v0.0.0", "<PLACEHOLDER — Track OP fills at release>");
    assert!(!stale_against_pin(None, pin_version, pin_sha256));
}

/// The stamp round-trips through disk, and garbage reads as absent — the
/// stale side — rather than as an error somebody has to handle.
#[test]
fn install_stamp_round_trips_and_garbage_reads_as_absent() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(read_at(dir.path(), STACK_RECEIPT), None);

    let stamp = Receipt::at_bringup("v2.4.110-moss.2".into(), "d".repeat(64), Origin::Pinned);
    write_at(dir.path(), STACK_RECEIPT, &stamp).unwrap();
    assert_eq!(read_at(dir.path(), STACK_RECEIPT), Some(stamp));

    fs::remove_file(dir.path().join(STACK_RECEIPT)).unwrap();
    assert_eq!(read_at(dir.path(), STACK_RECEIPT), None);

    fs::write(dir.path().join(STACK_RECEIPT), b"{torn").unwrap();
    assert_eq!(read_at(dir.path(), STACK_RECEIPT), None);
}

/// The stack receipt file name is a dot-prefixed non-`.app` name, so it can
/// never be mistaken for an install nor shipped inside the bundle an update
/// rotates away.
#[test]
fn the_stamp_lives_beside_the_app_not_inside_it() {
    let dir = Path::new("/stacks/onionpress");
    let stamp = dir.join(STACK_RECEIPT);
    assert_eq!(stamp.parent(), Some(dir));
    let name = stamp.file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with('.') && !name.ends_with(".app"));
}

/// S2's one-way door: a stack stamp written through the unified type carries
/// exactly these four keys — `id`/`download_url`/`files` stay omitted (a
/// stack write never sets them), and `receipt_on` now records truthfully
/// that this was written at bring-up.
#[test]
fn a_stack_stamp_keeps_its_exact_on_disk_keys() {
    let dir = tempfile::tempdir().unwrap();
    let stamp = Receipt::at_bringup("v2.4.110-moss.2".into(), "a".repeat(64), Origin::Pinned);
    write_at(dir.path(), STACK_RECEIPT, &stamp).unwrap();
    let raw = fs::read_to_string(dir.path().join(STACK_RECEIPT)).unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let obj = value.as_object().unwrap();
    let keys: std::collections::BTreeSet<&str> = obj.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        ["version", "sha256", "source", "receipt_on"]
            .into_iter()
            .collect(),
        "unexpected key set: {raw}"
    );
    assert_eq!(obj["source"], "pinned");
    assert_eq!(obj["receipt_on"], "bringup");
}

/// Three literal fixtures spanning the field's whole history — before it
/// existed, spelled `pinned`, spelled `local` — must still parse and land on
/// the right side of staleness against a real pin. The fixture strings are
/// what a real disk holds, not what today's constructors emit, and are
/// unchanged from before the type moved crates.
#[test]
fn stamps_written_by_older_moss_versions_still_read_current() {
    let (pin_version, pin_sha256) = ("v2", "bb".repeat(32));

    let pre_source: Receipt = serde_json::from_str(r#"{"version":"v1","sha256":"aa"}"#).unwrap();
    assert!(
        stale_against_pin(Some(&pre_source), pin_version, &pin_sha256),
        "a pre-source stamp reads as None, which stale_against_pin judges the same as Pinned, and against the real pin it does not match"
    );

    let explicit_pinned: Receipt =
        serde_json::from_str(r#"{"version":"v1","sha256":"aa","source":"pinned"}"#).unwrap();
    assert!(
        stale_against_pin(Some(&explicit_pinned), pin_version, &pin_sha256),
        "same fields, explicit source — same mismatch, same verdict"
    );

    let local: Receipt =
        serde_json::from_str(r#"{"version":"local","sha256":"","source":"local"}"#).unwrap();
    assert!(
        !stale_against_pin(Some(&local), pin_version, &pin_sha256),
        "a Local install is never stale, whatever the version/sha256 say"
    );
}
