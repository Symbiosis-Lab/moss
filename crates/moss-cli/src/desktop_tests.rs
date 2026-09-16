//! Unit tests for the pure halves of the handoff: `locate_in`, `executable`
//! and `hint`. The full `Test 1` (binary-level, driving `moss-cli edit` as a
//! subprocess with `MOSS_DESKTOP_SEARCH_ROOT` and `HOME` set) lives in
//! `src-tauri/tests/desktop_handoff_test.rs` — `locate()`'s env-var reads and
//! `hand_off`'s `eprintln!` are not observable from an in-process unit test
//! without either mutating shared process environment across parallel test
//! threads or adding a test-only output sink neither `locate()` nor
//! `hand_off()` otherwise need.

use super::*;

#[test]
fn locate_in_finds_the_first_existing_app() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let empty_root = tmp.path().join("empty");
    std::fs::create_dir_all(&empty_root).expect("mkdir");
    let real_root = tmp.path().join("real");
    std::fs::create_dir_all(real_root.join("moss.app")).expect("mkdir");

    let found = locate_in(&[empty_root.clone(), real_root.clone()]);
    assert_eq!(found, Some(real_root.join("moss.app")));
}

#[test]
fn locate_in_is_none_when_no_root_has_the_app() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("empty");
    std::fs::create_dir_all(&root).expect("mkdir");

    assert_eq!(locate_in(&[root]), None);
}

#[test]
fn executable_is_the_bundled_macos_binary() {
    let app = PathBuf::from("/Applications/moss.app");
    assert_eq!(executable(&app), PathBuf::from("/Applications/moss.app/Contents/MacOS/moss"));
}

#[test]
fn hint_names_the_install_subcommand_not_a_stale_url() {
    let text = hint();
    assert!(text.contains("moss desktop install"), "{text}");
    assert!(!text.contains(".dmg"), "{text}");
}

// Tests 4-6 (E2' step 4) need a minisign keypair and a signed message that
// are not moss's own. Neither `minisign` nor `rsign`/`rsign2` is installed on
// this box, so these are lifted verbatim from `minisign-verify`'s own
// doc/test fixture — the same keypair its `verify_prehashed` test uses to
// sign the literal bytes `b"test"`, which stand in here for "the artifact".
// `PUBKEY_B64` and `GOOD_SIG_B64` are each base64 of a plain-text minisign
// file, exactly as `tauri.conf.json`'s `updater.pubkey` and a manifest
// entry's `signature` are.

/// base64 of `untrusted comment: minisign public key E7620F1842B4E81F` +
/// `RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3` — the key
/// `minisign-verify`'s own `verify`/`verify_prehashed` tests use.
const PUBKEY_B64: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXkgRTc2MjBGMTg0MkI0RTgxRgpSV1FmNkxSQ0dBOWk1M21sWWVjTzRJelQ1MVRHUHB2V3VjTlNDaDFDQk0wUVRhTG43M1k3R0ZPMwo=";

/// A second, syntactically valid minisign public key with a different
/// `key_id` (bytes flipped from `PUBKEY_B64`'s key) — not a key anything was
/// ever signed with, which is the point of test 5: any pubkey that isn't the
/// signing one must be refused, and a `key_id` mismatch is minisign-verify's
/// own first check, before it touches ed25519 at all.
const OTHER_PUBKEY_B64: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXkgREVBREJFRUZDQUZFQkFCRQpSV1RnRjdSQ0dBOWk1M21sWWVjTzRJelQ1MVRHUHB2V3VjTlNDaDFDQk0wUVRhTG43M1k3R0ZPMwo=";

/// `minisign-verify`'s `verify_prehashed` fixture: a signature over `b"test"`
/// by the secret half of `PUBKEY_B64`, prehashed (so `verified_artifact`'s
/// `allow_legacy = false` accepts it).
const GOOD_SIG_B64: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIG1pbmlzaWduIHNlY3JldCBrZXkKUlVRZjZMUkNHQTlpNTU5cjNnN1YxcU55SkRBcEdpcDhNZnFjYWRJZ1Q5Q3VoVjNFTWhIb04xbUdUa1VpZEYvejdTcmxRZ1hkeThvZmpiN2JOSkp5bERPb2NyQ284S0x6WndvPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNTU2MTkzMzM1CWZpbGU6dGVzdAp5L3JVdzJ5OC9oT1VZalpVNzFlSHAvV28xS1o0MGZHeTJWSkVEbDM0WE1KTStUWDQ4U3MvMTd1M0l2SWZiVlIxRmtaWlNOQ2lzUWJ1UVkrYkh3aEVCZz09Cg==";

const ARTIFACT: &[u8] = b"test";

fn manifest_with_one_platform(key: &str, signature: &str, url: &str) -> UpdaterManifest {
    let json = serde_json::json!({
        "version": "1.0.0",
        "platforms": { key: { "signature": signature, "url": url } },
    });
    serde_json::from_value(json).expect("manifest fixture parses")
}

#[test]
fn a_manifest_signature_over_the_wrong_bytes_is_refused() {
    let manifest =
        manifest_with_one_platform("darwin-universal", GOOD_SIG_B64, "https://example.invalid/moss.app.tar.gz");

    assert_eq!(
        verified_artifact(&manifest, "darwin-universal", ARTIFACT, PUBKEY_B64),
        Ok("https://example.invalid/moss.app.tar.gz")
    );

    let mut flipped = ARTIFACT.to_vec();
    flipped[0] ^= 0xFF;
    assert_eq!(
        verified_artifact(&manifest, "darwin-universal", &flipped, PUBKEY_B64),
        Err(InstallError::BadSignature)
    );
}

#[test]
fn a_signature_from_another_key_is_refused() {
    let manifest =
        manifest_with_one_platform("darwin-universal", GOOD_SIG_B64, "https://example.invalid/moss.app.tar.gz");

    assert_eq!(
        verified_artifact(&manifest, "darwin-universal", ARTIFACT, OTHER_PUBKEY_B64),
        Err(InstallError::BadSignature)
    );
}

#[test]
fn the_platform_key_selects_its_own_entry() {
    let json = serde_json::json!({
        "version": "1.0.0",
        "platforms": {
            "darwin-x86_64": { "signature": GOOD_SIG_B64, "url": "https://example.invalid/x86_64.tar.gz" },
            "darwin-aarch64": { "signature": GOOD_SIG_B64, "url": "https://example.invalid/aarch64.tar.gz" },
            "darwin-universal": { "signature": GOOD_SIG_B64, "url": "https://example.invalid/universal.tar.gz" },
            "linux-x86_64": { "signature": GOOD_SIG_B64, "url": "https://example.invalid/linux.tar.gz" },
        },
    });
    let manifest: UpdaterManifest = serde_json::from_value(json).expect("manifest fixture parses");

    assert_eq!(
        verified_artifact(&manifest, "darwin-universal", ARTIFACT, PUBKEY_B64),
        Ok("https://example.invalid/universal.tar.gz")
    );
    assert_eq!(
        verified_artifact(&manifest, "windows-x86_64", ARTIFACT, PUBKEY_B64),
        Err(InstallError::NoPlatformEntry)
    );
}

// The remaining tar-shaped tests need an actual tar.gz archive, and the
// signed-install test needs a signature over one of them rather than over
// the literal `b"test"` the tests above reuse from `minisign-verify`'s own
// fixture. That fixture has no keypair to sign a second message with, so
// this key and signature are generated fresh (an Ed25519 keypair with no
// other use), in the identical minisign text-file-then-base64 shape
// `PUBKEY_B64`/`GOOD_SIG_B64` are. **No secret key is checked in anywhere —
// only its output.** `CLEAN_TARBALL_B64` is built with Python's `tarfile`
// (checked in as base64, never as a file) and carries the one allowed
// symlink shape (contained, relative — see `a_signed_clean_tarball_installs`
// below). The hostile shapes (traversal, absolute path, hardlink, symlink
// escaping the target) are `moss_build::system::tar_safe`'s own tests now
// (`crates/moss-build/src/system/tar_safe_tests.rs`), since `install_bundle`
// no longer implements that guard itself. Regenerate `CLEAN_TARBALL_B64`,
// including a fresh throwaway keypair, with `python3
// crates/moss-cli/tests/fixtures/gen_desktop_fixtures.py` and paste its
// output over the constants below — it needs only python3 and the `openssl`
// CLI, no network and no non-stdlib packages.

/// `moss.app/Contents/Info.plist` plus a relative symlink,
/// `Frameworks/X.framework/Versions/Current -> A`, that stays inside the
/// bundle — the fixture every install test unpacks, and the one that also
/// exercises the allowed symlink case (test `a_signed_clean_tarball_installs`
/// below).
const CLEAN_TARBALL_B64: &str = "H4sIAJx5pGoC/+3WTQuCMBzH8b0U6QW4J6cXK0QIegPR1YNBpE42rV5+YniROihqQr/PZRMED1/8b7m21k3KkpL5sEagVLs2+uuHfaCYIo4iC6htlZjmk+Q/5V3/WBdVWlSWrqA/55IJ9P9N/2Nx0W6ZXW01ZX/f8773l6rXXwrFiMPQf3bh/plnzj019qqL7Ya7bLPfhW1+uiPwf///wSR5+tDmNtlRMHz+S09g/v++/9m9dA900f7KF7j/rav/6X1EjBkKw/sHnkT/lfaP6AL9fY7+K+0f18Y0L055/+/vBWeBJI6I0B8AAAAAAAAAAAAAYIwXPznrjwAoAAA=";

/// A throwaway Ed25519 pubkey (not moss's), used only to check that a real,
/// matching signature over the clean tarball installs.
const FIXTURE_PUBKEY_B64: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXkgRDFFMUU3N0Y4QzI4MjlBQwpSV1NzS1NpTWYrZmgwUW9nTVppei8relU5NVI5aWNLR3k2cGlaT3NGZklMbmg1OSt3TDZSczl2cwo=";

/// That key's secret half signing [`CLEAN_TARBALL_B64`] — a real minisign
/// signature over real bytes, not over `b"test"`.
const FIXTURE_SIG_B64: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIG1pbmlzaWduIHNlY3JldCBrZXkKUlVTc0tTaU1mK2ZoMGZJWGcreXdQazd0WHpHd0xwZm9idExiSDkxZ0tyKzIyWi9uMW9vNzAxYkJPdHpSSFpFcXladWFvdVczL09CbUQ0Qzh4NFlCUy95NEhWMXVESmVUalFJPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzg5MTYzOTMyCWZpbGU6bW9zcy5hcHAudGFyLmd6CjlrZGpKa01ySVMrVm1Qd3Y4WmNpWkhUSStpMjE0cU1Td1JyblIvMnJDdEVHMUpiUjNxa1RZdGQxdEhONkkwaXJHQWtuM2hzVE40QS9ycnFHOGtmK0NnPT0K";

fn decode_fixture(b64: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(b64).expect("fixture decodes")
}

/// `moss.app` plus a second, stray top-level entry — the shape
/// `install_bundle`'s "top level must be exactly `moss.app`" doc claims is
/// refused, which until now nothing checked (`:250` only tested that
/// `moss.app` itself is a directory, not that it is the only thing there).
fn tar_gz_with_stray_top_level_file() -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());

    let mut dir_header = tar::Header::new_gnu();
    dir_header.set_path("moss.app").expect("relative path");
    dir_header.set_entry_type(tar::EntryType::Directory);
    dir_header.set_size(0);
    dir_header.set_mode(0o755);
    dir_header.set_cksum();
    builder.append(&dir_header, std::io::empty()).expect("append dir");

    let content = b"not part of the bundle";
    let mut file_header = tar::Header::new_gnu();
    file_header.set_path("README.txt").expect("relative path");
    file_header.set_size(content.len() as u64);
    file_header.set_mode(0o644);
    file_header.set_cksum();
    builder.append(&file_header, &content[..]).expect("append file");

    let tar_bytes = builder.into_inner().expect("finish tar");
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut gz, &tar_bytes).expect("write gz");
    gz.finish().expect("finish gz")
}

#[test]
fn a_stray_top_level_entry_is_refused() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dest_parent = tmp.path().join("Applications");
    std::fs::create_dir_all(&dest_parent).expect("mkdir");

    let result = install_bundle(&tar_gz_with_stray_top_level_file(), &dest_parent);
    assert_eq!(result, Err(InstallError::UnsafeEntry));
    assert!(!dest_parent.join("moss.app").exists(), "nothing should be installed");
    assert!(!dest_parent.join("README.txt").exists(), "the stray entry must never land in dest_parent");
}

#[test]
fn a_bad_signature_stops_before_anything_is_written() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dest_parent = tmp.path().join("Applications");
    std::fs::create_dir_all(&dest_parent).expect("mkdir");
    let clean = decode_fixture(CLEAN_TARBALL_B64);

    // GOOD_SIG_B64 signs `b"test"`, not `clean` — a signature over the wrong
    // bytes, same as test 4, driven here through the whole `install` path
    // rather than through `verified_artifact` alone.
    let manifest = manifest_with_one_platform(
        "darwin-universal",
        GOOD_SIG_B64,
        "https://example.invalid/moss.app.tar.gz",
    );

    let result = install(&manifest, "darwin-universal", &clean, PUBKEY_B64, &dest_parent);
    assert_eq!(result, Err(InstallError::BadSignature));

    let remaining: Vec<_> = std::fs::read_dir(&dest_parent).expect("read_dir").collect();
    assert!(remaining.is_empty(), "nothing should be written before the signature verifies, found {remaining:?}");
}

#[test]
fn a_signed_clean_tarball_installs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dest_parent = tmp.path().join("Applications");
    std::fs::create_dir_all(&dest_parent).expect("mkdir");
    let clean = decode_fixture(CLEAN_TARBALL_B64);

    let manifest = manifest_with_one_platform(
        "darwin-universal",
        FIXTURE_SIG_B64,
        "https://example.invalid/moss.app.tar.gz",
    );

    let installed = install(&manifest, "darwin-universal", &clean, FIXTURE_PUBKEY_B64, &dest_parent)
        .expect("a correctly signed clean tarball installs");
    assert_eq!(installed, dest_parent.join("moss.app"));

    let plist = std::fs::read_to_string(installed.join("Contents").join("Info.plist"))
        .expect("Info.plist was extracted");
    assert_eq!(plist, "<?xml version=\"1.0\"?><plist/>");

    // The allowed symlink case: relative, and resolves inside the bundle.
    let current = installed.join("Contents/Frameworks/X.framework/Versions/Current");
    assert_eq!(
        std::fs::read_link(&current).expect("symlink was extracted"),
        PathBuf::from("A")
    );
    assert!(
        current.join("..").join("A").exists(),
        "the symlink's relative target must exist inside the installed bundle"
    );

    let remaining: Vec<_> = std::fs::read_dir(&dest_parent)
        .expect("read_dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert_eq!(remaining, vec![std::ffi::OsString::from("moss.app")], "no leftover temp directory");
}
