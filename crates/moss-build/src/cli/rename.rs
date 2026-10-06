//! `moss rename <old> <new>` — rename a file or folder and rewrite every
//! project-wide reference to it.
//!
//! This is the CLI counterpart to the editor's rename-with-refs (the file tree
//! calls the app's `rename_entry_with_refs` command). Both binaries answer it
//! through `editor::ref_scan::rename_entry_with_refs_core`: the OS rename
//! happens first, then every `[[wikilink]]` / `[text](link.md)` reference
//! across the project is rewritten to the new name. Scriptable for bulk renames (e.g. localizing
//! filenames while keeping links intact).

use crate::vault_root::{resolve_input_in, VaultRoot};
use std::path::Path;

/// Entry point for `moss rename`. Returns an exit code (0 = success).
pub fn run(args: &[String]) -> i32 {
    let positional: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if positional.len() < 2 {
        eprintln!("Usage: moss rename <old_path> <new_path>");
        eprintln!();
        eprintln!("Renames a file or folder and rewrites every [[wikilink]] and");
        eprintln!("[text](link) reference to it across the project. Paths may be");
        eprintln!("relative to the current directory.");
        eprintln!();
        eprintln!("Tip: pair with a `url:` frontmatter pin to keep the published");
        eprintln!("URL stable when the filename changes language.");
        return 1;
    }

    let cwd = std::env::current_dir().unwrap_or_default();
    let owned: Vec<String> = positional.into_iter().cloned().collect();
    match rename_in(&owned, &cwd, dirs::home_dir().as_deref()) {
        Ok(line) => {
            println!("{line}");
            0
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            1
        }
    }
}

/// Testable core of [`run`]: `args` are the two positional paths, `cwd` is
/// what relative paths resolve against, `home` is the user's home folder.
/// Returns the success line.
fn rename_in(args: &[String], cwd: &Path, home: Option<&Path>) -> Result<String, String> {
    let old_abs = resolve_input_in(Path::new(&args[0]), cwd);
    let new_abs = resolve_input_in(Path::new(&args[1]), cwd);

    if !old_abs.exists() {
        return Err(format!("source path does not exist: {}", old_abs.display()));
    }

    // The project root is the folder that owns `.moss/`, found by walking up
    // from the (still-present) source path. A folder that was never built has
    // no such owner; `VaultRoot::containing` would then answer with the file's
    // parent folder and pages outside it would never be scanned, so the
    // current directory stands in, provided both paths are inside it and the
    // build would accept it as a site: not the home folder, a cloud drive's
    // root or the disk, and not a folder holding other sites.
    let root = match VaultRoot::find_containing_below(&old_abs, cwd, home) {
        Some(root) => root.path().to_path_buf(),
        None => {
            let cwd = resolve_input_in(cwd, cwd);
            crate::cli::site_guard::guard_cli_open_in(&cwd.to_string_lossy(), "rename", home)?;
            if !old_abs.starts_with(&cwd) || !new_abs.starts_with(&cwd) {
                return Err(format!(
                    "no site found: neither {} nor any folder above it has a .moss folder, \
                     and the paths are not both inside the current directory. Run the \
                     command from the folder that holds the site, or run `moss build` \
                     there once first.",
                    old_abs.display()
                ));
            }
            cwd
        }
    };

    let old_s = old_abs.to_string_lossy().to_string();
    let new_s = new_abs.to_string_lossy().to_string();
    let result = crate::editor::ref_scan::rename_entry_with_refs_core(root, &old_s, &new_s)?;

    let refs = result.edits.len();
    let files = result
        .edits
        .iter()
        .map(|e| e.file.as_str())
        .collect::<std::collections::HashSet<_>>()
        .len();
    let outcome = if refs == 0 {
        "no references to rewrite".to_string()
    } else {
        format!(
            "{refs} reference{} rewritten in {files} file{}",
            if refs == 1 { "" } else { "s" },
            if files == 1 { "" } else { "s" }
        )
    };
    Ok(format!("Renamed {} → {} ({outcome})", args[0], args[1]))
}

#[cfg(test)]
#[path = "rename_tests.rs"]
mod tests;
