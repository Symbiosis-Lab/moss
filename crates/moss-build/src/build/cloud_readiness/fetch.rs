//! Waiting for cloud-only files from a command with no waiting screen.
//!
//! The desktop app shows a waiting screen and keeps going while a provider
//! delivers. A command line has nowhere to show one, so it has to decide for
//! itself: keep waiting while files keep arriving, and stop with a diagnosis
//! when they do not. The rule is a *no-arrival window*, not a total deadline:
//! every file that lands restarts the clock, so a slow provider that is making
//! progress is never cut off, and one that has gone quiet is reported after
//! [`MATERIALIZE_DEADLINE`] instead of blocking until the OS gives up on each
//! file in turn.
//!
//! The loop takes its pool and its "still in the cloud" test as values, so it
//! can be driven against an injected materializer (see `Prefetcher::with_materializer`)
//! on a platform with no dataless files.
//!
//! Two commands use it: `moss build` (a gate before the build starts) and
//! `moss fetch <folder>`, which downloads everything cloud-only in a folder and
//! writes nothing there.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{MATERIALIZE_DEADLINE, POLL_INTERVAL};
use crate::vault_root::resolve_input_in;
use crate::build::cloud_prefetch::Prefetcher;
use crate::build::cloud_provider::{self, CloudProvider};
use crate::build::icloud;

/// Reads handed to the pool per poll. The pool queues a few more than it runs
/// and drops the rest of what it is offered, so a long list goes in by turns.
const PER_POLL: usize = 6;

/// How long to wait, and on what.
pub struct Wait {
    pool: Prefetcher,
    in_cloud: Box<dyn Fn(&Path) -> bool + Send + Sync>,
    window: Duration,
    poll: Duration,
}

impl Wait {
    /// The real pool and the real eviction test.
    pub fn production() -> Self {
        Self {
            pool: Prefetcher::new(),
            in_cloud: Box::new(icloud::is_still_in_the_cloud),
            window: MATERIALIZE_DEADLINE,
            poll: POLL_INTERVAL,
        }
    }

    #[cfg(test)]
    pub(crate) fn with(
        pool: Prefetcher,
        in_cloud: impl Fn(&Path) -> bool + Send + Sync + 'static,
        window: Duration,
        poll: Duration,
    ) -> Self {
        Self { pool, in_cloud: Box::new(in_cloud), window, poll }
    }
}

/// Cloud-only files the provider is not delivering.
#[derive(Debug)]
pub struct Stall {
    pub provider: Option<CloudProvider>,
    pub total: usize,
    pub remaining: Vec<PathBuf>,
    /// Reads the provider refused outright, among `remaining`.
    pub refused: usize,
    pub window: Duration,
}

impl Stall {
    /// What happened and what to do, for a person or an agent reading stderr.
    /// `retry` is the command to run again once the provider is back.
    pub fn message(&self, folder: &Path, retry: &str) -> String {
        let who = self.provider.and_then(CloudProvider::display_name).unwrap_or("The cloud provider");
        let mut names: Vec<String> = self
            .remaining
            .iter()
            .take(3)
            .map(|p| p.strip_prefix(folder).unwrap_or(p).display().to_string())
            .collect();
        if self.remaining.len() > names.len() {
            names.push(format!("and {} more", self.remaining.len() - names.len()));
        }
        let mut text = format!(
            "{who} is not delivering {} of {} cloud-only files: none arrived in the last {}s.\nStill in the cloud: {}.\n",
            self.remaining.len(),
            self.total,
            self.window.as_secs(),
            names.join(", ")
        );
        if self.refused > 0 {
            text.push_str(&format!("The provider refused {} of the downloads.\n", self.refused));
        }
        text.push_str(&format!("{}, then run `{retry}` again.", open_hint(self.provider)));
        text
    }
}

fn open_hint(provider: Option<CloudProvider>) -> String {
    match provider {
        Some(CloudProvider::ICloud) => "Check that iCloud Drive is signed in and online so these files can download".into(),
        Some(p) => match p.display_name() {
            Some(name) => format!("Open {name} so these files can download"),
            None => generic_hint(),
        },
        None => generic_hint(),
    }
}

fn generic_hint() -> String {
    "Open the app that syncs this folder so these files can download".into()
}

/// How a wait ended.
#[derive(Debug)]
pub enum Outcome {
    /// Every file is on disk.
    Local { arrived: usize },
    Stalled(Stall),
}

/// Ask for every file in `files` that is still in the cloud, and wait until all
/// are local or `wait.window` passes with none arriving. `on_progress` is
/// called with (still in the cloud, total) at the start and after each change.
pub fn await_local(
    folder: &Path,
    files: Vec<PathBuf>,
    wait: &Wait,
    on_progress: &mut dyn FnMut(usize, usize),
) -> Outcome {
    let total = files.len();
    let mut pending: Vec<PathBuf> = files.into_iter().filter(|p| (wait.in_cloud)(p)).collect();
    pending.sort();
    let started = pending.len();
    if started == 0 {
        return Outcome::Local { arrived: 0 };
    }
    on_progress(started, total);
    let mut last_arrival = Instant::now();
    let mut cursor = 0;
    loop {
        for offset in 0..PER_POLL.min(pending.len()) {
            wait.pool.read(&pending[(cursor + offset) % pending.len()]);
        }
        cursor = (cursor + PER_POLL) % pending.len();
        std::thread::sleep(wait.poll);
        let before = pending.len();
        pending.retain(|p| (wait.in_cloud)(p));
        if pending.is_empty() {
            return Outcome::Local { arrived: started };
        }
        if pending.len() < before {
            last_arrival = Instant::now();
            cursor = 0;
            on_progress(pending.len(), total);
        } else if last_arrival.elapsed() >= wait.window {
            let set: std::collections::HashSet<PathBuf> = pending.iter().cloned().collect();
            let refused = wait.pool.failure_paths_for(folder, &set).len();
            return Outcome::Stalled(Stall {
                provider: cloud_provider::detect_from_path(folder),
                total,
                remaining: pending,
                refused,
                window: wait.window,
            });
        }
    }
}

/// [`await_local`] for a command: progress and the diagnosis go through `say`,
/// and `Err` carries the diagnosis for the caller to exit on.
pub fn bring_local(
    folder: &Path,
    files: Vec<PathBuf>,
    wait: &Wait,
    retry: &str,
    say: &mut dyn FnMut(&str),
) -> Result<usize, String> {
    let provider = cloud_provider::detect_from_path(folder).and_then(CloudProvider::display_name);
    let mut announced = false;
    let outcome = await_local(folder, files, wait, &mut |remaining, total| {
        if !announced {
            announced = true;
            say(&format!(
                "{remaining} of {total} files are in {}, not on this device; asking for them",
                provider.unwrap_or("the cloud")
            ));
        } else {
            say(&format!("{} of {total} downloaded", total - remaining));
        }
    });
    match outcome {
        Outcome::Local { arrived } => Ok(arrived),
        Outcome::Stalled(stall) => Err(stall.message(folder, retry)),
    }
}

/// The gate `moss build` runs before building: every source file the sweep
/// would ask the provider for is on disk, or the build does not start.
///
/// A folder that cannot be listed is left to the build, which reports it with
/// the rest of its own errors.
pub fn require_sources_local(
    folder: &Path,
    wait: &dyn Fn() -> Wait,
    say: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let walked = match crate::ops::watch::sweep::walk::walk_with_error(folder, None, None) {
        Ok(walked) => walked,
        Err(error) => {
            log::debug!("cloud: could not list {} before the build: {error}", folder.display());
            return Ok(());
        }
    };
    if walked.dataless.is_empty() {
        return Ok(());
    }
    bring_local(folder, walked.dataless, &wait(), "moss build", say).map(|_| ())
}

/// Usage line for `moss fetch`.
const USAGE: &str = "Usage: moss fetch <folder>";

/// Entry point for `moss fetch`. Returns an exit code (0 = every file is local).
pub fn run(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}\n\nDownloads every file in <folder> that is still only in the cloud, so the folder can be copied. Writes nothing. Exits 1 if the provider stops delivering, naming it and what to do.");
        return 0;
    }
    let [target] = args else {
        eprintln!("{USAGE}");
        return 1;
    };
    // Reads on this process's own threads must fail rather than block; only
    // the pool's readers wait for the provider.
    crate::platform::set_dataless_fail_fast();
    let cwd = std::env::current_dir().unwrap_or_default();
    let folder = resolve_input_in(Path::new(target), &cwd);
    fetch_folder(&folder, &Wait::production, &mut |l| eprintln!("{l}"), &mut |l| println!("{l}"))
}

/// `moss fetch` for one folder, testable apart from the process. Progress and
/// errors go to `say`, the result line to `out`; returns the exit code.
pub fn fetch_folder(
    folder: &Path,
    wait: &dyn Fn() -> Wait,
    say: &mut dyn FnMut(&str),
    out: &mut dyn FnMut(&str),
) -> i32 {
    if !folder.is_dir() {
        say(&format!("Error: not a folder: {}", folder.display()));
        return 1;
    }
    let listing = cloud_only_files(folder);
    let total = listing.files.len();
    if total > 0 {
        if let Err(message) = bring_local(folder, listing.files, &wait(), "moss fetch", say) {
            say(&message);
            return 1;
        }
    }
    if !listing.unlisted.is_empty() {
        let names: Vec<String> = listing.unlisted.iter().take(3).map(|p| p.strip_prefix(folder).unwrap_or(p).display().to_string()).collect();
        say(&format!(
            "{} folders could not be listed, so the files in them were not fetched: {}. Make sure the app that syncs this folder is running, then run `moss fetch` again.",
            listing.unlisted.len(),
            names.join(", ")
        ));
        return 1;
    }
    out(&match total {
        0 => format!("Nothing to fetch: every file in {} is already on this device.", folder.display()),
        n => format!("Fetched {n} files: every file in {} is now on this device.", folder.display()),
    });
    0
}

struct Listing {
    files: Vec<PathBuf>,
    /// Directories that would not enumerate. A dataless directory counts as
    /// unfetched, never as empty.
    unlisted: Vec<PathBuf>,
}

/// Every cloud-only regular file under `folder`, by `lstat` alone. All of it,
/// dot folders included, because a copy takes it all; only `.moss/build.nosync`
/// is left out, which the sync clients never upload.
fn cloud_only_files(folder: &Path) -> Listing {
    let skipped = folder.join(".moss").join("build.nosync");
    let mut listing = Listing { files: Vec::new(), unlisted: Vec::new() };
    for entry in walkdir::WalkDir::new(folder).sort_by_file_name().into_iter().filter_entry(|e| e.path() != skipped) {
        match entry {
            Ok(entry) if entry.file_type().is_file() => {
                let stub_target = icloud::icloud_stub_target(entry.path());
                if stub_target.is_some() || icloud::is_evicted(entry.path()) {
                    listing.files.push(stub_target.unwrap_or_else(|| entry.path().to_path_buf()));
                }
            }
            Ok(_) => {}
            Err(error) => listing.unlisted.push(error.path().unwrap_or(folder).to_path_buf()),
        }
    }
    listing
}

#[cfg(test)]
#[path = "fetch_tests.rs"]
mod tests;
