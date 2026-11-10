//! The moss folder icon: a site's root folder shows moss's leaf in the file
//! manager, so a moss folder is recognisable among the others.
//!
//! Applied on every full build, which is when moss first meets a folder as a
//! site (from the CLI or the app) and how a lost icon comes back. Nothing here
//! may fail or slow a build: each step is best-effort and logs at debug.
//!
//! Only macOS is drawn today. A folder that already carries an icon moss did
//! not write is left alone. In a synced folder the marker is excluded from the
//! File Provider, so the icon stays on this Mac and each Mac draws its own.

use std::path::Path;

#[cfg(target_os = "macos")]
mod compose;
#[cfg(target_os = "macos")]
mod finder;

/// Give `root` moss's folder icon when it lacks one.
pub fn mark_moss_folder(root: &Path) {
    #[cfg(target_os = "macos")]
    {
        let Some(tag) = compose::current_tag() else { return };
        mark_with(root, &tag, || compose::folder_icns(&tag));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = root;
}

/// The policy, with the icon passed in so tests need no AppKit.
#[cfg(target_os = "macos")]
fn mark_with(root: &Path, tag: &str, icns: impl FnOnce() -> Option<Vec<u8>>) {
    use finder::Existing;
    if matches!(finder::existing(root, tag), Existing::Current | Existing::Foreign) {
        return;
    }
    let Some(icns) = icns() else {
        log::debug!(target: "build", "folder icon: could not compose one on this system");
        return;
    };
    match finder::write(root, &icns, tag) {
        Ok(()) => finder::exclude_from_git(root),
        Err(e) => log::debug!(target: "build", "folder icon not written: {e}"),
    }
}

#[cfg(all(test, target_os = "macos"))]
#[path = "folder_icon_tests.rs"]
mod tests;
