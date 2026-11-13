//! The nested-site decision table (rows 3–6) and its CLI refusal shapes.
//!
//! Detection lives in [`crate::nested_roots`], which never decides anything;
//! this module is the POLICY half both hosts share (crossed from the app's
//! nested-site guard at open-CLI slice 3).
//! The app's GUI guard consumes [`plan_unowned_open`] and renders a dialog;
//! [`guard_cli_open`] is the whole answer for a host with no surface to ask
//! on — every Confirm row becomes a refusal message naming the fix.

use crate::nested_roots::{
    classify_root, find_nested_roots, is_moss_copy, owns_moss, CopyEvidence, NestedRootInfo, NestedRootsReport, RootClass, ScanLimits, PUBLISH_TRAILS,
};
use std::path::Path;
use std::time::Duration;

/// Whether the folder's own listing has arrived from its cloud provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listing {
    /// Listed on this computer before the open went on.
    Settled,
    /// Still arriving: a `.moss` may be on its way.
    Settling,
    /// Still undecided at a later open, past the wait: what it needs may
    /// never come.
    Overdue,
}

/// The decision table's answer for an UNOWNED folder (no `.moss/` here or
/// above): proceed silently, or ask — and if asking, whether "make this
/// folder a site anyway" may be offered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnownedPlan {
    /// Row 3: normal folder, nothing nested, scan complete.
    Proceed,
    /// Rows 4/5/6 and the truncated-Normal case: confirmation required.
    Confirm { offer_create_anyway: bool },
    /// No `.moss/` here, and one copy of it that can only be this folder's
    /// site data: put it back as `.moss` without asking.
    HealCopy { copy: String },
    /// One copy, but what deciding it needs is still arriving: wait. Never
    /// make a second `.moss` beside it.
    AwaitCopy { copy: String },
    /// No copy in sight, and the folder's listing has not arrived: a `.moss`
    /// may be on its way, so none is made yet.
    AwaitFolder,
    /// One copy that waiting cannot decide: ask before putting it back.
    AskCopy { copy: String },
    /// Several copies and no `.moss/`: which one is the site is the
    /// person's call, so nothing is changed.
    RefuseCopies { copies: Vec<String> },
}

/// Decision-table rows 3–6, pure. `truncated` never degrades to "proceed":
/// only a CLEAN scan of a Normal folder with nothing nested builds silently.
///
/// A copy of `.moss` in the root that is the folder's site data (see
/// `MossCopy::is_site_data`) is put back, never replaced. Alone, in a folder
/// that would otherwise open without a question, it is put back without
/// asking once nothing else can be meant: the listing arrived and was read
/// whole, so no `.moss` is still on its way and no other copy sits unseen;
/// the copy reads; and a set-aside records a publish. Elsewhere it is left
/// alone and the folder's own question goes ahead, never offering a new site
/// beside it: moss itself sets a `.moss` aside over nested sites. A copy that
/// is here but cannot be read is asked about as soon as the folder has listed:
/// waiting cannot change the copy, but a `.moss` may still be arriving.
///
/// A quiet folder with no copy whose listing has not arrived waits too: a
/// `.moss` made now would become a sync client's `.moss 2` tomorrow. Past the
/// wait only the person can know, so the question offers set-up, and a late
/// arrival is left to the merge at build start.
///
/// "Create anyway" needs a Normal-shaped folder whose own listing was
/// complete, so no `.moss` or copy of one can be sitting in it unseen. Then
/// it is offered when nothing was found (only deeper levels were cut short),
/// or when a complete scan found one site that is not published (`published
/// != Some(true)`: orphaning a published identity stays off the table) and
/// that keeps its data in a real `.moss`.
pub fn plan_unowned_open(report: &NestedRootsReport, listing: Listing) -> UnownedPlan {
    let copies: Vec<_> = report.moss_copies.iter().filter(|c| c.is_site_data()).collect();
    let normal = report.root_class == RootClass::Normal;
    let quiet = report.nested.is_empty() && !report.truncated && normal;
    let listed = listing == Listing::Settled && report.root_listing_complete;
    match copies.as_slice() {
        [copy] if quiet => {
            let name = copy.name.clone();
            return match copy.evidence {
                CopyEvidence::Unreadable if listed => UnownedPlan::AskCopy { copy: name },
                CopyEvidence::Published | CopyEvidence::Listed if listed => UnownedPlan::HealCopy { copy: name },
                _ if listing == Listing::Overdue => UnownedPlan::AskCopy { copy: name },
                _ => UnownedPlan::AwaitCopy { copy: name },
            };
        }
        [] if quiet && !listed => {
            return if listing == Listing::Overdue {
                UnownedPlan::Confirm { offer_create_anyway: true }
            } else {
                UnownedPlan::AwaitFolder
            };
        }
        [] | [_] => {}
        _ => return UnownedPlan::RefuseCopies { copies: copies.iter().map(|c| c.name.clone()).collect() },
    }
    if quiet {
        return UnownedPlan::Proceed;
    }
    let offer_create_anyway = copies.is_empty()
        && normal
        && report.root_listing_complete
        && match report.nested.as_slice() {
            [] => true,
            [only] => !report.truncated && only.published != Some(true) && only.data_in.is_none(),
            _ => false,
        };
    UnownedPlan::Confirm { offer_create_anyway }
}

/// The site to offer first: shallowest, ties broken by relative path.
/// The report is BFS-ordered (shallowest-first) but sibling order is
/// filesystem order, so the tie-break keeps the choice deterministic.
pub fn pick_primary(sites: &[NestedRootInfo]) -> Option<&NestedRootInfo> {
    sites.iter().min_by(|a, b| {
        a.rel_path
            .split('/')
            .count()
            .cmp(&b.rel_path.split('/').count())
            .then_with(|| a.rel_path.cmp(&b.rel_path))
    })
}

/// Human phrase for a suspicious root shape, used in dialog and CLI copy.
pub fn class_description(class: &RootClass) -> Option<String> {
    match class {
        RootClass::Normal => None,
        RootClass::CloudProviderRoot { provider } => Some(format!("your whole {provider} drive")),
        RootClass::HomeDir => Some("your home folder".to_string()),
        RootClass::SystemSpecial(name) => Some(format!("your {name} folder")),
        RootClass::FilesystemRoot => Some("the entire disk".to_string()),
    }
}

/// The one `ScanLimits` both guards scan with.
pub fn default_limits() -> ScanLimits {
    ScanLimits::default()
}

/// CLI guard. `Err(message)` refuses the open; the caller prints it and exits
/// non-zero. Refusals are parametrized by `subcommand` (`build` / `deploy`).
///
/// Three refusal shapes: the folder belongs to an existing vault (re-rooting
/// a script silently is worse than refusing), the folder contains nested
/// moss roots, or the folder is suspicious/uncheckable (provider root, home,
/// truncated scan). A folder that IS a root always proceeds.
pub fn guard_cli_open(folder_path: &str, subcommand: &str) -> Result<(), String> {
    guard_cli_open_in(folder_path, subcommand, dirs::home_dir().as_deref())
}

/// [`guard_cli_open`] with the home folder passed in.
pub fn guard_cli_open_in(folder_path: &str, subcommand: &str, home: Option<&Path>) -> Result<(), String> {
    guard_cli_open_with(folder_path, subcommand, home, |copy_dir| request_copy_evidence(copy_dir, CLI_LISTING_WAIT))
}

/// [`guard_cli_open_in`] with the request for an undecided copy passed in.
fn guard_cli_open_with(folder_path: &str, subcommand: &str, home: Option<&Path>, request: impl FnOnce(&Path)) -> Result<(), String> {
    let path = Path::new(folder_path);
    // The home folder's `.moss/` holds moss's own app data, so it never makes
    // the home folder a site (the same rule `VaultRoot::find_containing` keeps).
    if owns_moss(path) && classify_root(path, home) != RootClass::HomeDir {
        return Ok(());
    }
    if let Some(vault) = crate::vault::paths::VaultRoot::find_containing_below(path, path, home) {
        let root = vault.as_str();
        return Err(format!(
            "'{folder_path}' belongs to the site at '{root}'.\n       {}",
            next_step(subcommand, format!("Run moss on that folder instead:  moss {subcommand} \"{root}\""))
        ));
    }
    let listing = if list_allowing_download(path, CLI_LISTING_WAIT) { Listing::Settled } else { Listing::Settling };
    let (report, plan) = scan_and_plan(path, home, |report| plan_unowned_open(report, listing), request);
    match plan {
        UnownedPlan::Proceed => Ok(()),
        // `history` only reads, so it leaves the folder as it found it.
        UnownedPlan::HealCopy { copy } if subcommand != "history" => {
            restore_moss_copy(path, &copy)?;
            crate::cli_eprintln!("Put '{copy}' back as .moss: it was this folder's site data.");
            Ok(())
        }
        _ => Err(cli_refusal_message(&report, listing, folder_path, subcommand)),
    }
}

/// Scan a folder and plan its open. A copy the plan waits on is requested
/// once and planned again, so evidence that lands in those seconds is acted
/// on in this open. `plan` runs once or twice; `request` at most once.
pub fn scan_and_plan(
    path: &Path,
    home: Option<&Path>,
    plan: impl Fn(&NestedRootsReport) -> UnownedPlan,
    request: impl FnOnce(&Path),
) -> (NestedRootsReport, UnownedPlan) {
    let report = find_nested_roots(path, home, &default_limits());
    let first = plan(&report);
    let UnownedPlan::AwaitCopy { copy } = &first else {
        return (report, first);
    };
    request(&path.join(copy));
    let report = find_nested_roots(path, home, &default_limits());
    let again = plan(&report);
    (report, again)
}

/// How long the command line waits for a cloud folder's listing.
const CLI_LISTING_WAIT: Duration = Duration::from_secs(10);

/// List `dir` on a thread of its own that may download, waiting at most
/// `wait`; whether the listing arrived in time. Every other thread fails fast
/// on a cloud placeholder, so only this one can make the provider fetch a
/// folder's listing. If the provider is slow the thread finishes after the
/// caller has moved on, and a second call for the same folder meanwhile does
/// not start another.
pub fn list_allowing_download(dir: &Path, wait: Duration) -> bool {
    static IN_FLIGHT: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<std::path::PathBuf>>> =
        std::sync::LazyLock::new(Default::default);
    let in_flight = || IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
    let dir = dir.to_path_buf();
    if !in_flight().insert(dir.clone()) {
        return false;
    }
    let (done, listed) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("moss-list-cloud-folder".into()).spawn({
        let dir = dir.clone();
        move || {
            crate::platform::materialize_on_this_thread();
            let complete = std::fs::read_dir(&dir).is_ok_and(|mut entries| entries.all(|e| e.is_ok()));
            in_flight().remove(&dir);
            let _ = done.send(complete);
        }
    });
    match spawned {
        Ok(_) => listed.recv_timeout(wait).unwrap_or(false),
        Err(_) => {
            in_flight().remove(&dir);
            false
        }
    }
}

/// What the plan reads of a copy to decide it: folders to list and files to
/// fetch. A request on a folder fetches nothing below it, so each is named.
fn copy_evidence(copy_dir: &Path) -> (Vec<std::path::PathBuf>, [std::path::PathBuf; 2]) {
    let mut dirs = vec![copy_dir.to_path_buf(), copy_dir.join("identity")];
    for trail in PUBLISH_TRAILS {
        let mut here = copy_dir.to_path_buf();
        for part in trail {
            here.push(part);
            dirs.push(here.clone());
        }
    }
    (dirs, [copy_dir.join("state.toml"), copy_dir.join("identity/public.json")])
}

/// Ask the cloud provider for what the plan could not read of a copy, and
/// wait for it. Returns within `wait`; a provider slower than that still
/// delivers, for a later open to decide.
pub fn request_copy_evidence(copy_dir: &Path, wait: Duration) {
    let deadline = std::time::Instant::now() + wait;
    let (dirs, files) = copy_evidence(copy_dir);
    for dir in dirs {
        list_allowing_download(&dir, deadline.saturating_duration_since(std::time::Instant::now()));
    }
    for file in &files {
        crate::build::cloud_readiness::request_download(file);
    }
    while files.iter().any(|file| crate::build::icloud::is_evicted(file)) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Put a copy of `.moss` (a sync client's conflict copy or a set-aside, see
/// [`is_moss_copy`]) back under its own name. Refuses unless `copy` is such a
/// copy and still a real directory, and never replaces a `.moss`: the rename
/// itself fails if one is there, even one a sync client created a moment ago.
pub fn restore_moss_copy(root: &Path, copy: &str) -> Result<(), String> {
    let target = root.join(".moss");
    let from = root.join(copy);
    if !is_moss_copy(copy) {
        return Err(format!("'{copy}' is not a copy of .moss"));
    }
    if !from.symlink_metadata().is_ok_and(|m| m.is_dir()) {
        return Err(format!("'{}' is no longer there", from.display()));
    }
    crate::platform::rename_exclusive(&from, &target).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => format!("'{}' already exists", target.display()),
        _ => format!("could not rename '{}' to .moss: {e}", from.display()),
    })?;
    log::info!("[site_guard] put '{copy}' back as .moss");
    Ok(())
}

/// `moss rename` reaches this guard only when no folder above the file has a
/// `.moss/`, so the file is in no site yet, even when the current directory
/// is inside one: its way forward is to make one.
/// Every other subcommand is given `default`.
fn next_step(subcommand: &str, default: String) -> String {
    match subcommand {
        "rename" => "The file is not inside a moss site. Run `moss build` once on the folder that should be the site, then run the rename again.".to_string(),
        "new" => "The folder is not inside a moss site. Run `moss build` once on the folder that should be the site, then run `moss new` again with a folder inside it.".to_string(),
        _ => default,
    }
}

/// The refusal text for each Confirm-shaped report. Pure, so every case is
/// testable without a filesystem.
pub fn cli_refusal_message(
    report: &NestedRootsReport,
    listing: Listing,
    folder_path: &str,
    subcommand: &str,
) -> String {
    match plan_unowned_open(report, listing) {
        UnownedPlan::AwaitFolder => {
            return format!("'{folder_path}' is still arriving from the cloud.\n       Run moss again once the folder has finished syncing.");
        }
        UnownedPlan::AwaitCopy { copy } => {
            return format!(
                "'{folder_path}' has no .moss yet, and its site data in '{copy}' is still arriving from the cloud.\n       Run moss again once the folder has finished syncing. If it never does, put it back yourself:  mv \"{folder_path}/{copy}\" \"{folder_path}/.moss\""
            );
        }
        UnownedPlan::HealCopy { copy } | UnownedPlan::AskCopy { copy } => {
            return format!(
                "'{folder_path}' has no .moss, but its site data is in '{copy}'.\n       Put it back, then run moss again:  mv \"{folder_path}/{copy}\" \"{folder_path}/.moss\""
            );
        }
        UnownedPlan::RefuseCopies { copies } => {
            return format!(
                "'{folder_path}' has no .moss, but several copies of it: {}.\n       Rename the right one to .moss, then run moss again.",
                copies.iter().map(|c| format!("'{c}'")).collect::<Vec<_>>().join(", ")
            );
        }
        UnownedPlan::Proceed | UnownedPlan::Confirm { .. } => {}
    }
    if let Some(primary) = pick_primary(&report.nested) {
        let id = primary
            .site_id
            .as_deref()
            .map(|id| format!(" (published site '{id}')"))
            .unwrap_or_default();
        let more = if report.nested.len() > 1 {
            format!(
                " and {} other moss site(s){}",
                report.nested.len() - 1,
                if report.truncated { ", possibly more" } else { "" }
            )
        } else if report.truncated {
            " (scan incomplete — there may be more)".to_string()
        } else {
            String::new()
        };
        return format!(
            "'{folder}' contains a moss site at {rel}{id}{more}.\n       {hint}",
            folder = folder_path,
            rel = primary.rel_path,
            hint = next_step(
                subcommand,
                format!("Run moss on that folder instead:  moss {subcommand} \"{folder_path}/{}\"", primary.rel_path)
            ),
        );
    }
    if let Some(desc) = class_description(&report.root_class) {
        return format!(
            "'{folder}' looks like {desc} — moss will not turn it into a site.\n       {hint}",
            folder = folder_path,
            hint = next_step(subcommand, format!("Run moss {subcommand} on the folder that holds your site instead.")),
        );
    }
    if !report.root_listing_complete {
        return format!(
            "'{folder_path}' could not be listed completely, so moss cannot tell whether a site is already in it.\n       {}",
            next_step(subcommand, "Run moss again once the folder has finished syncing.".to_string()),
        );
    }
    // Truncated Normal, nothing found: the scan cannot vouch for the tree.
    format!(
        "moss could not check every folder in '{folder}' for nested moss sites ({visited} folders scanned).\n       {hint}",
        folder = folder_path,
        visited = report.dirs_visited,
        hint = next_step(subcommand, format!("Run moss {subcommand} on the site folder itself.")),
    )
}

#[cfg(test)]
#[path = "site_guard_tests.rs"]
mod tests;
