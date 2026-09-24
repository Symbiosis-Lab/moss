//! Sync test: moss must not launch another application's UI.
//!
//! Open-half twin of the desktop app's `no_vendor_ui_launch_test.rs` — that
//! file scans two independent `SOURCE_ROOTS` (desktop `src`, plus
//! `../open/crates/moss-build/src`) with no cross-root comparison. This
//! twin runs the same lint over this crate's own `src`; the desktop half
//! keeps scanning its own `src`.
//!
//! moss drives OnionPress's CLI. It does not open OnionPress's windows.
//!
//! ## The marker convention
//!
//! Launching is not banned outright — a "Reveal in Finder" or an `open` of a
//! user-chosen document is a legitimate thing moss does on the user's
//! explicit request. Every such call MUST carry `// allow:vendor_ui <reason>`
//! on the same line or the line immediately before.

use std::fs;
use std::path::PathBuf;

#[path = "support/scanned_roots_nonempty.rs"]
mod support;
use support::scanned_roots_nonempty;

/// Production code only. Tests may shell out to `open` in scaffolding without
/// any of this mattering.
const SOURCE_ROOTS: &[&str] = &["src"];

/// `NSWorkspace` alone is deliberately NOT here — moss uses it for
/// `setIcon_forFile_options`, which launches nothing.
const LAUNCH_CALL_PATTERNS: &[&str] = &[
    "Command::new(\"open\")",
    "launchApplication",
    "openApplicationAtURL",
];

fn is_call_site_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with('*') {
        return false;
    }
    LAUNCH_CALL_PATTERNS.iter().any(|p| line.contains(p))
}

fn has_allow_marker(this_line: &str, prev_line: Option<&str>) -> bool {
    let marker = "allow:vendor_ui";
    this_line.contains(marker) || prev_line.is_some_and(|p| p.contains(marker))
}

#[test]
fn app_launches_carry_allow_marker() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut violations: Vec<(PathBuf, usize, String)> = Vec::new();
    let mut scanned = 0usize;

    for root in SOURCE_ROOTS {
        let root_path = crate_root.join(root);
        if !root_path.exists() {
            continue;
        }
        walk_rust_files(&root_path, &mut |_| scanned += 1);
        walk_rust_files(&root_path, &mut |path| {
            let Ok(content) = fs::read_to_string(path) else {
                return;
            };
            let lines: Vec<&str> = content.lines().collect();
            for (idx, line) in lines.iter().enumerate() {
                if !is_call_site_line(line) {
                    continue;
                }
                let prev = if idx > 0 { Some(lines[idx - 1]) } else { None };
                if !has_allow_marker(line, prev) {
                    violations.push((path.to_path_buf(), idx + 1, line.trim().to_string()));
                }
            }
        });
    }

    scanned_roots_nonempty(&crate_root, SOURCE_ROOTS, scanned);

    if !violations.is_empty() {
        let mut msg = String::from(
            "Found a call that launches another application, with no\n\
             `// allow:vendor_ui <reason>` marker.\n\n\
             moss drives vendor binaries through their CLI and re-renders what the user\n\
             needs on moss's own surfaces. It does not open their windows.\n\n\
             Recovery:\n  \
             1. If moss is starting a vendor service: drive its CLI and surface progress\n     \
                yourself.\n  \
             2. If the user explicitly asked to open something: add\n     \
                `// allow:vendor_ui <reason>` naming that action.\n\n\
             Violations:\n",
        );
        for (path, line_num, line) in &violations {
            let rel = path.strip_prefix(&crate_root).unwrap_or(path).display();
            msg.push_str(&format!("  - {rel}:{line_num}: {line}\n"));
        }
        panic!("{msg}");
    }
}

fn walk_rust_files(dir: &std::path::Path, visit: &mut dyn FnMut(&std::path::Path)) {
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
