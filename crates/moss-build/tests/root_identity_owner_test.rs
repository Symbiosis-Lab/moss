//! Regression guard: only `vault_root` derives a vault root or its name.
//!
//! Open-half twin of the desktop app's `root_identity_owner_test.rs` — that
//! file's `scan_roots()` walks the desktop app's own source plus every
//! `open/crates/*/src` dynamically, applying one ownership-prefix rule per
//! file with no cross-root comparison. This twin runs the same four rules
//! over every `crates/*/src` in THIS repo (moss-core, moss-build,
//! moss-cli); the desktop half keeps scanning its own `src`.
//!
//! Not carried here: the desktop file's
//! `the_scanner_scope_follows_the_code_into_crates` test, which asserts the
//! app crate's own source is part of the scan — that assertion is about the
//! desktop/open boundary specifically and has no equivalent in a standalone
//! open-repo scan.
//!
//! Four rules, guarding the two halves of "which folder is the root, and what
//! is it called": three catch a re-derived NAME; the fourth catches a
//! re-derived ROOT.
//!
//! Escape hatch: `// allow:root-name <reason>` on the offending line.

use std::fs;
use std::path::{Path, PathBuf};

#[path = "support/scanned_roots_nonempty.rs"]
mod support;
use support::scanned_roots_nonempty;

/// The one module allowed to answer "which folder, and what is it called".
/// Only the open-repo spelling — the desktop app's own re-export shim has
/// no equivalent here.
const OWNER_PREFIXES: &[&str] = &["crates/moss-build/src/vault_root"];

fn is_owner(name: &str) -> bool {
    OWNER_PREFIXES.iter().any(|p| name.contains(p))
}

const ALLOW_MARKER: &str = "allow:root-name";

const ROOT_IDENTS: &[&str] = &[
    "project_path",
    "folder_path",
    "source_path",
    "project_root",
    "folder_arg",
    "vault_root",
    "root_path",
];

const BANNED_ALGORITHMS: &[(&str, &str)] = &[
    ("components().filter_map", ".last()"),
    ("split(['/','\\\\'])", ".next_back()"),
    ("split(['/','\\\\'])", ".last()"),
    ("split(['/','\\\\'])", ".pop()"),
    ("rfind('/')", "+1..]"),
    ("rfind(\"/\")", "+1..]"),
    ("rsplitn(2,'/')", ".next()"),
];

const CHAIN_WINDOW: usize = 300;

/// The repo checkout root — this crate's `crates/<name>` parent's parent.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/<name> has a parent")
        .parent()
        .expect("crates/ has a parent")
        .to_path_buf()
}

/// Every workspace crate's `src/`.
fn scan_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(entries) = fs::read_dir(repo_root().join("crates")) {
        let mut crate_srcs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path().join("src"))
            .filter(|p| p.is_dir())
            .collect();
        crate_srcs.sort();
        roots.extend(crate_srcs);
    }
    roots
}

fn all_rust_files() -> Vec<PathBuf> {
    scan_roots().iter().flat_map(|root| rust_files(root)).collect()
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .map(|e| e.into_path())
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect()
}

/// A root that silently stopped resolving — renamed, moved, typoed — would
/// otherwise look identical to a passing run: every test above finds no
/// violations because it scanned nothing.
#[test]
fn the_scan_roots_resolve_to_some_files() {
    let scanned = all_rust_files().len();
    scanned_roots_nonempty(&repo_root(), &["crates/*/src"], scanned);
}

fn rel(path: &Path) -> String {
    path.strip_prefix(repo_root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The largest char-boundary index of `s` that is `<= idx`. `flat` is built
/// one whole `char` at a time (see `flatten`), so it is valid UTF-8
/// throughout, but a fixed byte-count window (`offset + N`) can still land
/// inside a multi-byte character — CJK fixture text a few bytes past a
/// matched pattern is enough to panic a raw `flat[start..end]` slice.
fn char_boundary_floor(s: &str, idx: usize) -> usize {
    let mut i = idx.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn flatten(source: &str) -> (String, Vec<usize>) {
    let mut flat = String::with_capacity(source.len());
    let mut lines = Vec::with_capacity(source.len());
    for (idx, raw) in source.lines().enumerate() {
        let code = match raw.find("//") {
            Some(pos) => &raw[..pos],
            None => raw,
        };
        for ch in code.chars().filter(|c| !c.is_whitespace()) {
            for _ in 0..ch.len_utf8() {
                lines.push(idx + 1);
            }
            flat.push(ch);
        }
    }
    (flat, lines)
}

fn allowed_lines(source: &str) -> Vec<usize> {
    source
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains(ALLOW_MARKER))
        .map(|(i, _)| i + 1)
        .collect()
}

fn ends_with_root_file_name(flat: &str, end: usize) -> Option<&'static str> {
    let head = &flat[..end];
    for ident in ROOT_IDENTS {
        for form in [
            format!("Path::new({ident})"),
            format!("Path::new(&{ident})"),
            format!("&{ident}"),
            (*ident).to_string(),
        ] {
            if !head.ends_with(&form) {
                continue;
            }
            let before = head[..head.len() - form.len()].chars().next_back();
            let bare = form == *ident || form == format!("&{ident}");
            if bare && matches!(before, Some(c) if c == '.' || c.is_alphanumeric() || c == '_') {
                continue;
            }
            return Some(ident);
        }
    }
    None
}

fn walk_up_offsets(flat: &str) -> Vec<usize> {
    if !flat.contains("join(\".moss") {
        return Vec::new();
    }
    flat.match_indices("ancestors()").map(|(o, _)| o).collect()
}

#[test]
fn no_file_name_derivation_of_a_vault_root_outside_the_owner() {
    let files = all_rust_files();
    let mut violations = Vec::new();

    for file in &files {
        let name = rel(file);
        if is_owner(&name) {
            continue;
        }
        let source = fs::read_to_string(file).expect("read source");
        let allowed = allowed_lines(&source);
        let (flat, line_of) = flatten(&source);

        for (offset, _) in flat.match_indices(".file_name(") {
            let Some(ident) = ends_with_root_file_name(&flat, offset) else {
                continue;
            };
            let line = line_of.get(offset).copied().unwrap_or(0);
            if allowed.contains(&line) {
                continue;
            }
            violations.push(format!(
                "{name}:{line}: `{ident}.file_name()` re-derives a root name — \
                 read `VaultRoot::name()` instead (the root is already resolved)"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "vault-root names must come from `vault_root::VaultRoot`, not `Path::file_name`:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_second_basename_algorithm() {
    let files = all_rust_files();
    let mut violations = Vec::new();

    for file in &files {
        let name = rel(file);
        if is_owner(&name) {
            continue;
        }
        let source = fs::read_to_string(file).expect("read source");
        let allowed = allowed_lines(&source);
        let (flat, line_of) = flatten(&source);

        for (opener, finisher) in BANNED_ALGORITHMS {
            for (offset, _) in flat.match_indices(opener) {
                let end = char_boundary_floor(&flat, offset + CHAIN_WINDOW);
                let stmt = flat[offset..end].split(';').next().unwrap_or("");
                if !stmt.contains(finisher) {
                    continue;
                }
                let line = line_of.get(offset).copied().unwrap_or(0);
                if allowed.contains(&line) {
                    continue;
                }
                violations.push(format!(
                    "{name}:{line}: second basename algorithm `{opener}…{finisher}`"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "one basename algorithm, in `vault_root`:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_canonicalize_of_a_root_ident_outside_the_owner() {
    let files = all_rust_files();
    let mut violations = Vec::new();

    for file in &files {
        let name = rel(file);
        if is_owner(&name) {
            continue;
        }
        let source = fs::read_to_string(file).expect("read source");
        let allowed = allowed_lines(&source);
        let (flat, line_of) = flatten(&source);

        for (start, _) in flat.match_indices("canonicalize(") {
            let end = char_boundary_floor(&flat, start + 240);
            let Some(hit) = flat[start..end].find(".file_name(") else {
                continue;
            };
            let window = &flat[start..start + hit];
            if window.contains(';') || window.contains('}') {
                continue;
            }
            let line = line_of.get(start).copied().unwrap_or(0);
            if allowed.contains(&line) {
                continue;
            }
            violations.push(format!(
                "{name}:{line}: `canonicalize(...).file_name()` is the deleted \
                 per-consumer fallback — resolve at the entry point and read `VaultRoot::name()`"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "a root name must never be recovered by canonicalizing at a consumer:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_second_moss_marker_walk_up_outside_the_owner() {
    let files = all_rust_files();
    let mut violations = Vec::new();

    for file in &files {
        let name = rel(file);
        if is_owner(&name) {
            continue;
        }
        let source = fs::read_to_string(file).expect("read source");
        let allowed = allowed_lines(&source);
        let (flat, line_of) = flatten(&source);

        for offset in walk_up_offsets(&flat) {
            let line = line_of.get(offset).copied().unwrap_or(0);
            if allowed.contains(&line) {
                continue;
            }
            violations.push(format!(
                "{name}:{line}: a second `.moss/` walk-up — call \
                 `vault_root::VaultRoot::containing()`, which resolves its input first"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "only `vault_root` may decide which ancestor owns a path:\n{}",
        violations.join("\n")
    );
}

/// The guard is only as good as its ability to SEE a violation.
#[test]
fn the_scanner_actually_detects_each_banned_shape() {
    let (flat, _) = flatten("let n = Path::new(source_path)\n    .file_name()\n    .unwrap();");
    let offset = flat.find(".file_name(").expect("chain flattened");
    assert_eq!(
        ends_with_root_file_name(&flat, offset),
        Some("source_path"),
        "a multi-line chain off a root ident must be detected"
    );

    let (flat, _) = flatten("let n = item.source_path.file_name();");
    let offset = flat.find(".file_name(").expect("present");
    assert_eq!(
        ends_with_root_file_name(&flat, offset),
        None,
        "a per-FILE basename off a struct field must NOT be flagged"
    );

    let (flat, _) = flatten("let n = folder_path.file_name();");
    let offset = flat.find(".file_name(").expect("present");
    assert_eq!(ends_with_root_file_name(&flat, offset), Some("folder_path"));

    let (flat, _) = flatten("match folder_path.split(['/', '\\\\']).next_back() {");
    assert!(
        BANNED_ALGORITHMS.iter().any(|(o, f)| flat.contains(o) && flat.contains(f)),
        "the shipped spacing of the recents split must still be recognised: {flat}"
    );

    let (flat, _) = flatten("article_id.split(['/', '\\\\']).any(|seg| seg == \"..\")");
    assert!(
        !BANNED_ALGORITHMS.iter().any(|(o, f)| flat.contains(o) && flat.contains(f)),
        "path-safety validation is a different job and must not be flagged: {flat}"
    );

    let (flat, _) = flatten(
        "let folder_name = match folder.rfind('/') { Some(idx) => &folder[idx + 1..], None => folder.as_str() };",
    );
    assert!(
        BANNED_ALGORITHMS.iter().any(|(o, f)| flat.contains(o) && flat.contains(f)),
        "the rfind('/') + trailing-slice basename must be recognised: {flat}"
    );

    let (flat, _) = flatten("let patch = version.rfind('.').map(|i| &version[i + 1..]);");
    assert!(
        !BANNED_ALGORITHMS.iter().any(|(o, f)| flat.contains(o) && flat.contains(f)),
        "rfind on a non-path separator must NOT be flagged: {flat}"
    );

    let (flat, _) = flatten("let dirs = relative.rfind('/').map(|i| &relative[..i]);");
    assert!(
        !BANNED_ALGORITHMS.iter().any(|(o, f)| flat.contains(o) && flat.contains(f)),
        "rfind('/') slicing the directory prefix must NOT be flagged: {flat}"
    );

    assert_eq!(allowed_lines("a\n// allow:root-name because X\nb"), vec![2]);

    let (flat, _) = flatten(
        "for ancestor in start_dir.ancestors() {\n    if ancestor.join(\".moss\").is_dir() {\n",
    );
    assert_eq!(
        walk_up_offsets(&flat).len(),
        1,
        "the `.moss/` walk-up must be detected across lines: {flat}"
    );

    let (flat, _) = flatten("let dir = root.join(\".moss\").join(\"build\");");
    assert!(
        walk_up_offsets(&flat).is_empty(),
        "joining `.moss` onto a resolved root must NOT be flagged: {flat}"
    );

    let (flat, _) = flatten("let depth = p.ancestors().count();");
    assert!(
        walk_up_offsets(&flat).is_empty(),
        "walking ancestors without probing for the vault marker must NOT be flagged: {flat}"
    );
}
