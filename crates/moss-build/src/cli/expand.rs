//! `moss expand <page.md|folder>` — write embeds out as text.
//!
//! A line that holds only `![[report.pdf#text]]` (any file a plugin derives
//! text from) becomes that text, and a line that holds only `![[note]]` or
//! `![[note#heading]]` becomes the note's body or section, so the page can be
//! edited as plain markdown, or kept when the source file is not. Embeds whose
//! text is not available (skipped, unreadable, unresolved) stay as written.
//! Text that needs deriving is derived first, with progress on stderr.

use std::path::{Path, PathBuf};

use moss_core::resolve::embeds::{derived::Derivations, expand::expand_page};

use crate::system::derive::site::DerivedSite;
use crate::moss_paths::MossPaths;
use crate::vault_root::{resolve_input_in, VaultRoot};

const USAGE: &str = "Usage: moss expand <page.md|folder> [--dry-run] [--allow-plugins]";

/// Entry point for `moss expand`. Returns an exit code (0 = success).
pub fn run(args: &[String]) -> i32 {
    let positional: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let allow_plugins = args.iter().any(|a| a == "--allow-plugins");
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}\n\nReplaces each line holding only a ready `![[file.ext#fragment]]` derived embed or a `![[note]]` / `![[note#heading]]` transclusion with the text it stands for. Files are written only when something changed; --dry-run reports without writing.");
        return 0;
    }
    let [target] = positional.as_slice() else {
        eprintln!("{USAGE}");
        return 1;
    };
    crate::plugins::install::registry_client::enforce::install_headless(allow_plugins);
    let cwd = std::env::current_dir().unwrap_or_default();
    match expand_in(Path::new(target.as_str()), &cwd, dry_run) {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            0
        }
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

/// Testable core of [`run`]: the report lines.
pub fn expand_in(target: &Path, cwd: &Path, dry_run: bool) -> Result<Vec<String>, String> {
    let abs = resolve_input_in(target, cwd);
    if !abs.exists() {
        return Err(format!("no such file or folder: {}", abs.display()));
    }
    let root = VaultRoot::find_containing_in(&abs, cwd).ok_or_else(|| {
        format!("{} is not inside a moss site (no .moss folder above it)", abs.display())
    })?;
    let root_path = root.path().to_path_buf();
        crate::vault::derive_state::site_registry(&MossPaths::new(&root_path))?;
    let structure = crate::build::scan::scan::scan_folder_with_dedup_emit(root.as_str(), None, None, false)?;
    let graph = crate::build::scan::scan::build_content_graph(&structure);

    let scope = abs.strip_prefix(&root_path).unwrap_or(Path::new("")).to_string_lossy().replace('\\', "/");
    let pages: Vec<String> = structure
        .markdown_files
        .iter()
        .map(|f| f.path.clone())
        .filter(|p| scope.is_empty() || *p == scope || p.starts_with(&format!("{}/", scope.trim_end_matches('/'))))
        .collect();
    if pages.is_empty() {
        return Err(format!("no markdown pages at {}", abs.display()));
    }

    let site = DerivedSite::load(true, &root_path);
    if let Some(site) = &site {
        site.prepare(site.demands(&pages, &graph), None);
    }
    let empty = moss_core::resolve::embeds::derived::DerivedTable::new();
    let read_derived = |path: &str, fragment: &str| match &site {
        Some(site) => site.read(path, fragment),
        None => moss_core::resolve::embeds::derived::DerivedFragment::Pending,
    };
    let derivations = Derivations { table: site.as_ref().map_or(&empty, |s| s.table()), reader: &read_derived };
    let read_note = |path: &str| std::fs::read_to_string(root_path.join(path)).ok();

    let (mut report, mut lines_total, mut pages_total) = (Vec::new(), 0usize, 0usize);
    for page in &pages {
        let file: PathBuf = root_path.join(page);
        let Ok(raw) = std::fs::read_to_string(&file) else { continue };
        let expansion = expand_page(page, &raw, &graph, &read_note, &derivations);
        if expansion.expanded.is_empty() || expansion.content == raw {
            continue;
        }
        if !dry_run {
            // Not the temp-file-and-rename writer: that replaces the file, and a page in a
            // synced folder must keep its identity, mode and extended attributes.
            // allow:raw_write the author's own page, edited in place on request
            std::fs::write(&file, &expansion.content).map_err(|e| format!("could not write {page}: {e}"))?;
        }
        pages_total += 1;
        for line in &expansion.expanded {
            lines_total += 1;
            report.push(format!("{page}:{}: {}", line.line, line.embed));
        }
    }
    let verb = if dry_run { "would expand" } else { "expanded" };
    report.push(if lines_total == 0 {
        "nothing to expand".to_string()
    } else {
        format!("{verb} {lines_total} embed{} in {pages_total} page{}", plural(lines_total), plural(pages_total))
    });
    Ok(report)
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
