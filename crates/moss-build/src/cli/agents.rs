//! `moss agents sync` writes the moss pointer for the coding agents installed
//! (Claude Code, Cursor) into a site; `moss agents init` does that and seeds
//! this site's own conventions in `.moss/AGENTS.md`. A build only refreshes
//! what these wrote; see [`sync`] for why it creates nothing.

pub mod skill_package;
pub mod sync;

use std::fs;

const TEMPLATE: &str = include_str!("../assets/agents/template.md");

/// Returns exit code i32, matching the `cli::domain::run` shape.
pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("init") => match run_sync() {
            0 => run_init(&args[1..]),
            code => code,
        },
        Some("sync") => run_sync(),
        Some(other) => {
            eprintln!("unknown agents subcommand: {}", other);
            1
        }
        None => {
            eprintln!("usage: moss agents sync | moss agents init");
            1
        }
    }
}

fn run_sync() -> i32 {
    let project = crate::vault::paths::resolve_input(std::env::current_dir().expect("cwd"));
    if !project.join(".moss").is_dir() {
        eprintln!("error: not in a moss project (no .moss/ directory found at {})", project.display());
        return 1;
    }
    match sync::sync_project(&project) {
        Ok(report) => {
            for path in &report.written {
                println!("wrote {path}");
            }
            for path in &report.user_owned {
                println!("left {path} alone: it is yours, not moss's");
            }
            if report.is_noop() && report.user_owned.is_empty() {
                println!("agent guidance is up to date");
            }
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn run_init(_args: &[String]) -> i32 {
    let project = crate::vault::paths::resolve_input(std::env::current_dir().expect("cwd"));
    let moss_dir = project.join(".moss");

    if !moss_dir.exists() {
        eprintln!(
            "error: not in a moss project (no .moss/ directory found at {})",
            project.display()
        );
        return 1;
    }

    let target = moss_dir.join("AGENTS.md");

    if target.exists() {
        let existing = match fs::read_to_string(&target) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error reading {}: {}", target.display(), e);
                return 1;
            }
        };

        let new_content = match merge_template(&existing, TEMPLATE) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: AGENTS.md cannot be safely merged: {}", e);
                eprintln!("  The file's managed/user-notes markers are missing or malformed.");
                eprintln!("  Inspect {} manually, OR delete it to start fresh.", target.display());
                return 1;
            }
        };

        if new_content == existing {
            println!("{} is up to date", target.display());
            return 0;
        }

        // Back up before write.
        let backup = target.with_extension("md.bak");
        // allow:raw_write the vault's root agent files are the user's documents, not build output
        if let Err(e) = fs::write(&backup, &existing) {
            eprintln!("error writing backup {}: {}", backup.display(), e);
            return 1;
        }

        // allow:raw_write the vault's root agent files are the user's documents, not build output
        if let Err(e) = fs::write(&target, &new_content) {
            eprintln!("error writing {}: {}", target.display(), e);
            return 1;
        }
        println!(
            "{} updated (managed block refreshed; your-notes preserved; backup at {})",
            target.display(),
            backup.display()
        );
    } else {
        // allow:raw_write the vault's root agent files are the user's documents, not build output
        if let Err(e) = fs::write(&target, TEMPLATE) {
            eprintln!("error writing {}: {}", target.display(), e);
            return 1;
        }
        println!("{} created", target.display());
    }

    match write_root_pointer(&project) {
        Ok(true) => println!("{} created", project.join(ROOT_POINTER).display()),
        Ok(false) => println!("left {ROOT_POINTER} alone: it is yours. To point other agents at moss, add: {ROOT_POINTER_HINT}"),
        Err(e) => {
            eprintln!("error writing {}: {e}", project.join(ROOT_POINTER).display());
            return 1;
        }
    }

    println!();
    println!("This file is for YOUR site's conventions. moss's own conventions are");
    println!("`moss guide`, and every build keeps the pointer files above current.");
    0
}

/// The root file Codex, Gemini CLI, Copilot and other tools read first.
const ROOT_POINTER: &str = "AGENTS.md";

const ROOT_POINTER_HINT: &str = "This folder is a moss site. Run `moss guide` before editing it.";

const ROOT_POINTER_TEXT: &str = "# moss site

This folder is a moss site. Run `moss guide` before editing it.
`moss describe --json` lists under `paths` where you may write; `.moss/AGENTS.md` holds this site's own conventions.
";

/// Write the root `AGENTS.md` pointer when the folder has none. Only
/// `moss agents init` calls this: a build or `moss agents sync` never does,
/// so a person who deletes the file does not get it back. An existing file is
/// the person's and is left as it is. Returns whether it wrote.
fn write_root_pointer(project: &std::path::Path) -> std::io::Result<bool> {
    let path = project.join(ROOT_POINTER);
    if path.symlink_metadata().is_ok() {
        return Ok(false);
    }
    // allow:raw_write the vault's root agent files are the user's documents, not build output
    fs::write(path, ROOT_POINTER_TEXT)?;
    Ok(true)
}

fn merge_template(existing: &str, new_template: &str) -> Result<String, String> {
    let user_notes = extract_user_notes(existing)?;
    Ok(replace_user_notes(new_template, &user_notes))
}

fn extract_user_notes(content: &str) -> Result<String, String> {
    let start = "<!-- your-notes; preserved across moss updates. Edit freely. -->";
    let end = "<!-- end your-notes -->";
    let start_idx = content
        .find(start)
        .ok_or("missing `<!-- your-notes ... -->` start marker")?;
    let end_idx = content
        .find(end)
        .ok_or("missing `<!-- end your-notes -->` end marker")?;
    let after_start = start_idx + start.len();
    if after_start >= end_idx {
        return Err("user-notes start marker appears after end marker".to_string());
    }
    // Both indices came from `find` on ASCII comment markers.
    Ok(content
        .get(after_start..end_idx)
        .unwrap_or_default()
        .to_string())
}

fn replace_user_notes(template: &str, notes: &str) -> String {
    let start = "<!-- your-notes; preserved across moss updates. Edit freely. -->";
    let end = "<!-- end your-notes -->";
    let start_idx = template.find(start).expect("template missing start marker");
    let end_idx = template.find(end).expect("template missing end marker");
    let after_start = start_idx + start.len();
    // Both indices came from `find` on ASCII comment markers.
    let mut out = String::new();
    out.push_str(template.get(..after_start).unwrap_or_default());
    out.push_str(notes);
    out.push_str(template.get(end_idx..).unwrap_or_default());
    out
}

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;
