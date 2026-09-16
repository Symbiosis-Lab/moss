//! Regression guard: no new string-literal `app.emit("channel-name", ...)` calls
//! may appear outside the allowlist below.
//!
//! Open-half twin of `src-tauri/tests/no_string_emits.rs` (desktop repo) —
//! that file scans its own `src_dir()` (desktop) and this crate's own `src`
//! independently (class B per
//! docs/archive/2026-09-16-boundary-gates-remeasured-for-dependency-model.md).
//! This crate has no Tauri dependency (see crates/moss-build/Cargo.toml's
//! "No tauri or wry, direct or transitive" rule), so a real `app.emit(...)`
//! call site cannot exist here — this twin's job is a JS-embedded string
//! constant matching the same literal shape (`plugin-message`, injected via
//! `__TAURI__.event.emit(...)` inside a bundled-plugin JS shim), which the
//! allowlist below already covers.
//!
//! Background: moss migrated ad-hoc string-literal events to a typed `MossEvent`
//! bus in moss#523. Channels that legitimately remain on legacy strings are
//! listed in docs/reference/typed-event-bus.md §"Channels NOT on this bus".

use regex::Regex;
use std::path::PathBuf;

/// Channels that are intentionally NOT on the typed MossEvent bus.
const ALLOWLIST: &[&str] = &[
    "moss-event",
    "initial-build-complete",
    "refresh-preview",
    "action-panel-title-changed",
    "show-toast-dismiss",
    "domain-dns-progress",
    "navigate-to-target",
    "build-config-changed",
    "config-ready",
    "download-progress",
    "icloud-sync",
    "deploy-progress",
    "scan-progress",
    "scan-complete",
    "email_op_rejected",
    "email_op_partial",
    "binary-stderr",
    // JS-side producer: plugin hooks emit progress via
    // `__TAURI__.event.emit("plugin-message", ...)` from the runtime
    // webview / QuickJS shim; the only occurrences in this crate's Rust
    // source are JS plugin-bundle fixture strings inside engine tests.
    "plugin-message",
    "browser-url-changed",
    "matters-room-skipped",
    "matters-room-published",
];

/// Returns this crate's own `src/` directory regardless of where `cargo test`
/// is invoked.
fn src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn find_line_comment_start(line: &str) -> Option<usize> {
    let mut chars = line.char_indices().peekable();
    let mut in_string = false;
    let mut prev_was_backslash = false;
    while let Some((i, c)) = chars.next() {
        if in_string {
            if c == '\\' && !prev_was_backslash {
                prev_was_backslash = true;
                continue;
            }
            if c == '"' && !prev_was_backslash {
                in_string = false;
            }
            prev_was_backslash = false;
        } else {
            if c == '"' {
                in_string = true;
                prev_was_backslash = false;
            } else if c == '/' && line[i..].starts_with("//") {
                return Some(i);
            }
        }
    }
    None
}

#[test]
fn no_new_string_literal_emits() {
    let pattern = Regex::new(r#"\.emit\(\s*"([a-z][a-z0-9_-]*)""#).unwrap();
    let mut violations: Vec<String> = Vec::new();

    for entry in walkdir::WalkDir::new(src_dir())
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("rs"))
    {
        let path = entry.path();
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let stripped: String = content
            .lines()
            .map(|line| {
                if let Some(idx) = find_line_comment_start(line) {
                    &line[..idx]
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        for cap in pattern.captures_iter(&stripped) {
            let channel = &cap[1];
            if !ALLOWLIST.contains(&channel) {
                let line_no = stripped[..cap.get(0).unwrap().start()]
                    .chars()
                    .filter(|&c| c == '\n')
                    .count()
                    + 1;
                violations.push(format!(
                    "{}:{}: .emit(\"{}\", ...) — not in allowlist",
                    path.display(),
                    line_no,
                    channel
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "New string-literal app.emit() call(s) detected outside the allowlist.\n\
         Use emit_moss_event(handle, MossEvent::Variant(...)) instead, or add\n\
         the channel to ALLOWLIST with a justification comment.\n\n\
         Violations:\n{}",
        violations.join("\n")
    );
}
