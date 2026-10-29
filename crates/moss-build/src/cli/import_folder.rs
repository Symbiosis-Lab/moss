//! `moss import <folder>` and `moss import --status`: showing the files in a
//! folder as pages, and reporting where each folder stands.
//!
//! `moss import <url|file>` brings content in from outside; these two forms
//! work on files already in the site. Which files can become pages, and what a
//! page looks like, is whatever the installed plugins declare — nothing here
//! names a format.

use std::path::{Path, PathBuf};

use crate::moss_paths::MossPaths;
use crate::system::derive::registry::{DerivationEntry, DerivationRegistry};
use crate::vault::derive_folders::{decisions, folder_key, nearest_decision, set_folder_decision, Decision};
use crate::vault::derive_state::{derivation_state, site_registry, FolderDerivationState};
use crate::vault::derive_stubs::{accepted_files, write_stubs};
use crate::vault_root::VaultRoot;

pub const USAGE: &str = "\
Usage: moss import <folder> [-r|--recursive] [--with <derivation-id>] [--allow-plugins]
       moss import --status [<site>] [--json] [--allow-plugins]

<folder> is an existing folder of files an installed plugin can turn into
pages. moss shows them as pages: it records the choice in .moss/config.toml
and writes a page beside each file. Existing pages are never overwritten, and a
file another page already embeds gets none. If more than one plugin accepts
files in the folder, pass --with to choose. A plugin the site carries runs
only if it is allowed, as in a build: pass --allow-plugins to run what the
folder carries, or allow it in the moss app first.

With -r (--recursive) the command covers <folder> and every folder beneath it
(hidden folders and .moss are skipped): each folder holding files the chosen
derivation accepts and with no decision yet is shown as pages. A folder that
already has a decision, by its own entry or an ancestor's, keeps it; a shown
one still gets the pages it lacks. It prints one
line per folder (folder, what was done, pages written) and a total. A folder
where several plugins accept the files is reported and skipped unless --with
picks one.

--status lists every folder holding such files: what was decided (undecided,
show, keep), how many files, how many pages are still missing, and the files a
plugin declined, with its reason. With --json it prints one object:
  {\"folders\": [{\"folder\": \"reports/\", \"derivation_id\": \"...\",
    \"noun_one\": \"doc\", \"noun_other\": \"docs\", \"count\": 3,
    \"decision\": \"undecided|show|keep\", \"stubs_missing\": 0,
    \"skipped\": [{\"file\": \"a.ext\", \"reason\": \"...\", \"message\": \"...\"}]}]}
folder is relative to the site root with a trailing slash (\"/\" is the root).";

enum Form {
    Status { site: Option<String>, json: bool },
    Folder { folder: PathBuf, with: Option<String>, recursive: bool },
}

const ALLOW_PLUGINS: &str = "--allow-plugins";

/// Recognise the two forms; `None` leaves the argument list to the URL/file
/// importer, so every existing form parses exactly as before.
fn recognise(args: &[String]) -> Option<Result<Form, String>> {
    if args.iter().any(|a| a == "--status") {
        let mut site = None;
        let mut json = false;
        for a in args.iter().filter(|a| *a != "--status") {
            match a.as_str() {
                "--json" => json = true,
                ALLOW_PLUGINS => {}
                s if s.starts_with('-') => return Some(Err(format!("unknown option: {s}"))),
                s if site.is_none() => site = Some(s.to_string()),
                _ => return Some(Err("--status takes at most one site".to_string())),
            }
        }
        return Some(Ok(Form::Status { site, json }));
    }
    let mut positional = Vec::new();
    let mut with = None;
    let mut recursive = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--with" => {
                i += 1;
                match args.get(i) {
                    Some(v) => with = Some(v.clone()),
                    None => return Some(Err("--with requires a derivation id".to_string())),
                }
            }
            ALLOW_PLUGINS => {}
            "-r" | "--recursive" => recursive = true,
            s if s.starts_with('-') => return None,
            s => positional.push(s.to_string()),
        }
        i += 1;
    }
    match positional.as_slice() {
        [one] if Path::new(one).is_dir() => Some(Ok(Form::Folder { folder: PathBuf::from(one), with, recursive })),
        _ => None,
    }
}

/// Run one of the two forms, or `None` when `args` is something else.
pub fn dispatch(args: &[String]) -> Option<i32> {
    let form = match recognise(args)? {
        Ok(f) => f,
        Err(msg) => {
            eprintln!("error: {msg}");
            eprintln!("{USAGE}");
            return Some(1);
        }
    };
        crate::plugins::install::registry_client::enforce::install_headless(args.iter().any(|a| a == ALLOW_PLUGINS));
    let result = match form {
        Form::Status { site, json } => status(site.as_deref(), json),
        Form::Folder { folder, with, recursive: false } => show_folder(&folder, with.as_deref()),
        Form::Folder { folder, with, recursive: true } => show_tree(&folder, with.as_deref()),
    };
    Some(match result {
        Ok(()) => 0,
        Err(msg) => {
            eprintln!("error: {msg}");
            1
        }
    })
}

fn open_site(near: &Path) -> Result<(MossPaths, DerivationRegistry), String> {
    let root = VaultRoot::find_containing(near)
        .ok_or_else(|| format!("{} is not inside a moss site (no .moss folder above it)", near.display()))?;
    let site = MossPaths::new(root.path());
    let registry = site_registry(&site)?;
    Ok((site, registry))
}

/// Why no single derivation could be chosen for a folder.
enum Pick<'a> {
    /// No installed derivation accepts files in the folder.
    Nothing,
    /// Several do; the author must say which.
    Several(Vec<&'a DerivationEntry>),
}

fn pick_entry<'a>(registry: &'a DerivationRegistry, folder: &Path, with: Option<&str>) -> Result<&'a DerivationEntry, Pick<'a>> {
    let candidates: Vec<&DerivationEntry> = registry
        .entries()
        .filter(|e| with.is_none_or(|id| e.derivation_id == id))
        .filter(|e| !accepted_files(folder, e).is_empty())
        .collect();
    match candidates.as_slice() {
        [] => Err(Pick::Nothing),
        [one] => Ok(*one),
        _ => Err(Pick::Several(candidates)),
    }
}

fn several_message(folder: &Path, many: &[&DerivationEntry]) -> String {
    let ids: Vec<String> = many
        .iter()
        .map(|e| format!("  {} ({}: {})", e.derivation_id, e.plugin_id, e.contribution.noun.other))
        .collect();
    format!(
        "more than one derivation accepts files in {}; choose with --with <derivation-id>:\n{}",
        folder.display(),
        ids.join("\n")
    )
}

/// Record that `folder` shows its files as pages and write the pages it lacks.
fn show_one(site: &MossPaths, entry: &DerivationEntry, folder: &Path) -> Result<Vec<PathBuf>, String> {
    set_folder_decision(site, folder, Decision::Show(vec![entry.derivation_id.clone()]))?;
    Ok(write_stubs(folder, entry))
}

fn show_folder(folder: &Path, with: Option<&str>) -> Result<(), String> {
    let folder = crate::vault::paths::resolve_input(folder);
    let (site, registry) = open_site(&folder)?;
    let entry = match pick_entry(&registry, &folder, with) {
        Ok(entry) => entry,
        Err(Pick::Nothing) => return Err(nothing_message(&folder, with)),
        Err(Pick::Several(many)) => return Err(several_message(&folder, &many)),
    };
    let written = show_one(&site, entry, &folder)?;
    let count = accepted_files(&folder, entry).len();
    let noun = if count == 1 { &entry.contribution.noun.one } else { &entry.contribution.noun.other };
    println!("Showing {count} {noun} as pages in {}", folder.display());
    for path in written {
        println!("{}", path.display());
    }
    Ok(())
}

fn nothing_message(folder: &Path, with: Option<&str>) -> String {
    match with {
        Some(id) => format!("no installed plugin derivation `{id}` accepts files in {}", folder.display()),
        None => format!(
            "no installed plugin turns the files in {} into pages; install one with `moss plugin install`",
            folder.display()
        ),
    }
}

/// `moss import <folder> --recursive`: [`show_one`] for `root` and every folder
/// beneath it that the build looks at. A folder the author already decided (by its
/// own entry or an ancestor's) keeps its decision, and a shown one gets the stubs it lacks.
fn show_tree(root: &Path, with: Option<&str>) -> Result<(), String> {
    let root = crate::vault::paths::resolve_input(root);
    let (site, registry) = open_site(&root)?;
    let decided = decisions(&site);
    let folders = walkdir::WalkDir::new(&root)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || crate::build::scan::classify::left_out_of_site(e).is_none())
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_dir());
    let (mut seen, mut total) = (0usize, 0usize);
    for dir in folders {
        let dir = dir.path();
        let own_key = folder_key(&site, dir);
        let decision = own_key.as_deref().and_then(|k| nearest_decision(&decided, k));
        let key = own_key.unwrap_or_else(|| dir.display().to_string());
        let entry = match (pick_entry(&registry, dir, with), decision) {
            (Err(Pick::Nothing), _) => continue,
            (Ok(entry), _) => Some(entry),
            (Err(Pick::Several(_)), Some(_)) => None,
            (Err(Pick::Several(_)), None) => {
                seen += 1;
                println!("{key}  not changed: several plugins accept these files, choose with --with  0 pages written");
                continue;
            }
        };
        seen += 1;
        let (what, written) = match (decision, entry) {
            (Some(Decision::Keep), _) => ("kept as files, left as it was", 0),
            (Some(Decision::Show(ids)), _) => {
                let written = ids
                    .iter()
                    .filter_map(|id| pick_entry(&registry, dir, Some(id)).ok())
                    .map(|entry| write_stubs(dir, entry).len())
                    .sum();
                (if written == 0 { "shown as pages, left as it was" } else { "shown as pages" }, written)
            }
            (Some(Decision::Undecided) | None, Some(entry)) => ("shown as pages", show_one(&site, entry, dir)?.len()),
            (_, None) => continue,
        };
        total += written;
        println!("{key}  {what}  {}", pages(written));
    }
    if seen == 0 {
        return Err(nothing_message(&root, with));
    }
    println!("{seen} {}, {}", if seen == 1 { "folder" } else { "folders" }, pages(total));
    Ok(())
}

fn pages(n: usize) -> String {
    format!("{n} {} written", if n == 1 { "page" } else { "pages" })
}

fn status(site: Option<&str>, json: bool) -> Result<(), String> {
    let near = match site {
        Some(s) => PathBuf::from(s),
        None => std::env::current_dir().map_err(|e| format!("could not determine current directory: {e}"))?,
    };
    let (site, registry) = open_site(&near)?;
    let rows = derivation_state(&site, &registry);
    if json {
        println!("{}", serde_json::json!({ "folders": rows }));
    } else {
        print!("{}", render_status(&rows));
    }
    Ok(())
}

fn render_status(rows: &[FolderDerivationState]) -> String {
    if rows.is_empty() {
        return "No folder holds files an installed plugin can turn into pages.\n".to_string();
    }
    let mut out = String::new();
    for r in rows {
        let noun = if r.count == 1 { &r.noun_one } else { &r.noun_other };
        let decision = match r.decision {
            Decision::Undecided => "undecided",
            Decision::Show(_) => "shown as pages",
            Decision::Keep => "kept as files",
        };
        out.push_str(&format!("{}  {} {noun}  {decision}", r.folder, r.count));
        if r.stubs_missing > 0 {
            out.push_str(&format!("  ({} pages missing)", r.stubs_missing));
        }
        out.push('\n');
        for s in &r.skipped {
            out.push_str(&format!("  skipped {}: {}\n", s.file, s.message));
        }
        for f in &r.failed {
            out.push_str(&format!("  could not read {}: {}\n", f.file, f.message));
        }
    }
    out
}
