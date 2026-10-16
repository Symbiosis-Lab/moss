//! `moss migrate` — rewrite what `moss doctor --surface` finds, with the
//! author's say-so.
//!
//! Nothing is written without `--apply`; a run without it prints the change
//! as a diff. With `--apply`, a version of the site is saved first, as
//! `moss history --save` does, so `moss history --restore` undoes the run.
//! Only frontmatter lines the change names are touched (see
//! `moss_core::frontmatter::edit_fields`).
//!
//! Two kinds of rewrite, told apart by whether the site looks different after:
//!
//! - A rewrite that should change nothing a reader sees runs on its own, and
//!   only once the site has been built from a scratch copy with and without
//!   it to the same output. Today that is a `translationKey` the page's path
//!   already implies (`translation_keys.rs`).
//! - Lifting a classed div into a declared field (`--lift`, `lift.rs`)
//!   changes the page on purpose — moss draws the field instead — so it is
//!   asked for one class at a time and never runs on its own.

use std::path::{Path, PathBuf};

use crate::vault_root::{resolve_input, VaultRoot};

mod lift;
mod translation_keys;

/// One file's text before and after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    /// Site-relative path with forward slashes.
    pub path: String,
    pub before: String,
    pub after: String,
}

/// A page left as written, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub path: String,
    /// The line the reason is about, when there is one.
    pub line: Option<usize>,
    pub reason: String,
}

const CONFIG: &str = ".moss/config.toml";

#[derive(Debug, Default, PartialEq, Eq)]
struct Args {
    folder: Option<String>,
    apply: bool,
    json: bool,
    lift: Option<String>,
    field: Option<String>,
    place: Option<String>,
}

/// What a usage error prints, as the command's other errors do.
fn usage_error(message: &str, json: bool) -> i32 {
    if json {
        println!("{}", serde_json::json!({ "error": message }));
    } else {
        eprintln!("error: {message}\n\n{}", usage());
    }
    2
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut out = Args::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| it.next().filter(|v| !v.starts_with("--")).cloned().ok_or_else(|| format!("{flag} needs a value"));
        match arg.as_str() {
            "--apply" => out.apply = true,
            "--json" => out.json = true,
            "--lift" => out.lift = Some(value("--lift")?.trim_start_matches('.').to_string()),
            "--as" => out.field = Some(value("--as")?),
            "--place" => out.place = Some(value("--place")?),
            flag if flag.starts_with('-') => return Err(format!("unknown flag {flag}")),
            folder if out.folder.is_none() => out.folder = Some(folder.to_string()),
            extra => return Err(format!("unexpected argument {extra}")),
        }
    }
    if out.lift.is_some() != out.field.is_some() {
        return Err("--lift <class> and --as <field> go together".to_string());
    }
    if out.place.is_some() && out.lift.is_none() {
        return Err("--place goes with --lift".to_string());
    }
    Ok(out)
}

fn usage() -> &'static str {
    "Usage: moss migrate [<folder>] [--apply] [--json]
       moss migrate [<folder>] --lift <class> --as <field> [--place <place>] [--apply] [--json]

Rewrites what `moss doctor --surface` finds. Without --apply it prints the
change as a diff and writes nothing; with --apply it saves a version of the
site first (undo with `moss history --restore`), then writes.

With no --lift, it removes a `translationKey` that the page's path already
implies, and only when building the site with and without it gives the same
output.

It compares the built site only: the scratch builds run the site's plugins as
a build does, but a tool that reads translationKey from the files themselves is
not checked, and a page renamed later pairs by key and not by path.

--lift <class> --as <field> moves each `::: {.class}` div holding one paragraph
of plain text into the page's frontmatter as <field>, declares <field> in
.moss/config.toml if it is not, and lists the pages it left as written.
--place (eyebrow, subtitle, key-figure, facts or none) sets where moss draws it."
}

pub fn run(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{}", usage());
        return 0;
    }
    let json = args.iter().any(|a| a == "--json");
    let args = match parse_args(args) {
        Ok(args) => args,
        Err(e) => return usage_error(&e, json),
    };
    let folder = args.folder.as_ref().map_or_else(|| std::env::current_dir().unwrap_or_default(), resolve_input);
    if !folder.join(".moss").is_dir() {
        return fail(&format!("not a moss site (no .moss folder): {}", folder.display()), args.json);
    }
    if let Err(msg) = crate::cli::site_guard::guard_cli_open(&folder.to_string_lossy(), "migrate") {
        return fail(&msg, args.json);
    }
    match migrate(&folder, &args) {
        Ok(report) => {
            if args.json {
                println!("{}", report.to_json());
            } else {
                print!("{}", report.to_text());
            }
            0
        }
        Err(e) => fail(&e, args.json),
    }
}

fn fail(message: &str, json: bool) -> i32 {
    if json {
        println!("{}", serde_json::json!({ "error": message }));
    } else {
        eprintln!("error: {message}");
    }
    1
}

/// Whether the rewrites were checked against a build, and how it came out.
#[derive(Debug, PartialEq, Eq)]
enum Check {
    /// Lifts change the page on purpose; nothing to compare.
    NotNeeded,
    SameOutput,
    DifferentOutput,
}

#[derive(Debug)]
struct Report {
    changes: Vec<FileChange>,
    skipped: Vec<Skipped>,
    notes: Vec<String>,
    check: Check,
    /// The version saved before writing, when `--apply` wrote.
    saved: Option<String>,
    applied: bool,
}

fn migrate(folder: &Path, args: &Args) -> Result<Report, String> {
    let mut report = plan(folder, args)?;
    if !(args.apply && report.writable()) {
        return Ok(report);
    }
    // Saving a version builds the site, and a build stamps `uid:` into a page
    // that has none, so the change is planned again from the files it left.
    let root = VaultRoot::resolve(folder);
    let label = match &args.lift {
        Some(class) => format!("Before moss migrate --lift .{class}"),
        None => "Before moss migrate".to_string(),
    };
    let (id, _) = crate::deploy::history::cli::save_version(&root, Some(label)).map_err(|e| format!("{e} — nothing was changed"))?;
    report = plan(folder, args)?;
    if !report.writable() {
        return Ok(report);
    }
    let store = crate::deploy::history::HistoryStore::in_vault(folder);
    for change in &report.changes {
        let new_file = change.before.is_empty() && !folder.join(&change.path).exists();
        if new_file {
            continue;
        }
        match store.read_version(&id, &change.path) {
            Ok(kept) if kept == change.before.as_bytes() => {}
            Ok(_) => return Err(format!("version {id} holds a different {} — nothing was changed", change.path)),
            Err(e) => return Err(format!("version {id} cannot give back {}: {e} — nothing was changed", change.path)),
        }
    }
    report.saved = Some(id);
    write_all(folder, &mut report.changes, &mut report.skipped);
    report.applied = true;
    Ok(report)
}

/// What a run would change, without writing anything.
fn plan(folder: &Path, args: &Args) -> Result<Report, String> {
    let pages: Vec<(String, String)> = super::markdown_files(folder)
        .into_iter()
        .filter_map(|p| Some((site_relative(folder, &p), std::fs::read_to_string(&p).ok()?)))
        .collect();
    let config_text = match std::fs::read_to_string(folder.join(CONFIG)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("could not read {CONFIG}: {e}")),
    };

    let mut notes = Vec::new();
    let (changes, skipped, check) = match (&args.lift, &args.field) {
        (Some(class), Some(field)) => {
            let config = crate::config::ConfigFile::parse(&config_text)?;
            let (decls, _) = config.field_decls();
            let place = match args.place.as_deref() {
                None => None,
                Some("none") => Some(None),
                Some(name) => Some(Some(moss_core::fields::parse_place(name).ok_or_else(|| {
                    format!("unknown place \"{name}\"; one of: {}, none", moss_core::fields::place_names().join(", "))
                })?)),
            };
            let lang = crate::build::site_config::get_site_lang(&folder.to_string_lossy()).ok().flatten();
            let lang = crate::i18n::Language::from_code(lang.as_deref().unwrap_or("en")).unwrap_or(crate::i18n::Language::En);
            let format = moss_core::fields::PageFormat {
                lang: lang.as_bcp47_attr(),
                decimal: crate::build::components::field_text::decimal_mark(lang),
            };
            let request = lift::LiftRequest { class, field, place };
            let plan = lift::plan(&request, &pages, &config_text, &decls, format)?;
            if plan.unstyled > 0 {
                notes.push(format!(
                    "On {} the div was wholly in italics or bold; the value drops it, so set the style on [data-field=\"{field}\"] in the theme.",
                    if plan.unstyled == 1 { "1 page".to_string() } else { format!("{} pages", plan.unstyled) }
                ));
            }
            let mut changes = plan.pages;
            if let Some(after) = plan.config {
                changes.push(FileChange { path: CONFIG.to_string(), before: config_text.clone(), after });
            }
            if !changes.is_empty() && theme_mentions(folder, class) {
                notes.push(format!(
                    "The theme still styles .{class}. Once the pages no longer carry it, restyle the field with [data-field=\"{field}\"]; `moss doctor --surface` lists rules that match nothing."
                ));
            }
            (changes, plan.skipped, Check::NotNeeded)
        }
        _ => {
            let (changes, skipped) = translation_keys::redundant(&pages);
            let check = if changes.is_empty() {
                Check::NotNeeded
            } else if output_unchanged(folder, &changes)? {
                Check::SameOutput
            } else {
                Check::DifferentOutput
            };
            if !changes.is_empty() {
                notes.push("A tool that reads translationKey from the files themselves is not part of this check, and a page renamed later pairs by key and not by path.".to_string());
            }
            (changes, skipped, check)
        }
    };

    let mut report = Report { changes, skipped, notes, check, saved: None, applied: false };
    if report.check == Check::DifferentOutput {
        report.notes.push("Removing these would change the built site, so they stay as written.".to_string());
    }
    Ok(report)
}

/// Write every change whose file still holds the text it was planned from,
/// the config first. A file that changed in the meantime, or that cannot be
/// written, is left as it was and listed as skipped; `changes` keeps what
/// was written.
fn write_all(folder: &Path, changes: &mut Vec<FileChange>, skipped: &mut Vec<Skipped>) {
    changes.sort_by_key(|c| c.path != CONFIG);
    let mut written = Vec::new();
    for change in changes.drain(..) {
        let path = folder.join(&change.path);
        let now = std::fs::read_to_string(&path).unwrap_or_default();
        if now != change.before {
            skipped.push(Skipped { path: change.path, line: None, reason: "the file changed while moss was working".to_string() });
            continue;
        }
        let write = || {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            // allow:raw_write the author's source file, rewritten at their request
            std::fs::write(&path, &change.after)
        };
        match write() {
            Ok(()) => written.push(change),
            Err(e) => skipped.push(Skipped { path: change.path, line: None, reason: format!("could not be written: {e}") }),
        }
    }
    *changes = written;
}

/// Whether building a scratch copy of the site with `changes` gives the same
/// output as building it without them.
fn output_unchanged(folder: &Path, changes: &[FileChange]) -> Result<bool, String> {
    let scratch = tempfile::tempdir().map_err(|e| format!("could not make a scratch copy: {e}"))?;
    copy_site(folder, scratch.path())?;
    let root = VaultRoot::resolve(scratch.path());
    let build = |what: &str| {
        crate::deploy::history::cli::headless_build_sealed(&root)
            .map(|sealed| sealed.generation_id().to_string())
            .map_err(|e| format!("could not build the scratch copy {what}: {e}"))
    };
    let before = build("as it is")?;
    for change in changes {
        // allow:raw_write a scratch copy in the system temp folder
        std::fs::write(scratch.path().join(&change.path), &change.after).map_err(|e| format!("could not write the scratch copy: {e}"))?;
    }
    Ok(build("with the changes")? == before)
}

/// The site's files, without its build output, version history and git
/// store. A symlink is copied as a symlink.
fn copy_site(from: &Path, to: &Path) -> Result<(), String> {
    let skip = |rel: &Path| [".git", ".moss/build.nosync", ".moss/build", ".moss/history"].iter().any(|s| rel == Path::new(s));
    for entry in walkdir::WalkDir::new(from).into_iter().filter_entry(|e| !skip(e.path().strip_prefix(from).unwrap_or(e.path()))) {
        let entry = entry.map_err(|e| format!("could not copy the site: {e}"))?;
        let dest: PathBuf = to.join(entry.path().strip_prefix(from).unwrap_or(entry.path()));
        let result = if entry.file_type().is_dir() {
            std::fs::create_dir_all(&dest)
        } else if entry.file_type().is_file() {
            // allow:raw_write a scratch copy in the system temp folder
            std::fs::copy(entry.path(), &dest).map(|_| ())
        } else if entry.path_is_symlink() {
            copy_symlink(entry.path(), &dest)
        } else {
            Ok(())
        };
        result.map_err(|e| format!("could not copy {} to a scratch folder: {e}", entry.path().display()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn copy_symlink(link: &Path, dest: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(std::fs::read_link(link)?, dest)
}

#[cfg(not(unix))]
fn copy_symlink(link: &Path, dest: &Path) -> std::io::Result<()> {
    // allow:raw_write a scratch copy in the system temp folder
    std::fs::copy(link, dest).map(|_| ())
}

fn theme_mentions(folder: &Path, class: &str) -> bool {
    walkdir::WalkDir::new(folder.join(".moss").join("theme"))
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("css"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .any(|css| crate::cli::describe::css_query::mentions(&css, &format!(".{class}")))
}

fn site_relative(folder: &Path, file: &Path) -> String {
    file.strip_prefix(folder).unwrap_or(file).to_string_lossy().replace('\\', "/")
}

impl Report {
    fn writable(&self) -> bool {
        self.check != Check::DifferentOutput && !self.changes.is_empty()
    }

    fn to_text(&self) -> String {
        let mut out = String::new();
        for change in &self.changes {
            out.push_str(&format!("--- {0}\n+++ {0}\n{1}", change.path, diff(&change.before, &change.after)));
        }
        if !self.skipped.is_empty() {
            out.push_str("\nLeft as written:\n");
            for s in &self.skipped {
                let loc = s.line.map_or_else(|| s.path.clone(), |line| format!("{}:{line}", s.path));
                out.push_str(&format!("  {loc}  {}\n", s.reason));
            }
        }
        for note in &self.notes {
            out.push_str(&format!("\n{note}\n"));
        }
        let files = |n: usize| if n == 1 { "1 file".to_string() } else { format!("{n} files") };
        out.push('\n');
        out.push_str(&match (&self.saved, self.changes.len(), &self.check) {
            (_, 0, _) if self.skipped.is_empty() && self.notes.is_empty() => "Nothing to rewrite.\n".to_string(),
            (_, _, Check::DifferentOutput) => "Nothing was written.\n".to_string(),
            (Some(id), n, _) if self.applied => format!(
                "Wrote {}. A version of the site from before was saved as {id}; undo with `moss history --restore --at {id} --yes`.\n",
                files(n)
            ),
            (_, 0, _) => "Nothing to rewrite.\n".to_string(),
            (_, n, check) => format!(
                "{}This would write {}. Nothing was written; run again with --apply to write it.\n",
                if *check == Check::SameOutput { "The site builds to the same output with these changes. " } else { "" },
                files(n)
            ),
        });
        out
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "changes": self.changes.iter().map(|c| serde_json::json!({ "path": c.path, "diff": diff(&c.before, &c.after) })).collect::<Vec<_>>(),
            "skipped": self.skipped.iter().map(|s| serde_json::json!({ "path": s.path, "line": s.line, "reason": s.reason })).collect::<Vec<_>>(),
            "notes": self.notes,
            "same_output": match self.check { Check::NotNeeded => serde_json::Value::Null, Check::SameOutput => true.into(), Check::DifferentOutput => false.into() },
            "applied": self.applied,
            "saved_version": self.saved,
        })
    }
}

/// Above this many cells, the differing middle of a diff is shown whole as
/// removed and added rather than matched line by line.
const MAX_DIFF_CELLS: usize = 4_000_000;

/// A line diff of `before` and `after` in unified-diff hunks with one line of
/// context.
fn diff(before: &str, after: &str) -> String {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    if a == b {
        return if before == after { String::new() } else { "@@ line endings or the final newline change @@\n".to_string() };
    }
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..].iter().rev().zip(b[prefix..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (am, bm) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    // Longest common subsequence of the differing middle; all zeros when it is too large to match.
    let matched = (am.len() + 1) * (bm.len() + 1) <= MAX_DIFF_CELLS;
    let mut lcs = vec![vec![0u32; if matched { bm.len() + 1 } else { 1 }]; if matched { am.len() + 1 } else { 1 }];
    if matched {
        for i in (0..am.len()).rev() {
            for j in (0..bm.len()).rev() {
                lcs[i][j] = if am[i] == bm[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
            }
        }
    }
    let at = |i: usize, j: usize| if matched { lcs[i][j] } else { 0 };
    // (old line, tag, text), the tag ' ', '-' or '+'.
    let mut ops: Vec<(usize, char, &str)> = a[..prefix].iter().enumerate().map(|(i, l)| (i, ' ', *l)).collect();
    let (mut i, mut j) = (0, 0);
    while i < am.len() || j < bm.len() {
        if matched && i < am.len() && j < bm.len() && am[i] == bm[j] {
            ops.push((prefix + i, ' ', am[i]));
            i += 1;
            j += 1;
        } else if j < bm.len() && (i == am.len() || at(i, j + 1) > at(i + 1, j)) {
            ops.push((prefix + i, '+', bm[j]));
            j += 1;
        } else {
            ops.push((prefix + i, '-', am[i]));
            i += 1;
        }
    }
    ops.extend(a[a.len() - suffix..].iter().enumerate().map(|(k, l)| (a.len() - suffix + k, ' ', *l)));

    let changed: Vec<usize> = ops.iter().enumerate().filter(|(_, op)| op.1 != ' ').map(|(k, _)| k).collect();
    let mut out = String::new();
    let mut k = 0;
    while k < changed.len() {
        let from = changed[k].saturating_sub(1);
        let mut to = changed[k];
        while k + 1 < changed.len() && changed[k + 1] <= to + 3 {
            k += 1;
            to = changed[k];
        }
        let to = (to + 1).min(ops.len() - 1);
        out.push_str(&format!("@@ line {} @@\n", ops[from].0 + 1));
        for op in &ops[from..=to] {
            out.push_str(&format!("{}{}\n", op.1, op.2));
        }
        k += 1;
    }
    out
}

#[cfg(test)]
#[path = "migrate_tests.rs"]
mod tests;
