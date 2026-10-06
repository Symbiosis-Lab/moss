//! File writer for saving scraped content
//!
//! Maps URLs to file paths *relative to the import target folder*, treating
//! the folder as the scope's root on disk (not a host mirror).
//!
//! On filename collisions, uses macOS-Finder-style suffix renaming
//! (`name.md` → `name 2.md` → `name 3.md`).

use std::path::Path;
use url::Url;

use super::scope::UrlScope;

/// THE filename policy for everything the importer writes — one authority, so
/// a caller cannot pick the weaker of two.
///
/// Separators, the Windows-reserved punctuation and every control character
/// become spaces; runs of whitespace collapse; leading and trailing dots go,
/// which is what turns a `..` segment into nothing. 120 chars leaves headroom
/// under the common 255-byte limit even for multibyte (CJK) titles, plus the
/// `.md` and ` 2` suffixes.
///
/// `is_control` rather than the three whitespace escapes it subsumes: a URL may
/// carry `%00`, which reaches `Path::join` intact and makes the write fail —
/// and in a crawl that `?` ends the run over one bad link.
pub fn sanitize_filename(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
            c if c.is_control() => ' ',
            _ => c,
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
        .trim_matches('.')
        .chars()
        .take(120)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Apply [`sanitize_filename`] to every segment of a relative path, dropping
/// the segments it empties.
///
/// This is what keeps a URL from escaping the import folder. `Url::parse` does
/// not collapse percent-encoding, so `https://host/%2e%2e%2f%2e%2e%2fetc/evil`
/// arrives here as the single opaque segment it was, and only decodes into
/// `../../etc/evil` at the line above the call. A `..` segment sanitizes to the
/// empty string and is dropped, so the result is always inside the folder.
fn sanitize_relative(relative: &str) -> String {
    relative
        .split('/')
        .map(sanitize_filename)
        .filter(|seg| !seg.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// Convert a URL to a file path *relative to the scope root*.
///
/// The target folder behaves as the import's site root, so paths are computed
/// after stripping the scope's path prefix.
///
/// Examples (scope = `https://example.com/blog/`):
/// - `https://example.com/blog/` → `index.md`
/// - `https://example.com/blog/post-1` → `post-1.md`
/// - `https://example.com/blog/2024/intro` → `2024/intro.md`
/// - `https://example.com/blog/2024/` → `2024/index.md`
///
/// Scope-root special case (scope = `https://example.com/2024/12/22/the-reporters-notebook/`):
/// - The scope-root URL itself → `the-reporters-notebook.md`
///   (last non-empty segment of the scope path becomes the filename, so a
///   leaf-article import lands as a single named file in the target folder,
///   not `index.md`)
pub fn url_to_file_path(url_str: &str, scope: &UrlScope) -> String {
    let url = match Url::parse(url_str) {
        Ok(u) => u,
        Err(_) => return "index.md".to_string(),
    };

    let raw_path = url.path();
    let decoded_path = urlencoding::decode(raw_path).unwrap_or_else(|_| raw_path.into());
    let decoded = decoded_path.as_ref();

    // Strip scope prefix → path relative to the target folder.
    let scope_prefix = scope.path_prefix();
    let scope_trimmed = scope_prefix.trim_end_matches('/');

    let relative: &str = if scope_trimmed.is_empty() {
        decoded.trim_start_matches('/')
    } else if let Some(rest) = decoded.strip_prefix(scope_trimmed) {
        rest.trim_start_matches('/')
    } else {
        // Out-of-scope URL — caller should normally not reach this branch.
        // Fall back to the full path so we don't silently drop the page.
        decoded.trim_start_matches('/')
    };

    // Every segment goes through the one filename policy before it reaches a
    // `join`, so a decoded `..` cannot leave the import folder.
    let ends_in_slash = relative.ends_with('/');
    let safe = sanitize_relative(relative);

    if safe.is_empty() {
        // Scope root, or a path the policy emptied → one file at the folder's
        // top level, named from the scope's slug (or "index" at the host root).
        let slug = scope_trimmed
            .rsplit('/')
            .map(sanitize_filename)
            .find(|s| !s.is_empty())
            .unwrap_or_else(|| "index".to_string());
        return format!("{}.md", slug);
    }

    if ends_in_slash {
        return format!("{}/index.md", safe);
    }

    format!("{}.md", safe)
}

/// If the proposed file already exists, return a renamed version using the
/// macOS-Finder convention: `name.md` → `name 2.md` → `name 3.md`.
///
/// Only the basename changes; the directory portion is preserved.
pub fn rename_for_collision(folder: &Path, relative: &str) -> String {
    if !folder.join(relative).exists() {
        return relative.to_string();
    }

    let (dir_part, file_part) = match relative.rsplit_once('/') {
        Some((d, f)) => (d, f),
        None => ("", relative),
    };

    let (stem, ext) = match file_part.rsplit_once('.') {
        Some((s, e)) => (s, e),
        None => (file_part, ""),
    };

    let mut n: u32 = 2;
    loop {
        let new_base = if ext.is_empty() {
            format!("{} {}", stem, n)
        } else {
            format!("{} {}.{}", stem, n, ext)
        };
        let new_rel = if dir_part.is_empty() {
            new_base.clone()
        } else {
            format!("{}/{}", dir_part, new_base)
        };
        if !folder.join(&new_rel).exists() {
            return new_rel;
        }
        n += 1;
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn scope(url: &str) -> UrlScope {
        UrlScope::new(url).unwrap()
    }

    #[test]
    fn sanitize_filename_strips_separators_bounds_length_and_dots() {
        // Path separators removed → no traversal possible.
        assert!(!sanitize_filename("../../etc/passwd").contains('/'));
        assert_eq!(sanitize_filename("a/b\\c:d"), "a b c d");
        // All-dots → empty (caller falls back to the file stem).
        assert_eq!(sanitize_filename("..."), "");
        // Length is capped on a char boundary.
        assert!(sanitize_filename(&"字".repeat(500)).chars().count() <= 120);
    }

    /// A percent-encoded `..` survives `Url::parse` as one opaque segment and
    /// only becomes a separator when the path is decoded. Every segment goes
    /// through the filename policy after that, so the mapping stays inside the
    /// import folder.
    #[test]
    fn a_percent_encoded_traversal_cannot_escape_the_folder() {
        let s = scope("https://example.com/");
        let mapped = url_to_file_path("https://example.com/%2e%2e%2f%2e%2e%2fetc/evil", &s);
        assert!(
            !mapped.starts_with('/') && !mapped.split('/').any(|seg| seg == ".."),
            "mapped to {mapped}"
        );
        assert_eq!(mapped, "etc/evil.md");
    }

    #[test]
    fn root_scope_maps_to_index() {
        let s = scope("https://example.com/");
        let r = url_to_file_path("https://example.com/", &s);
        assert_eq!(r, "index.md");
    }

    #[test]
    fn root_scope_child_keeps_relative_path() {
        let s = scope("https://example.com/");
        let r = url_to_file_path("https://example.com/blog/post-1", &s);
        assert_eq!(r, "blog/post-1.md");
    }

    #[test]
    fn root_scope_trailing_slash_child_becomes_index() {
        let s = scope("https://example.com/");
        let r = url_to_file_path("https://example.com/blog/", &s);
        assert_eq!(r, "blog/index.md");
    }

    #[test]
    fn scoped_root_uses_last_path_segment_as_slug() {
        let s = scope("https://www.example.com/2024/12/22/the-reporters-notebook-abroad/");
        let r = url_to_file_path(
            "https://www.example.com/2024/12/22/the-reporters-notebook-abroad/",
            &s,
        );
        assert_eq!(r, "the-reporters-notebook-abroad.md");
    }

    #[test]
    fn scoped_root_no_trailing_slash_also_uses_slug() {
        let s = scope("https://example.com/about");
        let r = url_to_file_path("https://example.com/about", &s);
        assert_eq!(r, "about.md");
    }

    #[test]
    fn scoped_child_strips_prefix() {
        let s = scope("https://example.com/blog/");
        let r = url_to_file_path("https://example.com/blog/2024/intro", &s);
        assert_eq!(r, "2024/intro.md");
    }

    #[test]
    fn scoped_child_with_trailing_slash() {
        let s = scope("https://example.com/blog/");
        let r = url_to_file_path("https://example.com/blog/2024/", &s);
        assert_eq!(r, "2024/index.md");
    }

    #[test]
    fn url_decoding_applied_to_path() {
        let s = scope("https://example.com/");
        let r = url_to_file_path("https://example.com/blog/hello%20world", &s);
        assert_eq!(r, "blog/hello world.md");
    }

    #[test]
    fn collision_appends_finder_suffix() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();

        assert_eq!(rename_for_collision(base, "post.md"), "post.md");

        std::fs::write(base.join("post.md"), "x").unwrap();
        assert_eq!(rename_for_collision(base, "post.md"), "post 2.md");

        std::fs::write(base.join("post 2.md"), "x").unwrap();
        assert_eq!(rename_for_collision(base, "post.md"), "post 3.md");
    }

    #[test]
    fn collision_preserves_directory_part() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();
        std::fs::create_dir_all(base.join("sub")).unwrap();
        std::fs::write(base.join("sub/post.md"), "x").unwrap();

        assert_eq!(rename_for_collision(base, "sub/post.md"), "sub/post 2.md");
    }
}
