//! The two scanning primitives every source-scan invariant test shares: which
//! lines sit inside a `#[cfg(test)]` item, and a walk over a tree's `.rs` files.
//! Each consumer owns its own patterns, exemptions and message.

use std::fs;
use std::path::Path;

/// Line numbers (0-indexed) that sit inside a `#[cfg(test)]` item.
pub fn cfg_test_lines(lines: &[&str]) -> Vec<bool> {
    let mut skipped = vec![false; lines.len()];
    let mut idx = 0;
    while idx < lines.len() {
        // An attribute, not a doc comment that mentions one: a comment naming
        // `#[cfg(test)]` used to hide the item after it from every scan.
        if !lines[idx].trim_start().starts_with("#[cfg(test)]") {
            idx += 1;
            continue;
        }
        let mut depth = 0usize;
        let mut opened = false;
        let mut j = idx;
        while j < lines.len() {
            skipped[j] = true;
            let mut ended = false;
            for ch in lines[j].chars() {
                match ch {
                    '{' => {
                        depth += 1;
                        opened = true;
                    }
                    '}' => depth = depth.saturating_sub(1),
                    ';' if !opened => ended = true,
                    _ => {}
                }
            }
            if ended || (opened && depth == 0) {
                break;
            }
            j += 1;
        }
        idx = j + 1;
    }
    skipped
}

/// Walks a directory, or visits a single `.rs` file.
pub fn walk_rust_files(dir: &Path, visit: &mut dyn FnMut(&Path)) {
    if dir.is_file() {
        visit(dir);
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "target" || name.starts_with('.') {
                continue;
            }
            walk_rust_files(&path, visit);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            visit(&path);
        }
    }
}
