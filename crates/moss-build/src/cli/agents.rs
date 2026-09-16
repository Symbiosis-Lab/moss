//! `moss agents init` — seed this site's own conventions in `.moss/AGENTS.md`.
//!
//! Only `init` remains. `install-skill` wrote the moss skill into `$HOME`,
//! which is now [`sync`]'s job — into the project, on every full build, with
//! no command to run. See that module for why $HOME was the wrong place.

pub mod skill_package;
pub mod sync;

use std::fs;

const TEMPLATE: &str = include_str!("../assets/agents/template.md");

/// Returns exit code i32, matching the `cli::domain::run` shape.
pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("init") => run_init(&args[1..]),
        Some(other) => {
            eprintln!("unknown agents subcommand: {}", other);
            1
        }
        None => {
            eprintln!("usage: moss agents init");
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

    println!();
    println!("This file is for YOUR site's conventions. moss's own conventions are");
    println!("written for your coding agent on every build — nothing to run.");
    0
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
