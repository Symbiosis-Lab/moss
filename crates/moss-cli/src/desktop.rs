//! The CLI's handoff to moss desktop.
//!
//! `moss-cli` has no window system, so `preview` and `edit` cannot run here.
//! Today they refuse; this module turns the refusal into a handoff when the
//! app is installed, and into a one-line hint naming how to get it when it
//! is not. `main.rs`'s two refusal arms call `hand_off` and exit with the
//! code it returns.

use std::collections::HashMap;
use std::fmt;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use minisign_verify::{PublicKey, Signature};
use moss_build::system::tar_safe::{extract_tar_gz, SymlinkPolicy, TarSafeError};

/// Same ceilings the stack executor's tar.gz arm uses
/// (`crates/moss-build/src/system/stack_exec/extract.rs`); a `moss.app`
/// tarball is two orders of magnitude below both. Defined here rather than
/// shared, because `stack_exec::extract` keeps its own ceilings private.
const INSTALL_MAX_ENTRIES: usize = 100_000;
const INSTALL_MAX_TOTAL_UNCOMPRESSED: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB

/// Where the updater manifest lives — the same endpoint `tauri.conf.json:52`
/// gives the app's own auto-updater, so `moss desktop install` reads the
/// release the app would offer itself. Names the repository the current
/// release repo becomes at the F6 flip, deliberately ahead of that rename.
const MANIFEST_URL: &str =
    "https://github.com/Symbiosis-Lab/moss/releases/latest/download/latest.json";

/// `tauri.conf.json:54`, lifted rather than read at runtime — this binary
/// carries no window system and no config loader, and the pubkey is public by
/// design (it verifies signatures; it cannot make any).
const PUBKEY_B64: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDc4QThBN0ZCNjg5NUJGNzIKUldSeXY1Vm8rNmVvZU1rUzQ5TlBWN3JXZ0lWbUExaUszVU9zb29nQXNlekVTaEhGQnEwZVpWZ1AK";

/// The only platform entry this binary ever installs — macOS only and
/// universal rather than per-architecture, so one download works on both
/// Apple silicon and Intel.
const PLATFORM_KEY: &str = "darwin-universal";

/// Where an installed moss desktop is, if anywhere.
///
/// macOS only today. Two `stat`s, in order:
/// `/Applications/moss.app`, then `~/Applications/moss.app` — the two places
/// `moss desktop install` can put it, and no others. **No `mdfind`**: a
/// developer box carries a debug build, a release build and often a
/// `moss.app` under a `target/` directory, all with the `host.moss.publisher`
/// identifier, and Spotlight indexing can additionally be off, stale, or
/// excluded for a volume — so it answers both "the wrong moss" and "not
/// installed" for an app sitting in plain sight.
///
/// `MOSS_DESKTOP_SEARCH_ROOT`, when set, REPLACES both roots with the single
/// directory it names. It is the test seam and nothing else: `locate`
/// otherwise reads `/Applications`, which a test may not write to, and
/// `$HOME`, which a test can only redirect for a child process. `locate_in`
/// is the pure half both paths call.
///
/// Debug builds only (`cfg!(debug_assertions)`): a release binary honoring
/// this var would let an env var alone choose the binary `hand_off` execs at
/// `:98`, on the same macOS platform the gate above exists to restrict.
pub fn locate() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        if let Some(root) = std::env::var_os("MOSS_DESKTOP_SEARCH_ROOT") {
            return locate_in(&[PathBuf::from(root)]);
        }
    }
    if !cfg!(target_os = "macos") {
        return None;
    }
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join("Applications"));
    }
    locate_in(&roots)
}

/// The pure locator: the first `<root>/moss.app` that exists.
pub fn locate_in(roots: &[PathBuf]) -> Option<PathBuf> {
    roots.iter().map(|root| root.join("moss.app")).find(|app| app.exists())
}

/// The binary inside a located bundle: `<app>/Contents/MacOS/moss`.
pub fn executable(app: &Path) -> PathBuf {
    app.join("Contents").join("MacOS").join("moss")
}

/// The one line printed when nothing is installed, shared by the handoff and
/// by `install` on a platform it refuses.
pub const fn hint() -> &'static str {
    "the editor lives in moss desktop — install it with `moss desktop install`, \
     or download it from https://mosspub.com"
}

/// Hand `verb <path>` to moss desktop and return, or print the one-line hint
/// and return the exit code the caller exits with.
///
/// Spawned, never `open -a`: single-instance forwards argv only on a real
/// launch, in the app's own startup path. Detached and NOT waited on — the
/// app outlives the CLI by design.
pub fn hand_off(verb: &str, path: &str) -> i32 {
    let Some(app) = locate() else {
        eprintln!("{}", hint());
        return 2;
    };
    let bin = executable(&app);
    match Command::new(&bin).args([verb, path]).spawn() {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("found moss desktop at {} but could not launch it: {e}", app.display());
            2
        }
    }
}

/// `moss desktop <install>`. Returns the process exit code.
///
/// The platform check runs BEFORE the terminal check, in that order and not
/// the reverse: on a platform with no desktop build there is nothing to ask
/// about whether or not stdin is a terminal, so a piped `cargo test` run or a
/// CI invocation there gets [`hint()`] rather than a complaint about the
/// terminal it was never going to use. Elsewhere, the command refuses without
/// a real terminal to ask `y` on, so a script piping `install`'s stdin can
/// never trigger an unattended download.
pub fn run(args: &[String]) -> i32 {
    if args.first().map(String::as_str) != Some("install") {
        eprintln!("Usage: moss desktop install");
        return 1;
    }
    if !cfg!(target_os = "macos") {
        eprintln!("{}", hint());
        return 2;
    }
    if !std::io::stdin().is_terminal() {
        eprintln!("moss desktop install needs a terminal to ask before downloading");
        return 2;
    }
    if let Some(app) = locate() {
        eprintln!("moss desktop is already installed at {}", app.display());
        return 2;
    }

    let (dest_parent, why) = match install_parent() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };

    let manifest_text = match moss_build::system::large_download::fetch_text(MANIFEST_URL, "latest.json") {
        Ok(text) => text,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let manifest: UpdaterManifest = match serde_json::from_str(&manifest_text) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("latest.json did not parse: {e}");
            return 2;
        }
    };
    let entry = match manifest.platforms.get(PLATFORM_KEY) {
        Some(entry) => entry,
        None => {
            eprintln!("{}", InstallError::NoPlatformEntry);
            return 2;
        }
    };

    println!("downloading moss desktop ({PLATFORM_KEY}) from {}", entry.url);
    println!("installing into {} ({why})", dest_parent.display());
    print!("continue? [y/N] ");
    if std::io::stdout().flush().is_err() {
        // Nothing depends on the flush succeeding except the prompt showing
        // up before we block on stdin below; a failure here isn't fatal.
    }
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() || answer.trim().to_lowercase() != "y" {
        eprintln!("not installing");
        return 2;
    }

    let artifact = match moss_build::system::large_download::fetch_bytes(&entry.url, "moss desktop") {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };

    match install(&manifest, PLATFORM_KEY, &artifact, PUBKEY_B64, &dest_parent) {
        Ok(app) => {
            println!(
                "verified: minisign signature over moss.app.tar.gz, against the release key moss ships"
            );
            println!("installed {}", app.display());
            0
        }
        Err(e) => {
            eprintln!("{e}");
            2
        }
    }
}

/// Verify `artifact` against `manifest`'s `platform_key` entry, then unpack
/// it — in that order, and never the reverse: nothing under `dest_parent` is
/// touched until [`verified_artifact`] returns `Ok`. The seam between
/// network I/O (in [`run`]) and this pure-of-network verify-then-extract
/// step, so tests 4–6 and 11–15 can drive it with fixture bytes.
pub fn install(
    manifest: &UpdaterManifest,
    platform_key: &str,
    artifact: &[u8],
    pubkey: &str,
    dest_parent: &Path,
) -> Result<PathBuf, InstallError> {
    verified_artifact(manifest, platform_key, artifact, pubkey)?;
    install_bundle(artifact, dest_parent)
}

/// `/Applications` when it is writable, else `~/Applications` (created if
/// absent). The caller prints which, and why, before installing.
pub fn install_parent() -> Result<(PathBuf, &'static str), InstallError> {
    let system_apps = PathBuf::from("/Applications");
    if is_writable_dir(&system_apps) {
        return Ok((system_apps, "the system Applications folder"));
    }
    let home = std::env::var_os("HOME")
        .ok_or_else(|| InstallError::Io("HOME is not set".to_string()))?;
    let user_apps = PathBuf::from(home).join("Applications");
    std::fs::create_dir_all(&user_apps).map_err(|e| InstallError::Io(e.to_string()))?;
    Ok((user_apps, "/Applications is not writable; using your own Applications folder"))
}

fn is_writable_dir(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    // The only reliable, side-effect-free writability probe: create and
    // immediately remove a throwaway file. `Path::metadata` permission bits
    // don't account for ACLs, SIP or a read-only volume.
    let probe = path.join(format!(".moss-desktop-write-probe-{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Unpack `moss.app` from an ALREADY-VERIFIED tarball and rename it under
/// `dest_parent` (`/Applications` or `~/Applications`). Returns the installed
/// bundle path.
///
/// Entry by entry via `moss_build::system::tar_safe::extract_tar_gz`, never
/// `Archive::unpack`, into a temp dir created inside `dest_parent` so the
/// last step is a rename and not a cross-device copy. Refuses, before
/// writing the entry: an absolute or climbing path, a hardlink, and a
/// symlink whose target is absolute or carries a `..` component. The
/// archive's top level must be exactly `moss.app`.
pub fn install_bundle(tarball: &[u8], dest_parent: &Path) -> Result<PathBuf, InstallError> {
    let temp_dir = dest_parent.join(format!(".moss-desktop-install-{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).map_err(|e| InstallError::Io(e.to_string()))?;

    let result = extract_tar_gz(
        tarball,
        &temp_dir,
        SymlinkPolicy::ContainedRelative,
        INSTALL_MAX_ENTRIES,
        INSTALL_MAX_TOTAL_UNCOMPRESSED,
    )
    .map_err(|e| match e {
        TarSafeError::UnsafeEntry { .. } => InstallError::UnsafeEntry,
        TarSafeError::Io(msg) => InstallError::Io(msg),
    });
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(e);
    }

    let extracted = temp_dir.join("moss.app");
    let top_level_is_exactly_moss_app = extracted.is_dir()
        && std::fs::read_dir(&temp_dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .all(|e| e.file_name() == std::ffi::OsStr::new("moss.app"))
            })
            .unwrap_or(false);
    if !top_level_is_exactly_moss_app {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(InstallError::UnsafeEntry);
    }

    let dest = dest_parent.join("moss.app");
    let renamed = std::fs::rename(&extracted, &dest);
    let _ = std::fs::remove_dir_all(&temp_dir);
    renamed.map_err(|e| InstallError::Io(e.to_string()))?;
    Ok(dest)
}

/// `latest.json` — the tauri updater manifest, UNSIGNED. Only `platforms` is
/// read; serde ignores the manifest's other fields (`version`, `notes`, …) by
/// default, so there is nothing to name here just to satisfy the parser.
///
/// This carries no signature of its own — see [`verified_artifact`] for what
/// is actually checked and why that is still sufficient.
#[derive(serde::Deserialize)]
pub struct UpdaterManifest {
    pub platforms: HashMap<String, ManifestPlatform>,
}

/// One `platforms` entry: `signature` is base64 of a minisign `.sig` file
/// covering the bytes at `url`, in the same double-base64 shape as
/// `tauri.conf.json`'s `updater.pubkey`.
#[derive(serde::Deserialize)]
pub struct ManifestPlatform {
    pub signature: String,
    pub url: String,
}

/// Everything that can stop `moss desktop install` before it prints success.
#[derive(Debug, PartialEq, Eq)]
pub enum InstallError {
    /// The manifest has no entry for the platform key that was looked up.
    NoPlatformEntry,
    /// The artifact's bytes did not verify against the entry's signature and
    /// `pubkey` — a decode failure of either counts as this too, since both
    /// mean the same thing to a caller: these bytes cannot be trusted.
    BadSignature,
    /// An archive entry was an absolute path, a path that leaves the temp
    /// directory, a hardlink, a symlink whose target is absolute or carries
    /// a `..` component — or the archive's top level was not exactly
    /// `moss.app`.
    UnsafeEntry,
    /// Anything else that stopped the install: network, filesystem, or a
    /// manifest/archive that failed to parse.
    Io(String),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallError::NoPlatformEntry => write!(f, "no release for this platform"),
            InstallError::BadSignature => write!(f, "signature verification failed"),
            InstallError::UnsafeEntry => write!(f, "the release archive contains an unsafe entry"),
            InstallError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// The platform entry's url, once THE ARTIFACT BYTES verify against `pubkey`.
///
/// `latest.json` carries no signature of its own; each entry's `signature`
/// covers the artifact its `url` names. So what this proves is narrow and
/// sufficient: these bytes are the ones moss signed for this platform. A
/// substituted manifest cannot produce bytes that pass.
///
/// Pure: takes the manifest and the artifact bytes, touches no network and no
/// filesystem, so the test can drive it with a fixture signed by a throwaway
/// key. `pubkey` is a parameter for exactly that reason — production passes
/// the constant lifted from `tauri.conf.json:54`. Both `signature` and
/// `pubkey` are base64 of a minisign text file and are decoded here.
pub fn verified_artifact<'a>(
    manifest: &'a UpdaterManifest,
    platform_key: &str,
    artifact: &[u8],
    pubkey: &str,
) -> Result<&'a str, InstallError> {
    let entry = manifest
        .platforms
        .get(platform_key)
        .ok_or(InstallError::NoPlatformEntry)?;

    let pubkey_text = decode_minisign_b64(pubkey)?;
    let public_key = PublicKey::decode(&pubkey_text).map_err(|_| InstallError::BadSignature)?;
    let signature_text = decode_minisign_b64(&entry.signature)?;
    let signature = Signature::decode(&signature_text).map_err(|_| InstallError::BadSignature)?;

    public_key
        .verify(artifact, &signature, false)
        .map_err(|_| InstallError::BadSignature)?;

    Ok(&entry.url)
}

/// The base64 layer both `pubkey` and a manifest entry's `signature` share:
/// each is base64 of the plain-text minisign file, not the key/signature
/// itself. Decoding failure at this layer is indistinguishable from a bad
/// signature to a caller, so it collapses into the same error.
fn decode_minisign_b64(value: &str) -> Result<String, InstallError> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| InstallError::BadSignature)?;
    String::from_utf8(bytes).map_err(|_| InstallError::BadSignature)
}

#[cfg(test)]
#[path = "desktop_tests.rs"]
mod desktop_tests;
