//! `[editor] attachment_folder` — where a page's images live.
//!
//! Three placements, path-intuitive encoding (decision:
//! `docs/archive/2026-08-14-site-settings-and-attachments.md`):
//!
//! | config value | meaning | example for `posts/on-gardens.md` |
//! |---|---|---|
//! | `""` (default) | next to the page | `posts/photo.png` |
//! | `./assets` (leading `./`) | subfolder of the page's own folder | `posts/assets/photo.png` |
//! | `assets` (bare relative) | root-relative folder | `assets/photo.png` |
//!
//! Absolute paths and `..` are rejected at load with a config diagnostic
//! (a logged warning) and fall back to the default. Folders are created
//! lazily — only when an image is actually stored.
//!
//! Safety property that keeps this a no-drama setting: wikilink resolution is
//! stem-indexed and location-independent, so `![[photo.png]]` resolves
//! wherever the file lands — changing the setting never breaks existing links.
//!
//! The rules live here, in the pure crate, because two callers on opposite
//! sides of a crate boundary need them: the editor (`src-tauri`) resolves where
//! to *write* an image, and the build (`moss-build`) asks which scanned
//! directories are storage rather than sections of the site.

/// Reject values that would escape the project: absolute paths and any `..`
/// component. Returns the reason so both the save command and the load-time
/// diagnostic can state it.
pub fn validate_attachment_folder(raw: &str) -> Result<(), String> {
    if is_rooted(raw) {
        return Err("attachment_folder must be a relative path".to_string());
    }
    let has_dotdot = raw.split(['/', '\\']).any(|c| c == "..");
    if has_dotdot {
        return Err("attachment_folder must not contain '..'".to_string());
    }
    Ok(())
}

/// Whether `raw` is rooted — an absolute path that would escape the project.
///
/// Deliberately NOT `Path::is_absolute()`, which answers a different question
/// on each platform: on Windows it is false for rooted-but-driveless paths
/// (`/etc/passwd`, `\x`) that still resolve from the drive root, and on Unix
/// it is false for `C:\x` — but config files and plugin paths travel between
/// machines, so both must be refused everywhere. These three string tests
/// give the same answer on every platform by construction, which is the whole
/// point; they also subsume the `Component::RootDir | Prefix(_)` walk, since
/// every UNC, verbatim and device prefix begins `\\`.
///
/// One owner for both trust boundaries that ask this: the attachment setting
/// above, and `vault::fs`'s plugin-path rules, which delegate here. It lives
/// in this module rather than in the host because the pure crate cannot call
/// upward — the same boundary that put the placement rules here.
pub fn is_rooted(raw: &str) -> bool {
    let b = raw.as_bytes();
    // A drive letter, not merely a colon: `1:x` is a filename, `a:x` is not
    // (any single letter names a drive, and Windows forbids `:` in filenames).
    let drive_letter = b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':';
    drive_letter || raw.starts_with('/') || raw.starts_with('\\')
}

/// Pure placement rule: project-relative directory (forward slashes, no
/// trailing slash, `""` = project root) where an image dropped/pasted into
/// `page_rel` (project-relative path of the open page) should land.
pub fn attachment_dir_for_page(raw: &str, page_rel: &str) -> String {
    let page_dir = page_rel.rsplit_once('/').map_or("", |(dir, _)| dir);
    let raw = raw.trim().trim_end_matches('/');
    if raw.is_empty() || raw == "." {
        return page_dir.to_string();
    }
    if let Some(rest) = raw.strip_prefix("./") {
        // Sibling subfolder of the page's folder — the config value reads
        // exactly like the path it produces from the page's point of view.
        let rest = rest.trim_matches('/');
        if rest.is_empty() {
            return page_dir.to_string();
        }
        return if page_dir.is_empty() {
            rest.to_string()
        } else {
            format!("{page_dir}/{rest}")
        };
    }
    // Bare relative path: root-relative folder.
    raw.to_string()
}

/// Whether the project-relative directory `dir` is attachment storage under
/// the setting `raw` — it, or an ancestor of it, is where images land.
///
/// This is the inverse question to [`attachment_dir_for_page`]: not "where
/// does this page's image go?" but "is this folder holding somebody's
/// images?". The build asks it to decide that a directory is storage rather
/// than a section of the site, so it gets no index page and no listing row.
///
/// The default (`""`) puts attachments beside the page, so there is no asset
/// folder at all and nothing is excluded — the no-op case for most vaults.
pub fn is_attachment_dir(raw: &str, dir: &str) -> bool {
    // The project root is never storage: excluding it would take the whole
    // site with it.
    if dir.trim_matches('/').is_empty() {
        return false;
    }
    let raw = raw.trim().trim_end_matches('/');
    // `./X` names a folder relative to EVERY page's own folder, so it matches
    // wherever it appears; a bare `X` is one folder at the site root.
    let (pattern, anywhere) = match raw.strip_prefix("./") {
        Some(rest) => (rest, true),
        None if raw == "." => return false,
        None => (raw, false),
    };
    let want: Vec<&str> = pattern.trim_matches('/').split('/').collect();
    if want == [""] {
        // `""` / `"./"` — attachments sit beside the page. No folder is storage.
        return false;
    }
    // Compare whole path segments, so `assets` never matches `assets-and-crafts`
    // and a multi-segment `a/b` only matches those segments adjacent. Matching
    // anywhere in the path is also what makes a match cover everything BENEATH
    // the folder, not just the folder itself.
    let segments: Vec<&str> = dir.trim_matches('/').split('/').collect();
    if anywhere {
        segments.windows(want.len()).any(|w| w == want)
    } else {
        segments.starts_with(&want)
    }
}

#[cfg(test)]
#[path = "attachment_tests.rs"]
mod tests;
