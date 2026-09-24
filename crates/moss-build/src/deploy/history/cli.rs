//! `moss history` — the CLI form of publish history (slice 2).
//! One row in [`crate::cli::commands::cli_commands`], on `OWN_HELP` because it
//! parses its own flags. Lives beside [`super::store`]/[`super::record`]/
//! [`super::restore`]/[`super::timeline`] rather than under `cli/`, which is
//! at its file budget.
//!
//! Five forms, dispatched by which flags/positional are present:
//!
//! ```text
//! moss history [--json]                              site timeline
//! moss history <path> [--json]                        one page's timeline
//! moss history --save [<name>]                        save a version now
//! moss history <path> --restore --at <id> [--copy]    restore a page
//! moss history --restore --at <id> --yes              restore the site
//! ```
//!
//! There is no `<folder>` argument anywhere in this command, unlike
//! `build`/`deploy`/`list` — every form resolves the vault the same way,
//! [`VaultRoot::containing`] from the current directory, matching a person
//! running it from inside their site. A `<path>` argument on the timeline and
//! page-restore forms is therefore a manifest-relative SOURCE path *within
//! that vault*, resolved against the root and never a second folder to act on
//! — see [`relative_source_path`].
//!
//! `--save` and `--restore` both need a [`SealedManifest`] of the tree as it
//! stands right now, and the only way to get one outside the app (which keeps
//! one live in `AppState`) is to build — [`headless_build_sealed`], the same
//! shape `moss deploy` builds with (`deploy::push::run_hosted_deploy`), minus
//! the publish.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::record::{PublishRecord, Trigger};
use super::restore::{self, RestoreMode, RestoreReport};
use super::store;
use super::HistoryStore;
use crate::build::manifest::{published_record, SealedManifest};
use crate::moss_paths::MossPaths;
use crate::vault_root::{resolve_input, VaultRoot};

pub fn run(args: &[String]) -> i32 {
    let parsed = match ParsedArgs::parse(args) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => {
            println!("{}", usage());
            return 0;
        }
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };

    let root = VaultRoot::containing(Path::new("."));

    if parsed.save {
        return run_save(&root, parsed.path);
    }
    if parsed.restore {
        // `--at` is required for either restore shape; checked once here so
        // both branches below can assume it is `Some`.
        let Some(at) = parsed.at else {
            eprintln!("error: --restore requires --at <id>\n{}", usage());
            return 1;
        };
        return match parsed.path {
            Some(path) => run_restore_page(&root, &path, &at, parsed.copy),
            None => run_restore_site(&root, &at, parsed.yes),
        };
    }
    match parsed.path {
        Some(path) => run_page_timeline(&root, &path, parsed.json),
        None => run_site_timeline(&root, parsed.json),
    }
}

// ---------------------------------------------------------------------------
// Argument parsing
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct ParsedArgs {
    path: Option<String>,
    json: bool,
    save: bool,
    restore: bool,
    at: Option<String>,
    copy: bool,
    yes: bool,
}

impl ParsedArgs {
    /// `Ok(None)` means `-h`/`--help` was seen and the caller should print
    /// usage and exit 0. `Err` is the whole text to print on stderr before
    /// exiting 1, matching `deploy::DeployArgs::parse`'s contract.
    fn parse(args: &[String]) -> Result<Option<Self>, String> {
        let mut path: Option<String> = None;
        let mut json = false;
        let mut save = false;
        let mut restore = false;
        let mut at: Option<String> = None;
        let mut copy = false;
        let mut yes = false;

        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "-h" | "--help" => return Ok(None),
                "--json" => json = true,
                "--save" => save = true,
                "--restore" => restore = true,
                "--copy" => copy = true,
                "--yes" => yes = true,
                "--at" => {
                    i += 1;
                    match args.get(i) {
                        Some(v) => at = Some(v.clone()),
                        None => return Err(format!("error: --at requires a version id\n{}", usage())),
                    }
                }
                a if a.starts_with('-') => {
                    return Err(format!("error: unknown option '{a}'\n{}", usage()));
                }
                a => {
                    if path.is_some() {
                        return Err(format!("error: moss history takes at most one path\n{}", usage()));
                    }
                    path = Some(a.to_string());
                }
            }
            i += 1;
        }

        if save && restore {
            return Err(format!("error: --save and --restore cannot be combined\n{}", usage()));
        }
        // The positional after `--save` is the version's NAME, not a
        // path — `moss history --save "Before rewriting the intro"`. It
        // was already collected as `path` above; hand it back as the
        // label instead of refusing it.
        Ok(Some(ParsedArgs { path, json, save, restore, at, copy, yes }))
    }
}

fn usage() -> &'static str {
    "Usage:
  moss history [--json]                              site timeline, newest first
  moss history <path> [--json]                        one page's timeline
  moss history --save [<name>]                        save a version now
  moss history <path> --restore --at <id> [--copy]    restore one page
  moss history --restore --at <id> --yes              restore the whole site

<path> is a file's location within the current site, not a second folder —
moss history always operates on the site containing the current directory.
<id> is a version's id (a timeline row), or an unambiguous prefix of one."
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// The vault's own history store. Infallible now that it is just a join
/// against `root` — kept as its own function so every command that touches
/// the store goes through one name rather than repeating the join.
fn open_store(root: &VaultRoot) -> HistoryStore {
    HistoryStore::in_vault(root.path())
}

/// Turn a CLI path argument into the manifest-relative SOURCE path
/// [`PublishRecord::entries`] is keyed by ("articles/hello.md", never an
/// absolute path or one relative to the current directory).
///
/// Deliberately does not require the path to exist: a page's history —
/// including restoring it — is exactly what you reach for after the page is
/// gone (`resolve_input` never touches the filesystem beyond a `..`
/// component, so a deleted page's stem still resolves).
fn relative_source_path(root: &VaultRoot, raw: &str) -> Result<String, String> {
    let abs = resolve_input(raw);
    let rel = abs
        .strip_prefix(root.path())
        .map_err(|_| format!("'{raw}' is not inside the site at '{}'", root.as_str()))?;
    if rel.as_os_str().is_empty() {
        return Err(format!("'{raw}' names the site itself, not a page inside it"));
    }
    Ok(moss_core::slug::normalize_separators(&rel.to_string_lossy()))
}

/// `<id>` is a record filename stem, or an unambiguous prefix of one — typed
/// once by a person copying the first few characters off a timeline row.
/// Exact match wins outright, so a full id already on disk is never
/// second-guessed by a coincidental prefix collision.
fn resolve_id<'a>(records: &'a [(String, PublishRecord)], needle: &str) -> Result<&'a str, String> {
    if let Some((id, _)) = records.iter().find(|(id, _)| id == needle) {
        return Ok(id.as_str());
    }
    let matches: Vec<&str> = records
        .iter()
        .map(|(id, _)| id.as_str())
        .filter(|id| id.starts_with(needle))
        .collect();
    match matches.len() {
        0 => Err(format!("no such version: {needle}")),
        1 => Ok(matches[0]),
        _ => Err(format!(
            "'{needle}' matches more than one version: {}",
            matches.join(", ")
        )),
    }
}

/// This process's setup, then a real headless build of `root`, and the
/// manifest it sealed — [`crate::deploy::one_shot::build_sealed_now`], the
/// same build `deploy::push::run_hosted_deploy` and
/// `deploy::plugin_push::run_plugin_deploy` use, minus the publish. `--save`
/// and `--restore` both need this: neither is running inside the app, which
/// keeps a sealed manifest live in `AppState` as files change, so a terminal
/// process has to make its own.
///
/// The three setup lines and the runtime are what a fresh CLI process owes and
/// the shared build does not do — a serving process has already done them, and
/// `register_session` in particular DRAINS the folder's existing session.
///
/// No `--allow-plugins` here — this command has no such flag — so a
/// sideloaded plugin is refused exactly as a plain `moss build` refuses one;
/// a plugin the app has already approved still runs.
fn headless_build_sealed(root: &VaultRoot) -> Result<SealedManifest, String> {
    crate::build::cli_output::install_headless_logger();
    crate::plugins::install::registry_client::enforce::install_headless(false);
    let _session = crate::system::folder_session::register_session(root.as_str());

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("failed to start runtime: {e}"))?;

    runtime.block_on(crate::deploy::one_shot::build_sealed_now(root))
}

/// The site's currently configured publish target, or moss's own target for
/// a folder that has never named one — the same fallback the app's
/// `current_deploy_target` makes.
fn current_target(root: &VaultRoot) -> String {
    crate::build::site_config::get_domain_config(root.as_str())
        .ok()
        .and_then(|c| c.publish_target())
        .unwrap_or_else(|| crate::config::deployment::MOSS_TARGET_ID.to_string())
}

// ---------------------------------------------------------------------------
// --save
// ---------------------------------------------------------------------------

fn run_save(root: &VaultRoot, name: Option<String>) -> i32 {
    if let Err(msg) = crate::cli::site_guard::guard_cli_open(root.as_str(), "history") {
        eprintln!("error: {msg}");
        return 1;
    }
    let store = open_store(root);
    let label = name.filter(|s| !s.trim().is_empty());

    let sealed = match headless_build_sealed(root) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };

    let target = current_target(root);
    if let Err(e) = store.snapshot_manual(root.path(), &sealed, &target, label) {
        eprintln!("error: could not save a version: {e}");
        return 1;
    }

    // `list_records` sorts by filename, which sorts chronologically (see its
    // own doc) — the record `snapshot_manual` just wrote is always last.
    match store.list_records().pop() {
        Some((id, _)) => {
            println!("{id}");
            0
        }
        None => {
            // Unreachable in practice (the write above just succeeded), but
            // an empty listing is not this function's place to explain.
            eprintln!("error: saved a version but could not find it afterward");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Restore
// ---------------------------------------------------------------------------

/// What both restores need before they diverge: the guard `--save` already
/// applies, the store, the resolved id, and the pre-restore version.
struct RestorePrep {
    store: HistoryStore,
    id: String,
    sealed: SealedManifest,
}

/// Guard, open the store, resolve `at` to a real id, build the tree as it
/// stands now, and save that as a version before anything is overwritten or
/// trashed. Both `run_restore_page` and `run_restore_site` are this plus
/// their own final call — the guard used to be missing from both (finding 1
/// of the 2026-09-11 handoff), which is why it opens this shared prefix
/// rather than living at either call site.
fn prepare_restore(root: &VaultRoot, at: &str) -> Result<RestorePrep, String> {
    crate::cli::site_guard::guard_cli_open(root.as_str(), "history")?;
    let store = open_store(root);
    let records = store.list_records();
    let id = resolve_id(&records, at)?.to_string();
    let target = records
        .iter()
        .find(|(rid, _)| rid == &id)
        .map(|(_, r)| r.target.clone())
        .unwrap_or_else(|| current_target(root));
    let sealed = headless_build_sealed(root)?;
    if let Err(e) = store.save_before_restore(root.path(), &sealed, &target, &id) {
        eprintln!("warning: could not save a version before restoring: {e}");
    }
    Ok(RestorePrep { store, id, sealed })
}

fn run_restore_page(root: &VaultRoot, path_arg: &str, at: &str, copy: bool) -> i32 {
    // A bad path fails before any build or guard consultation.
    let rel = match relative_source_path(root, path_arg) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    let prep = match prepare_restore(root, at) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };

    let mode = if copy { RestoreMode::AsCopy } else { RestoreMode::InPlace };
    match prep.store.restore_page(root.path(), &prep.id, &rel, mode) {
        Ok(()) => {
            match mode {
                RestoreMode::InPlace => println!("Restored {rel}."),
                RestoreMode::AsCopy => println!("Saved a copy of {rel} from this version beside it."),
            }
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn run_restore_site(root: &VaultRoot, at: &str, yes: bool) -> i32 {
    if !yes {
        eprintln!("error: restoring the whole site needs --yes to confirm\n{}", usage());
        return 1;
    }
    let prep = match prepare_restore(root, at) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };

    let report = prep.store.restore_site(root.path(), &prep.id, &prep.sealed, &restore::real_trash);
    print_restore_report(&report);
    if report.failed.is_empty() {
        0
    } else {
        1
    }
}

/// The honest accounting the design asks for: what came back, what did not,
/// and what moved to the Trash to reach the restored state — never a single
/// pass/fail bit for an operation that is best-effort by contract.
fn print_restore_report(report: &RestoreReport) {
    if !report.restored.is_empty() {
        println!("Restored {} page(s):", report.restored.len());
        for path in &report.restored {
            println!("  {path}");
        }
    }
    if !report.trashed.is_empty() {
        println!(
            "Moved {} page(s) to the Trash (added since this version):",
            report.trashed.len()
        );
        for path in &report.trashed {
            println!("  {path}");
        }
    }
    if !report.failed.is_empty() {
        eprintln!("Failed to restore {} page(s):", report.failed.len());
        for (path, reason) in &report.failed {
            eprintln!("  {path}: {reason}");
        }
    }
    if report.restored.is_empty() && report.trashed.is_empty() && report.failed.is_empty() {
        println!("Nothing to restore — the site already matches this version.");
    }
}

// ---------------------------------------------------------------------------
// Timeline listing
// ---------------------------------------------------------------------------

/// One timeline row plus the fields [`super::timeline::Row`] does not carry
/// (`generation_id`, `git_head`) but a listing needs — `live` and the commit.
/// Built once per listing from [`HistoryStore::list_records`], keyed by id,
/// so `site_timeline`/`page_timeline`'s folded rows can be decorated without
/// a second I/O pass.
struct DisplayRow {
    id: String,
    published_at: String,
    target: String,
    trigger: Trigger,
    label: Option<String>,
    live: bool,
    changed: usize,
    added: usize,
    removed: usize,
    git_head: Option<String>,
    /// `None` off the site timeline (design: "page_timeline leaves them at
    /// zero" for the counts, and kept-ness is a per-page question that the
    /// site timeline has no single page to ask). `Some` on the page timeline:
    /// whether THIS page's bytes at this record are still in the store.
    kept: Option<bool>,
}

/// Resolve `[live_generation_id for a target]`, once per target, over the
/// lifetime of one listing — a site with one target asks this once; a site
/// with several asks it once per target rather than once per row.
struct LiveResolver<'a> {
    root: &'a Path,
    cache: HashMap<String, Option<String>>,
}

impl<'a> LiveResolver<'a> {
    fn new(root: &'a Path) -> Self {
        Self { root, cache: HashMap::new() }
    }

    fn is_live(&mut self, record: &PublishRecord) -> bool {
        let root = self.root;
        let live_gen = self
            .cache
            .entry(record.target.clone())
            .or_insert_with(|| {
                let mp = MossPaths::new(root);
                published_record::load_for(&mp, Some(&record.target)).map(|s| s.generation_id)
            });
        super::is_live(record, live_gen.as_deref())
    }
}

fn decorate(
    store_root: &Path,
    rows: Vec<super::timeline::Row>,
    by_id: &HashMap<String, PublishRecord>,
    live: &mut LiveResolver,
    page: Option<&str>,
) -> Vec<DisplayRow> {
    let objects = page.map(|_| store::object_store(store_root));
    let mut out: Vec<DisplayRow> = rows
        .into_iter()
        .filter_map(|row| {
            let record = by_id.get(&row.id)?;
            let kept = page.map(|path| match (record.entries.get(path), &objects) {
                (Some(entry), Some(objects)) => objects.get_path(&entry.hash).is_some(),
                // Absent entry is not "not kept" — the page simply is not in
                // this version, which the timeline fold already decided is
                // still worth a stop (a manual/restore record, most often).
                _ => true,
            });
            Some(DisplayRow {
                id: row.id,
                published_at: row.published_at,
                target: row.target,
                trigger: row.trigger,
                label: row.label,
                live: live.is_live(record),
                changed: row.changed,
                added: row.added,
                removed: row.removed,
                git_head: record.git_head.clone(),
                kept,
            })
        })
        .collect();
    // Both `site_timeline` and `page_timeline` are oldest-first (matching
    // `store::list_records`); the design wants newest-first everywhere.
    out.reverse();
    out
}

fn run_site_timeline(root: &VaultRoot, json: bool) -> i32 {
    let store = open_store(root);
    let records = store.list_records();
    if records.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No versions yet.");
            println!("moss saves one every time you publish. You can also save one now: moss history --save");
        }
        return 0;
    }
    let by_id: HashMap<String, PublishRecord> = records.into_iter().collect();
    let mut live = LiveResolver::new(root.path());
    let rows = decorate(store.store_dir(), store.site_timeline(), &by_id, &mut live, None);

    if json {
        print_json(&rows);
        return 0;
    }

    let show_target = by_id.values().map(|r| &r.target).collect::<HashSet<_>>().len() > 1;
    render_human(&rows, show_target, true);
    println!();
    println!("Versions are kept at {}.", store.store_dir().display());
    0
}

fn run_page_timeline(root: &VaultRoot, path_arg: &str, json: bool) -> i32 {
    let rel = match relative_source_path(root, path_arg) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    let store = open_store(root);
    let records = store.list_records();
    if records.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No versions of {rel} yet.");
        }
        return 0;
    }
    let by_id: HashMap<String, PublishRecord> = records.into_iter().collect();
    let mut live = LiveResolver::new(root.path());
    let rows = decorate(store.store_dir(), store.page_timeline(&rel), &by_id, &mut live, Some(&rel));

    if json {
        print_json(&rows);
        return 0;
    }
    if rows.is_empty() {
        println!("No versions of {rel} yet.");
        return 0;
    }
    let show_target = by_id.values().map(|r| &r.target).collect::<HashSet<_>>().len() > 1;
    render_human(&rows, show_target, false);
    0
}

fn print_json(rows: &[DisplayRow]) {
    #[derive(serde::Serialize)]
    struct RowJson<'a> {
        id: &'a str,
        published_at: &'a str,
        trigger: Trigger,
        label: Option<&'a str>,
        live: bool,
        target: &'a str,
        changed: usize,
        added: usize,
        removed: usize,
        git_head: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        kept: Option<bool>,
    }
    let json: Vec<RowJson> = rows
        .iter()
        .map(|r| RowJson {
            id: &r.id,
            published_at: &r.published_at,
            trigger: r.trigger,
            label: r.label.as_deref(),
            live: r.live,
            target: &r.target,
            changed: r.changed,
            added: r.added,
            removed: r.removed,
            git_head: r.git_head.as_deref(),
            kept: r.kept,
        })
        .collect();
    match serde_json::to_string_pretty(&json) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("error: could not serialize the timeline: {e}"),
    }
}

/// Newest-first, grouped by calendar day (the day key and the header both
/// come from the record's own `published_at`, which is always UTC — see
/// [`record::format_date`], the same convention this reuses).
fn render_human(rows: &[DisplayRow], show_target: bool, show_counts: bool) {
    let mut current_day: Option<String> = None;
    for row in rows {
        let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(&row.published_at) else {
            continue;
        };
        let day = parsed.format("%Y-%m-%d").to_string();
        if current_day.as_deref() != Some(day.as_str()) {
            if current_day.is_some() {
                println!();
            }
            println!("{}", parsed.format("%B %-d, %Y"));
            current_day = Some(day);
        }

        let what = match row.trigger {
            Trigger::Publish => "Published".to_string(),
            _ => row.label.clone().unwrap_or_else(|| "Saved by you".to_string()),
        };
        let mut line = format!("  {}  {}", parsed.format("%H:%M"), what);
        if row.live {
            line.push_str("  live");
        }
        if show_target {
            line.push_str(&format!("  [{}]", row.target));
        }
        if show_counts {
            let mut parts = Vec::new();
            if row.changed > 0 {
                parts.push(format!("{} changed", row.changed));
            }
            if row.added > 0 {
                parts.push(format!("{} added", row.added));
            }
            if row.removed > 0 {
                parts.push(format!("{} removed", row.removed));
            }
            if !parts.is_empty() {
                line.push_str("  ");
                line.push_str(&parts.join(", "));
            }
        }
        if let Some(git_head) = &row.git_head {
            line.push_str(&format!("  {}", &git_head[..git_head.len().min(8)]));
        }
        if row.kept == Some(false) {
            line.push_str("  not kept");
        }
        println!("{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_vault() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        (dir, vault)
    }

    #[test]
    fn parse_rejects_combining_save_and_restore() {
        let err = ParsedArgs::parse(&["--save".to_string(), "--restore".to_string()])
            .unwrap_err();
        assert!(err.contains("cannot be combined"), "{err}");
    }

    #[test]
    fn parse_rejects_a_second_positional() {
        let err = ParsedArgs::parse(&["a.md".to_string(), "b.md".to_string()]).unwrap_err();
        assert!(err.contains("at most one path"), "{err}");
    }

    #[test]
    fn parse_requires_a_value_after_at() {
        let err = ParsedArgs::parse(&["--restore".to_string(), "--at".to_string()]).unwrap_err();
        assert!(err.contains("--at requires"), "{err}");
    }

    #[test]
    fn help_flag_short_circuits_to_none() {
        assert!(ParsedArgs::parse(&["--help".to_string()]).unwrap().is_none());
        assert!(ParsedArgs::parse(&["-h".to_string()]).unwrap().is_none());
    }

    #[test]
    fn save_takes_its_positional_as_a_name_not_a_path() {
        let parsed = ParsedArgs::parse(&["--save".to_string(), "Before the rewrite".to_string()])
            .unwrap()
            .unwrap();
        assert!(parsed.save);
        assert_eq!(parsed.path.as_deref(), Some("Before the rewrite"));
    }

    /// A page path resolves relative to the SITE, not to the process's
    /// current directory when the two differ — the walk-up in
    /// `VaultRoot::containing` and the strip-prefix here must agree on the
    /// same root regardless of which subfolder a person is sitting in.
    #[test]
    fn relative_source_path_strips_the_vault_root() {
        let (_dir, vault) = tmp_vault();
        std::fs::create_dir_all(vault.join("articles")).unwrap();
        let root = VaultRoot::resolve(&vault);
        let rel = relative_source_path(&root, vault.join("articles/hello.md").to_str().unwrap()).unwrap();
        assert_eq!(rel, "articles/hello.md");
    }

    /// A folder with no `[hooks] deploy` and no `site_id` has never named a
    /// publish target. A manual save before the first publish still needs a
    /// target string to record, and it must be the same fallback the app's
    /// `current_deploy_target` makes, not a sentinel of the CLI's own.
    #[test]
    fn a_site_that_never_published_records_the_moss_target() {
        let (_dir, vault) = tmp_vault();
        let root = VaultRoot::resolve(&vault);
        assert_eq!(current_target(&root), crate::config::deployment::MOSS_TARGET_ID);
    }

    /// A page's history must be reachable after the page itself is gone —
    /// that is the whole point of "restore a deleted page".
    #[test]
    fn relative_source_path_does_not_require_the_file_to_exist() {
        let (_dir, vault) = tmp_vault();
        let root = VaultRoot::resolve(&vault);
        let rel = relative_source_path(&root, vault.join("gone.md").to_str().unwrap()).unwrap();
        assert_eq!(rel, "gone.md");
    }

    #[test]
    fn relative_source_path_refuses_a_path_outside_the_site() {
        let (_dir, vault) = tmp_vault();
        let outside = tempfile::tempdir().unwrap();
        let root = VaultRoot::resolve(&vault);
        let err =
            relative_source_path(&root, outside.path().join("x.md").to_str().unwrap()).unwrap_err();
        assert!(err.contains("not inside the site"), "{err}");
    }

    fn record(id: &str, target: &str) -> (String, PublishRecord) {
        (
            id.to_string(),
            PublishRecord {
                version: 1,
                published_at: "2026-01-01T00:00:00Z".to_string(),
                target: target.to_string(),
                generation_id: "gen".to_string(),
                git_head: None,
                trigger: Trigger::Publish,
                label: None,
                entries: Default::default(),
            },
        )
    }

    #[test]
    fn resolve_id_matches_an_exact_id_even_when_it_is_also_a_prefix_of_another() {
        let records = vec![record("2026-01-01T00-00-00Z-abc", "t"), record("2026-01-01T00-00-00Z-abcd", "t")];
        assert_eq!(resolve_id(&records, "2026-01-01T00-00-00Z-abc").unwrap(), "2026-01-01T00-00-00Z-abc");
    }

    #[test]
    fn resolve_id_accepts_an_unambiguous_prefix() {
        let records = vec![record("2026-01-01T00-00-00Z-abc", "t")];
        assert_eq!(resolve_id(&records, "2026-01-01T00").unwrap(), "2026-01-01T00-00-00Z-abc");
    }

    #[test]
    fn resolve_id_refuses_an_ambiguous_prefix() {
        let records = vec![record("2026-01-01T00-00-00Z-abc", "t"), record("2026-01-02T00-00-00Z-def", "t")];
        let err = resolve_id(&records, "2026-01-0").unwrap_err();
        assert!(err.contains("more than one"), "{err}");
    }

    #[test]
    fn resolve_id_reports_no_match() {
        let records = vec![record("2026-01-01T00-00-00Z-abc", "t")];
        let err = resolve_id(&records, "nope").unwrap_err();
        assert!(err.contains("no such version"), "{err}");
    }
}
