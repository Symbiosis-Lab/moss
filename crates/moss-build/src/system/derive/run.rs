//! One run of a derivation binary: request in on stdin, one JSON answer out,
//! bounded in time, output size and — on macOS — what it may touch.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::{sandbox, DeriveFailure, DeriveOutcome};
use crate::system::bounded_command::{run_bounded_capped, OutputCaps};

/// Longest answer accepted on stdout. A file's fragments are text; anything
/// near this is a runaway, and the build holds the answer in memory.
pub const OUTPUT_CAP: usize = 32 * 1024 * 1024;

/// Longest stderr kept. It only feeds the one-line failure message, so a
/// deriver that logs more than this is a runaway too.
pub const STDERR_CAP: usize = 1024 * 1024;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// Per-file time limit: ten minutes (a long scanned document legitimately takes
/// minutes; the limit exists to stop a hung process, not a slow one), or `MOSS_DERIVE_TIMEOUT_SECS` when set to
/// a positive whole number.
pub fn derive_timeout() -> Duration {
    timeout_from(std::env::var("MOSS_DERIVE_TIMEOUT_SECS").ok().as_deref())
}

fn timeout_from(value: Option<&str>) -> Duration {
    value
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .map_or(DEFAULT_TIMEOUT, Duration::from_secs)
}

/// A scratch directory for one run, removed when dropped. Created under the
/// system temp dir and canonicalized, since the sandbox matches real paths.
struct Scratch(PathBuf);

impl Scratch {
    /// `create_dir` (not `create_dir_all`) refuses to proceed through a path
    /// that already exists, including a symlink someone planted at a guessable
    /// name, so the directory removed on drop is always one this run made.
    fn create() -> Result<Self, DeriveFailure> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let fail = |e: std::io::Error| DeriveFailure::new(format!("could not create a scratch directory: {e}"));
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        for _ in 0..16 {
            let dir = std::env::temp_dir().join(format!(
                "moss-derive-{}-{}-{nanos:x}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
            match builder.create(&dir) {
                Ok(()) => return dir.canonicalize().map(Self).map_err(fail),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(fail(e)),
            }
        }
        Err(DeriveFailure::new("could not create a scratch directory: every candidate name was taken"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The last non-empty stderr line, the part of a crash worth showing.
fn last_line(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(300).collect())
}

/// Run `binary` on `input` and parse its answer. Does not check the fragments
/// against a declaration — [`DeriveOutcome::check_against`] does, because only
/// the caller knows the declaration.
pub fn run_deriver(
    binary: &Path,
    input: &Path,
    timeout: Duration,
) -> Result<DeriveOutcome, DeriveFailure> {
    let input = std::path::absolute(input)
        .map_err(|e| DeriveFailure::new(format!("bad input path: {e}")))?;
    let input_str = input
        .to_str()
        .ok_or_else(|| DeriveFailure::new("the input path is not valid UTF-8"))?;
    let request = serde_json::json!({ "path": input_str }).to_string();

    let scratch = Scratch::create()?;
    let mut cmd = sandbox::command(binary, &scratch.0);
    let what = binary.file_name().map_or_else(|| "derivation binary".into(), |n| n.to_string_lossy().into_owned());
    let caps = OutputCaps { stdout: Some(OUTPUT_CAP), stderr: Some(STDERR_CAP) };
    let out = run_bounded_capped(&mut cmd, Some(request.into_bytes()), timeout, &what, caps, || {})
        .map_err(DeriveFailure::new)?;

    if !out.status.success() {
        let detail = last_line(&out.stderr).map_or(String::new(), |l| format!(": {l}"));
        return Err(DeriveFailure::new(format!("{what} failed ({}){detail}", out.status)));
    }
    serde_json::from_str::<DeriveOutcome>(out.stdout.trim()).map_err(|e| {
        let detail = last_line(&out.stderr).map_or(String::new(), |l| format!(" ({l})"));
        DeriveFailure::new(format!("{what} did not print a derivation result: {e}{detail}"))
    })
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod run_tests;
