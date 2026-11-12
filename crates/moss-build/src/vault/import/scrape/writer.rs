//! File writer for saving scraped content
//!
//! Maps URLs to file paths *relative to the import target folder*, treating
//! the folder as the scope's root on disk (not a host mirror).
//!
//! On filename collisions, uses macOS-Finder-style suffix renaming
//! (`name.md` → `name 2.md` → `name 3.md`).

use std::path::Path;
use std::io::Write;
use moss_core::content_graph::normalize_path;
use std::collections::{HashMap, HashSet};

use super::design::norm_url;
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
    let scope_prefix = urlencoding::decode(scope.path_prefix()).unwrap_or_else(|_| scope.path_prefix().into());
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
    available_path(relative, |candidate| folder.join(candidate).exists())
}

fn available_path(relative: &str, occupied: impl Fn(&str) -> bool) -> String {
    if !occupied(relative) {
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
        if !occupied(&new_rel) {
            return new_rel;
        }
        n += 1;
    }
}


/// Converted content keeps source addresses until every destination is reserved.
pub(crate) struct PendingPage {
    pub source: String,
    content: tempfile::TempPath,
    pub imported: bool,
}

impl PendingPage {
    pub fn new(source: &str, content: &str, imported: bool) -> Result<Self, String> {
        let mut spool = tempfile::NamedTempFile::new().map_err(|e| format!("Cannot spool imported page: {e}"))?;
        spool.write_all(content.as_bytes()).map_err(|e| format!("Cannot spool imported page: {e}"))?;
        Ok(Self { source: source.to_string(), content: spool.into_temp_path(), imported })
    }
}

/// The crawl's final addresses, including collisions with existing author files.
pub(crate) struct OutputPaths {
    pub pages: HashMap<String, String>,
}

impl OutputPaths {
    pub fn reserve(folder: &Path, scope: &UrlScope, pages: &[PendingPage]) -> Result<Self, String> {
        let proposed: Vec<String> = pages.iter().map(|p| url_to_file_path(&p.source, scope)).collect();
        let desired: Vec<String> = pages.iter().zip(&proposed).map(|(page, proposed_path)| {
            let leaf = proposed_path.strip_suffix("/index.md").filter(|folder| {
                page.imported && !proposed.iter().any(|other| {
                    other != proposed_path && other.starts_with(&format!("{folder}/"))
                })
            });
            leaf.map_or_else(|| proposed_path.clone(), |folder| format!("{folder}.md"))
        }).collect();
        // Only the final paths' parents can contain filename collisions. An
        // unrelated subtree may be unreadable or expensive to enumerate.
        let parents: HashSet<&Path> = desired.iter().filter_map(|path| Path::new(path).parent()).collect();
        let mut occupied = HashSet::new();
        for parent in parents {
            let entries = match std::fs::read_dir(folder.join(parent)) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(format!("Cannot inspect import destination {}: {e}", folder.join(parent).display())),
            };
            for entry in entries {
                let entry = entry.map_err(|e| format!("Cannot inspect import destination {}: {e}", folder.join(parent).display()))?;
                occupied.insert(normalize_path(&parent.join(entry.file_name()).to_string_lossy()));
            }
        }
        let mut reserved = HashMap::new();
        for (page, desired) in pages.iter().zip(&desired) {
            let relative = available_path(desired, |candidate| {
                folder.join(candidate).exists() || occupied.contains(&normalize_path(candidate))
            });
            occupied.insert(normalize_path(&relative));
            if let Some(key) = source_key(&page.source) {
                reserved.insert(key, relative);
            }
        }
        // A slashless link may name a page fetched with a trailing slash.
        // Exact addresses win when a source serves both as distinct pages.
        for page in pages {
            if let (Some(exact), Some(alias)) = (source_key(&page.source), norm_url(&page.source)) {
                if let Some(path) = reserved.get(&exact).cloned() {
                    reserved.entry(alias).or_insert(path);
                }
            }
        }
        Ok(Self { pages: reserved })
    }

    pub fn get(&self, source: &str) -> Option<&String> {
        self.pages.get(&source_key(source)?)
    }

    pub fn write(
        &self,
        folder: &Path,
        pages: &[PendingPage],
        redirects: &HashMap<String, String>,
        recursive: bool,
    ) -> Result<Vec<String>, String> {
        let mut written = Vec::new();
        let mut members = false;
        let no_pages = HashMap::new();
        for page in pages {
            let relative = self.get(&page.source)
                .ok_or_else(|| format!("No output path reserved for {}", page.source))?;
            // allow:raw_read a temporary spool written by this import, never a cloud-backed input
            let source = std::fs::read_to_string(&page.content).map_err(|e| format!("Cannot read imported page spool: {e}"))?;
            let content = if page.imported {
                render_note(&source, &page.source, relative,
                    if recursive { &self.pages } else { &no_pages }, redirects)
            } else {
                source
            };
            crate::vault::import::page::write_note(folder, relative, &content)?;
            members |= restricts(&content);
            if page.imported {
                written.push(relative.clone());
            }
        }
        if members { crate::vault::config::require_members_moss(folder)?; }
        Ok(written)
    }
}

/// Whether an older moss would publish more of this page than this one does.
fn restricts(content: &str) -> bool {
    use moss_core::frontmatter::{member_cascade::declared_cascade, members::public_source};
    matches!(public_source(content), std::borrow::Cow::Owned(_)) || declared_cascade(content).is_some()
}

fn render_note(
    content: &str,
    source: &str,
    relative: &str,
    pages: &HashMap<String, String>,
    redirects: &HashMap<String, String>,
) -> String {
    let split = moss_core::frontmatter::frontmatter_span(content).map_or(0, |s| s.body);
    let (head, body) = content.split_at(split);
    let media_prefix = format!("./{}/", super::run::ASSETS_SUBDIR);
    let local_prefix = relative_path(relative, &format!("{}/", super::run::ASSETS_SUBDIR));
    let mut out = String::new();
    for line in head.split_inclusive('\n') {
        if line.starts_with("cover:") {
            out.push_str(&line.replace(&media_prefix, &local_prefix));
        } else {
            out.push_str(line);
        }
    }
    let body = map_link_targets(body, |target| {
        target.strip_prefix(&media_prefix).map(|file| format!("{local_prefix}{file}"))
    });
    out.push_str(&rewrite_links(&body, source, relative, pages, redirects));
    out
}

fn source_key(source: &str) -> Option<String> {
    let mut url = Url::parse(source).ok()?;
    url.set_fragment(None);
    Some(url.to_string())
}

/// Resolve only addresses that actually landed, preserving unknown destinations.
pub(super) fn rewrite_links(
    markdown: &str,
    source: &str,
    from: &str,
    pages: &HashMap<String, String>,
    redirects: &HashMap<String, String>,
) -> String {
    let Ok(base) = Url::parse(source) else { return markdown.to_string() };
    map_link_targets(markdown, |target| {
        if target.starts_with('#') || super::crawler::is_non_page_file_url(target) {
            return None;
        }
        let address = base.join(target).ok()?;
        let key = source_key(address.as_str())?;
        let alias = norm_url(address.as_str())?;
        let fragment = address.fragment().map(|f| format!("#{f}")).unwrap_or_default();
        if let Some(path) = pages.get(&key).or_else(|| pages.get(&alias)) {
            let path = relative_path(from, path).replace('%', "%25").replace(' ', "%20")
                .replace('#', "%23").replace('(', "%28").replace(')', "%29");
            return Some(format!("{path}{fragment}"));
        }
        redirects.get(&key).or_else(|| redirects.get(&alias)).map(|destination| {
            let mut destination = destination.replace(' ', "%20");
            if !fragment.is_empty() && !destination.contains('#') {
                destination.push_str(&fragment);
            }
            destination
        })
    })
}

/// Rewrite the target of every markdown link or image (`](target)`) and wiki
/// embed (`![[target]]`) in `text` that `rewrite` answers for, leaving fenced
/// and inline code, and everything else, as written.
pub(super) fn map_link_targets(text: &str, mut rewrite: impl FnMut(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fence: Option<char> = None;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let marker = trimmed.chars().next().filter(|c| (*c == '`' || *c == '~') && trimmed.starts_with(&c.to_string().repeat(3)));
        match (fence, marker) {
            (None, Some(c)) => fence = Some(c),
            (Some(open), Some(c)) if open == c => fence = None,
            _ => {}
        }
        if fence.is_some() || marker.is_some() {
            out.push_str(line);
            continue;
        }
        let b = line.as_bytes();
        let (mut i, mut last) = (0, 0);
        while i < b.len() {
            if b[i] == b'`' {
                let n = b[i..].iter().take_while(|&&c| c == b'`').count();
                let ticks = "`".repeat(n);
                i += n;
                if let Some(end) = line[i..].find(&ticks) {
                    i += end + n;
                }
                continue;
            }
            let (open, close) = if b[i..].starts_with(b"](") {
                (2, ")")
            } else if b[i..].starts_with(b"![[") {
                (3, "]]")
            } else {
                i += 1;
                continue;
            };
            let Some(len) = line[i + open..].find(close) else {
                i += 1;
                continue;
            };
            let inner = &line[i + open..i + open + len];
            let target = inner.split(|c: char| c.is_whitespace() || c == '|').next().unwrap_or("");
            if let Some(new) = rewrite(target) {
                out.push_str(&line[last..i + open]);
                out.push_str(&new);
                out.push_str(&inner[target.len()..]);
                last = i + open + len;
            }
            i += open + len;
        }
        out.push_str(&line[last..]);
    }
    out
}

/// `target` as a path from the folder holding `from`: `./other.md` beside it,
/// `../about.md` from one folder down.
pub(super) fn relative_path(from: &str, target: &str) -> String {
    let from_dirs: Vec<&str> = from.split('/').collect();
    let from_dirs = &from_dirs[..from_dirs.len() - 1];
    let to: Vec<&str> = target.split('/').collect();
    let common = from_dirs.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let ups = from_dirs.len() - common;
    let rest = to[common..].join("/");
    if ups == 0 {
        format!("./{rest}")
    } else {
        format!("{}{rest}", "../".repeat(ups))
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn scope(url: &str) -> UrlScope {
        UrlScope::new(url).unwrap()
    }

    #[test]
    fn reservations_resolve_only_written_pages_and_preserve_link_titles() {
        let folder = tempfile::tempdir().unwrap();
        let scope = scope("https://example.test/blog/");
        let pages = vec![
            PendingPage::new("https://example.test/blog/", "Home", true).unwrap(),
            PendingPage::new("https://example.test/blog/post/", "Post", true).unwrap(),
        ];
        let paths = OutputPaths::reserve(folder.path(), &scope, &pages).unwrap();
        let md = "[home](/blog/#top) [post](https://example.test/blog/post/#part \"Title\") [missing](https://example.test/blog/missing/) [external](https://other.test/post/) ![](./assets/imported/a.png) [pdf](/blog/paper.pdf)";
        assert_eq!(rewrite_links(md, &pages[0].source, "blog.md", &paths.pages, &HashMap::new()),
            "[home](./blog.md#top) [post](./post.md#part \"Title\") [missing](https://example.test/blog/missing/) [external](https://other.test/post/) ![](./assets/imported/a.png) [pdf](/blog/paper.pdf)");
    }

    #[test]
    fn reservations_read_only_final_parents_and_keep_normalized_collisions() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::create_dir(folder.path().join("posts")).unwrap();
        std::fs::write(folder.path().join("posts/CAFÉ.md"), "Existing page").unwrap();
        let pages = vec![
            PendingPage::new("https://example.test/posts/caf%C3%A9/", "First", true).unwrap(),
            PendingPage::new("https://example.test/posts/cafe%CC%81/", "Second", true).unwrap(),
            PendingPage::new("https://example.test/new/deep/post/", "Third", true).unwrap(),
        ];
        let paths = OutputPaths::reserve(folder.path(), &scope("https://example.test/"), &pages).unwrap();
        assert_eq!(normalize_path(paths.get(&pages[0].source).unwrap()), "posts/café 2.md");
        assert_eq!(normalize_path(paths.get(&pages[1].source).unwrap()), "posts/café 3.md");
        assert_eq!(paths.get(&pages[2].source).unwrap(), "new/deep/post.md");
        paths.write(folder.path(), &pages, &HashMap::new(), true).unwrap();
        assert_eq!(std::fs::read_to_string(folder.path().join("posts/CAFÉ.md")).unwrap(), "Existing page");
        assert_eq!(std::fs::read_to_string(folder.path().join("new/deep/post.md")).unwrap(), "Third");
    }

    #[test]
    fn reservations_report_errors_from_relevant_parents() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("posts"), "Not a directory").unwrap();
        let pages = vec![PendingPage::new("https://example.test/posts/article/", "Body", true).unwrap()];
        let error = OutputPaths::reserve(folder.path(), &scope("https://example.test/"), &pages).err().unwrap();
        assert!(error.contains("Cannot inspect import destination"), "{error}");
        assert!(error.contains("posts"), "{error}");
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
