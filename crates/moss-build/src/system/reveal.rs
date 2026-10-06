//! Show a path in the OS file manager.
//!
//! One function, and it is here rather than in the app crate because two
//! carriers ask for it: the desktop `reveal_entry` command, and the HTTP
//! carrier's `reveal_history_store` arm, which runs in a process with no Tauri
//! shell. Both are the user pressing "Show in Finder" on a path moss itself
//! resolved — see `tests/no_vendor_ui_launch_test.rs` for why launching another
//! application's UI is otherwise refused.
//!
//! Containment is the caller's: this spawns the file manager on whatever it is
//! given. `reveal_entry` validates a vault-relative path first; the history
//! store's directory is a fixed subpath moss derives from the vault root and
//! has nothing to validate.

use std::path::Path;

/// Open the OS file manager with `target` selected (macOS, Windows) or its
/// containing folder open (other Unix, where there is no portable "select").
pub fn reveal_path(target: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        // allow:vendor_ui the user picked "Reveal"/"Show in Finder" on this path
        std::process::Command::new("open")
            .arg("-R")
            .arg(target)
            .spawn()
            .map_err(|e| format!("Failed to reveal in Finder: {}", e))?;
    }
    #[cfg(target_os = "windows")]
    {
        // Two-arg form: explorer.exe quotes the path itself (spaces/commas safe).
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(target)
            .spawn()
            .map_err(|e| format!("Failed to show in Explorer: {}", e))?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let parent = target.parent().unwrap_or(target);
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|e| format!("Failed to open file manager: {}", e))?;
    }
    Ok(())
}
