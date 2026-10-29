//! The confinement a derivation binary runs in.
//!
//! On macOS the binary runs under `sandbox-exec` with the network denied and
//! file writes allowed only inside a per-run scratch directory and, when the
//! caller gives one, the plugin's persistent cache directory (plus the handful
//! of device nodes every program touches). Reads stay open: the binary has to
//! read its input and its own libraries.
//!
//! The profile is a denylist over `(allow default)`, so it also closes the
//! ways out of it that do not touch files or the network: Apple events, signals
//! to other processes, and the two services (Launch Services, the pasteboard)
//! through which a process asks the desktop session to act for it.
//!
//! Other platforms get no sandbox yet — only the scratch directory handed over
//! as `TMPDIR`/`HOME`, which a well-behaved binary writes to. The pinned hash
//! on the binary is the protection there.

use std::path::Path;
use std::process::Command;

#[cfg(target_os = "macos")]
const PROFILE: &str = r#"(version 1)
(allow default)
(deny network*)
(deny appleevent-send)
(deny signal (target others))
(deny mach-lookup
  (global-name "com.apple.coreservices.launchservicesd")
  (global-name "com.apple.pasteboard.1"))
(deny file-write*)
(allow file-write*
  (subpath (param "SCRATCH"))
  (subpath (param "CACHE"))
  (literal "/dev/null")
  (literal "/dev/dtracehelper")
  (regex #"^/dev/tty"))
"#;

/// A command that runs `binary` confined, with `scratch` and `cache` as its
/// only writable directories. Both must already exist and be canonical
/// (symlink-free), because the sandbox matches resolved paths. `cache` is
/// handed over as `MOSS_CACHE_DIR`: the plugin's private store, kept across
/// runs and sites, never synced, and cleared by moss at any time, so every
/// entry in it must be disposable. With no `cache` the variable is unset.
pub fn command(binary: &Path, scratch: &Path, cache: Option<&Path>) -> Command {
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut cmd = Command::new("/usr/bin/sandbox-exec");
        cmd.arg("-D")
            .arg(format!("SCRATCH={}", scratch.display()))
            .arg("-D")
            .arg(format!("CACHE={}", cache.unwrap_or(scratch).display()))
            .arg("-p")
            .arg(PROFILE)
            .arg(binary);
        cmd
    };
    #[cfg(not(target_os = "macos"))]
    let mut cmd = Command::new(binary);
    // Nothing of the build's environment reaches the binary: tokens and
    // credentials the user has exported are not its business. PATH stays so a
    // deriver can run a helper it ships beside itself; locale variables keep
    // its text handling the same as a terminal run.
    cmd.env_clear();
    for (key, value) in std::env::vars_os() {
        let name = key.to_string_lossy();
        if name == "PATH" || name == "LANG" || name.starts_with("LC_") {
            cmd.env(&key, value);
        }
    }
    cmd.env("TMPDIR", scratch).env("HOME", scratch);
    if let Some(cache) = cache {
        cmd.env("MOSS_CACHE_DIR", cache);
    }
    cmd
}

#[cfg(test)]
#[path = "sandbox_tests.rs"]
mod sandbox_tests;
