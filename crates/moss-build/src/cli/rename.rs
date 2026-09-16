//! `moss rename <old> <new>` — rename a file or folder and rewrite every
//! project-wide reference to it.
//!
//! This is the CLI counterpart to the editor's rename-with-refs (the file tree
//! calls the app's `rename_entry_with_refs` command). Both binaries answer it
//! through `editor::ref_scan::rename_entry_with_refs_core`: the OS rename
//! happens first, then every `[[wikilink]]` / `[text](link.md)` reference
//! across the project is rewritten to the new name. Scriptable for bulk renames (e.g. localizing
//! filenames while keeping links intact).

use crate::vault_root::{resolve_input, VaultRoot};

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

    let old_abs = resolve_input(positional[0]);
    let new_abs = resolve_input(positional[1]);

    if !old_abs.exists() {
        eprintln!("Error: source path does not exist: {}", old_abs.display());
        return 1;
    }

    // The project root is the folder that owns `.moss/`, found by walking up
    // from the (still-present) source path.
    let root = VaultRoot::containing(&old_abs);

    let old_s = old_abs.to_string_lossy().to_string();
    let new_s = new_abs.to_string_lossy().to_string();

    match crate::editor::ref_scan::rename_entry_with_refs_core(root.path().to_path_buf(), &old_s, &new_s) {
        Ok(()) => {
            println!("Renamed {} → {} (references updated)", positional[0], positional[1]);
            0
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            1
        }
    }
}
