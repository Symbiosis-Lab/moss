//! No `tokio::spawn` in this crate's Tauri-context files.
//!
//! Open-half twin of `test_no_tokio_spawn_in_tauri_context_files` in the
//! desktop app's test suite — that test checks `build.rs` (desktop) plus
//! `manager.rs` / `watch.rs` (this crate) against the same regex via
//! `include_str!`, and the `include_str!` reads are the only thing forcing
//! the desktop crate to see this crate's source for that one test. This
//! twin asserts this crate's own two files never call `tokio::spawn`; the
//! desktop half keeps checking `build.rs`.
//!
//! ## Why this exists
//!
//! `tokio::spawn` requires an active tokio runtime context. Tauri event
//! listener callbacks run on Tauri's event loop, NOT in tokio — calling
//! `tokio::spawn` there panics or, worse, causes a runtime mismatch leading
//! to heap corruption (SIGABRT, "corrupted double-linked list" in CLI mode).
//! `tauri::async_runtime::spawn` works from any context and is the fix.

#[test]
fn test_no_tokio_spawn_in_tauri_context_files() {
    // Both files are still Tauri-context at runtime (both run inside the
    // app's one async runtime), so still checked here even though this
    // crate itself has no tauri dependency.
    let files_to_check = [
        (
            "src/plugins/manager.rs",
            include_str!("../src/plugins/manager.rs"),
        ),
        (
            "src/build/watch.rs",
            include_str!("../src/build/watch.rs"),
        ),
    ];

    let mut failures = Vec::new();

    for (filename, content) in files_to_check {
        let mut in_test_module = false;
        for (line_num, line) in content.lines().enumerate() {
            let trimmed = line.trim();

            if trimmed == "#[cfg(test)]" {
                in_test_module = true;
                continue;
            }
            if in_test_module {
                continue;
            }

            if trimmed.starts_with("//") {
                continue;
            }

            let code_part = if let Some(comment_start) = line.find("//") {
                &line[..comment_start]
            } else {
                line
            };

            if code_part.contains("tokio::spawn") {
                failures.push(format!("{}:{}: {}", filename, line_num + 1, trimmed));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "\n\ntokio::spawn found in Tauri context files!\n\n\
         These should use tauri::async_runtime::spawn instead to avoid:\n\
         - Runtime mismatch causing heap corruption\n\
         - 'corrupted double-linked list' SIGABRT errors in CLI mode\n\n\
         Found {} occurrence(s):\n{}\n\n\
         FIX: Replace `tokio::spawn` with `tauri::async_runtime::spawn`\n",
        failures.len(),
        failures.join("\n")
    );
}
