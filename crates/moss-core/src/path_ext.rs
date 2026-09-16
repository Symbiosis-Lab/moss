//! Canonical path-extension helpers for moss-core.
//!
//! All other modules must import from here instead of defining their own
//! variant. There are two flavours:
//!
//! - [`path_extension`] — returns `Option<String>` (no lowercasing, dotfile-safe).
//!   Used where `None` has semantic meaning (e.g. a reference that carries no
//!   extension intent vs one that does).
//!
//! - [`path_extension_lower`] — returns `String` (lowercased, empty string if
//!   absent, strips `?query` / `#fragment`). Used where the extension is fed
//!   into a match arm or MIME table and the caller just wants a plain string.

/// Return the lowercased extension of a file path, or `None` when there is
/// genuinely no extension.
///
/// Rules:
/// - Strips any leading directory components (`dir/file.txt` → `txt`).
/// - Dotfiles (`.gitignore`, `.env`) have an **empty stem** before their dot
///   and return `None` — they are not treated as files with extension
///   `"gitignore"`.
/// - The result is ASCII-lowercased (extension matching is always
///   case-insensitive in moss).
/// - Does NOT strip `?query` or `#fragment`; callers with URL-shaped
///   strings should use [`path_extension_lower`] which does strip them.
pub(crate) fn path_extension(path: &str) -> Option<String> {
    let filename = path.rsplit('/').next().unwrap_or(path);
    let (stem, ext) = filename.rsplit_once('.')?;
    // Dotfiles like `.gitignore` have an empty stem — not a real extension.
    if stem.is_empty() {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

/// Return the lowercased extension of a file path or URL as a plain `String`.
///
/// Returns an empty string when there is no recognisable extension, rather
/// than `None`, so callers can match on it without an extra unwrap step.
///
/// Additional behaviour over [`path_extension`]:
/// - Strips `?query` and `#fragment` before inspecting the filename, so
///   URLs like `track.mp3?v=2` resolve to `"mp3"`.
/// - Does **not** apply the dotfile guard: `.gitignore` would yield
///   `"gitignore"`. Callers that need dotfile-safety should use
///   [`path_extension`] instead.
pub(crate) fn path_extension_lower(path: &str) -> String {
    // Strip query string and fragment before filename extraction.
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let filename = path.rsplit('/').next().unwrap_or(path);
    match filename.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() => ext.to_ascii_lowercase(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- path_extension ---

    #[test]
    fn ext_plain_file() {
        assert_eq!(path_extension("file.md"), Some("md".to_owned()));
    }

    #[test]
    fn ext_uppercase_lowercased() {
        assert_eq!(path_extension("photo.JPG"), Some("jpg".to_owned()));
    }

    #[test]
    fn ext_with_dir() {
        assert_eq!(path_extension("dir/sub/file.mp4"), Some("mp4".to_owned()));
    }

    #[test]
    fn ext_dotfile_returns_none() {
        assert_eq!(path_extension(".gitignore"), None);
        assert_eq!(path_extension("dir/.env"), None);
    }

    #[test]
    fn ext_no_dot_returns_none() {
        assert_eq!(path_extension("README"), None);
    }

    #[test]
    fn ext_query_string_not_stripped() {
        // path_extension does NOT strip queries — caller must split first.
        // "file.mp3?v=2" has filename "file.mp3?v=2"; rfind('.') → "mp3?v=2"
        // (not "mp3"). This is the documented limitation.
        assert_ne!(path_extension("file.mp3?v=2"), Some("mp3".to_owned()));
    }

    // --- path_extension_lower ---

    #[test]
    fn lower_plain_file() {
        assert_eq!(path_extension_lower("file.md"), "md");
    }

    #[test]
    fn lower_uppercase_lowercased() {
        assert_eq!(path_extension_lower("photo.JPG"), "jpg");
    }

    #[test]
    fn lower_with_dir() {
        assert_eq!(path_extension_lower("dir/file.mp4"), "mp4");
    }

    #[test]
    fn lower_no_extension() {
        assert_eq!(path_extension_lower("noext"), "");
    }

    #[test]
    fn lower_strips_query() {
        assert_eq!(path_extension_lower("track.mp3?v=2"), "mp3");
    }

    #[test]
    fn lower_strips_fragment() {
        assert_eq!(path_extension_lower("page.html#section"), "html");
    }

    #[test]
    fn lower_strips_query_and_fragment() {
        assert_eq!(path_extension_lower("file.ogg?v=1#t=30"), "ogg");
    }

    #[test]
    fn lower_empty_string() {
        assert_eq!(path_extension_lower(""), "");
    }
}
