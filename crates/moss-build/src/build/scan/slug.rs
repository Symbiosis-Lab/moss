//! Slug generation, UID generation, and duplicate URL resolution.
//!
//! This module provides:
//! - [`generate_slug`] — URL-safe slug from arbitrary text (titles, headings)
//! - [`generate_uid`] — random 8-char hex UID (opaque identifier)
//! - [`insert_uid_into_frontmatter`] — boundary-aware UID insertion into markdown frontmatter
//! - [`resolve_duplicate_slugs`] / [`resolve_duplicate_slugs_with_lang`] — deduplication of URL paths

use crate::build::types::ParsedDocument;
use moss_core::PageKind;

// generate_slug and slugify_path_segments moved to moss-core (ADR-018).
pub use moss_core::slug::{generate_slug, slugify_path_segments};

/// Lowercase directory segments only; preserve the final basename verbatim.
///
/// This is the rule used by [`ServedPath::from_source`][crate::build::served_path::ServedPath::from_source]
/// for build-pipeline output paths. The bug it fixes is **case-sensitivity
/// drift** (a source `Resources/` directory landing as `resources/` on
/// case-sensitive servers), so the only transformation we need is
/// lowercasing. Slug-rewriting beyond that breaks paths that are *already
/// valid URLs* with non-ASCII or `@`-prefixed segments — e.g.,
/// `@jupyter-notebook/` (npm-scope-style names JupyterLite ships in its
/// bundled extensions directory).
///
/// The basename is preserved verbatim because third-party JS bundles
/// (JupyterLite, MathJax, p5.js) load asset files by their exact
/// filenames. Lowercase-rewriting `MathJax_Main-Bold.woff` → 404.
///
/// Examples:
/// - `Resources/habitable-zone.html` → `resources/habitable-zone.html`
/// - `jupyter/build/schemas/@jupyter-notebook/foo.json` → `jupyter/build/schemas/@jupyter-notebook/foo.json`
/// - `News/Sub Section/post.md` → `news/sub section/post.md`
/// - `Photo (1).jpg` → `Photo (1).jpg` (no dirs)
/// - `文档/介绍.html` → `文档/介绍.html`
///
/// For *URL emission* (article pretty-URLs in `compute_url_path`), the
/// filename IS slugged via `generate_slug(stem)` separately — that's
/// intentional: pretty URLs need ascii-safe slugs; static asset filenames
/// don't.
pub fn slugify_dir_path(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    // Normalize `\`→`/` so Windows backslash paths split into segments correctly
    // (otherwise the whole path is one leaf and the separator/case round-trips
    // into the output URL). moss treats `\` as a path separator everywhere.
    let path = moss_core::slug::normalize_separators(path);
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return String::new();
    }
    let last = segments.len() - 1;
    segments
        .iter()
        .enumerate()
        .map(|(i, seg)| {
            if i == last {
                seg.to_string()
            } else {
                seg.to_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Generates a random 8-character hex UID.
///
/// Produces a cryptographically random identifier each time it is called.
/// The `relative_path` parameter is accepted for API compatibility but
/// is not used — UIDs are opaque identifiers, not derived from file paths.
///
/// # Arguments
/// * `_relative_path` - Ignored; kept for API compatibility
///
/// # Returns
/// An 8-character lowercase hex string
///
/// # Examples
/// ```ignore
/// let uid = generate_uid("posts/my-article.md");
/// assert_eq!(uid.len(), 8);
/// ```
pub fn generate_uid(_relative_path: &str) -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 4] = rng.gen();
    format!("{:02x}{:02x}{:02x}{:02x}", bytes[0], bytes[1], bytes[2], bytes[3])
}

/// Inserts a `uid` field into a markdown file's frontmatter, in either dialect.
///
/// Surgical: the `uid:` line is spliced into the field text and every other byte
/// of the file — delimiters, key order, comments, quoting, line endings — is
/// left exactly as the author wrote it. Nothing is deserialized or re-emitted.
///
/// Idempotent: content that already has a `uid:` field comes back unchanged, as
/// does content with no frontmatter.
///
/// WHERE the frontmatter is, is `moss_core::frontmatter::frontmatter_span`'s
/// call, never a local scan: this writes its answer back to the author's file,
/// so a `---` misread as a delimiter corrupts their document. That is exactly
/// what a local copy did to a page whose body opened with `:::grid` (moss#932).
pub fn insert_uid_into_frontmatter(content: &str, uid: &str) -> String {
    let Some(span) = moss_core::frontmatter::frontmatter_span(content) else {
        return content.to_string();
    };
    // `fields` is a line-boundary range from the splitter, so both ends are
    // char boundaries; `get` states that instead of assuming it.
    let Some(fields) = content.get(span.fields.clone()) else {
        return content.to_string();
    };
    if fields.lines().any(|line| line.trim_start().starts_with("uid:")) {
        return content.to_string();
    }

    // Insert after the `title:` line when there is one, so the stamped file
    // reads the way an author would have written it; otherwise after the last
    // field. Both land on a line boundary inside `fields`.
    //
    // `eol` is READ off the line the new one follows, never guessed from whether
    // the file contains a `\r\n` anywhere: a mixed-ending file would then get a
    // `\r\n` spliced into an LF block. `replace_uid_in_frontmatter` below takes
    // the terminator the same way.
    let mut insert_at = span.fields.end;
    let mut offset = span.fields.start;
    // Fallback for an empty block (`---\r\n---\r\n`), where there is no field
    // line to read: the opening delimiter's own terminator.
    let opener = content.get(..span.fields.start).unwrap_or("");
    let mut eol = if opener.ends_with("\r\n") { "\r\n" } else { "\n" };
    for raw in fields.split_inclusive('\n') {
        // Char-aligned: `trim_end_matches` on ASCII terminators.
        #[allow(clippy::string_slice)]
        let terminator = &raw[raw.trim_end_matches(['\r', '\n']).len()..];
        if !terminator.is_empty() {
            eol = terminator;
        }
        offset += raw.len();
        if raw.trim_start().starts_with("title:") {
            insert_at = offset;
            break;
        }
    }
    // Quote the uid to prevent YAML scientific notation misparse
    // (e.g., `753659e7` would be parsed as float 7.53659e11 if unquoted).
    let mut out = String::with_capacity(content.len() + uid.len() + 10);
    // Char-aligned: `insert_at` is a line boundary within `fields`.
    #[allow(clippy::string_slice)]
    {
        out.push_str(&content[..insert_at]);
        out.push_str(&format!("uid: \"{}\"{}", uid, eol));
        out.push_str(&content[insert_at..]);
    }
    out
}

/// Replaces an existing `uid:` value in frontmatter with a new one.
///
/// If the content has no `uid:` field in its frontmatter, returns the content
/// unchanged. Both dialects; the body is never touched — a `uid:` line in an
/// author's prose or code block used to be rewritten, because this scanned the
/// whole file rather than the frontmatter (moss#937).
pub fn replace_uid_in_frontmatter(content: &str, new_uid: &str) -> String {
    let Some(span) = moss_core::frontmatter::frontmatter_span(content) else {
        return content.to_string();
    };
    let Some(fields) = content.get(span.fields.clone()) else {
        return content.to_string();
    };

    let mut offset = span.fields.start;
    for raw in fields.split_inclusive('\n') {
        let line_start = offset;
        offset += raw.len();
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        let line = line.strip_suffix('\r').unwrap_or(line);
        if !line.trim_start().starts_with("uid:") {
            continue;
        }
        // Preserve leading whitespace and the line's own terminator; only the
        // key/value text is replaced. Quoted to prevent YAML scientific-notation
        // misparse (`753659e7` → 7.53659e11) — and because the whole line goes,
        // a previously-quoted value cannot double-quote.
        let indent = line.strip_suffix(line.trim_start()).unwrap_or("");
        // Char-aligned: `line_start` and `offset` are line boundaries.
        #[allow(clippy::string_slice)]
        let terminator = &raw[line.len()..];
        let mut out = String::with_capacity(content.len() + new_uid.len());
        #[allow(clippy::string_slice)]
        {
            out.push_str(&content[..line_start]);
            out.push_str(indent);
            out.push_str(&format!("uid: \"{}\"{}", new_uid, terminator));
            out.push_str(&content[offset..]);
        }
        return out;
    }
    content.to_string()
}

/// One document that lost a URL contest, and what it lost.
///
/// The repair is always an edit to the **loser's** `url:` — the keeper is
/// named only so the author can see which pair is in conflict. Produced by
/// [`resolve_duplicate_slugs_with_lang`] and reported at the frontmatter
/// field, not in the log: see
/// docs/archive/2026-09-02-url-collision-as-a-frontmatter-diagnostic.md.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct UrlCollision {
    /// Source path of the file whose `url:` must change.
    pub loser: String,
    /// Source path of the file keeping the contested address.
    pub keeper: String,
    /// The address both wanted, in served form (`awards/comics/`).
    pub wanted: String,
    /// Where the loser is published instead, in served form.
    pub moved_to: String,
}

/// Windows reserved device names — `CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`,
/// `LPT1`-`LPT9` — matched case-insensitively and regardless of any
/// extension. The OS intercepts a path component spelled this way as the
/// device itself, so neither a file nor a directory can exist under one of
/// these names on an NTFS volume.
const RESERVED_WINDOWS_DEVICE_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL",
    "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Whether a page's `url_path` (`"<dir>/index.html"`, or the root
/// `"index.html"`) would create an output path with a Windows reserved
/// device name as ANY directory component — the leaf (the page's own slug)
/// or an intermediate one (a source folder that slugifies to a reserved
/// name, which becomes an ancestor directory of every page under it).
///
/// The root homepage owns no directory of its own (`None` from the
/// `strip_suffix`) and is never flagged. A moss site built directly on
/// Windows can't produce such a path at all — the source file/folder that
/// would slug to it could never have been created there — but a vault built
/// on macOS/Linux (legal filenames there) and later synced to Windows, or
/// built there directly, would try to create e.g. `nul/index.html` or
/// `con/anything/index.html`, which fails: `CreateDirectory`/`CreateFile` on
/// a segment named `nul` or `con` resolves to the device, not a path on
/// disk. Each segment is compared by the part before its first `.`, so a
/// segment carrying an extension (`nul.txt`) still matches.
fn is_reserved_device_output(url_path: &str) -> bool {
    let Some(dir) = url_path.strip_suffix("/index.html") else {
        return false;
    };
    dir.split('/').filter(|s| !s.is_empty()).any(|segment| {
        let stem = segment.split('.').next().unwrap_or(segment);
        RESERVED_WINDOWS_DEVICE_NAMES
            .iter()
            .any(|&reserved| stem.eq_ignore_ascii_case(reserved))
    })
}

/// A source folder whose index page would live at a Windows reserved device
/// name gets no index page at all: every page under it is already skipped,
/// and the directory would otherwise be created for the index alone. Warns
/// once, naming the folder; `true` means skip.
pub(crate) fn warn_reserved_folder(dir: &str, mapped_index: &str) -> bool {
    if !is_reserved_device_output(mapped_index) {
        return false;
    }
    log::warn!(
        "Skipping folder '{dir}': its output directory would be a Windows reserved device name ({mapped_index}) — cannot exist on NTFS"
    );
    true
}

/// Pushes `doc` onto `docs`, unless [`is_reserved_device_output`] flags its
/// `url_path` — in which case the page is dropped, with one warning naming
/// the file, instead of failing the whole build. Called once per page from
/// the sequential reduce in `build::render::blocking`, before the
/// whole-corpus passes (slug dedup, folder synthesis, ...) can see it.
pub(crate) fn push_unless_reserved_device_output(
    docs: &mut Vec<crate::build::types::ParsedDocument>,
    doc: crate::build::types::ParsedDocument,
) {
    if is_reserved_device_output(&doc.url_path) {
        log::warn!(
            "Skipping '{}': its output directory would be a Windows reserved device name ({}) — cannot exist on NTFS",
            doc.source_path.as_deref().unwrap_or(&doc.url_path),
            doc.url_path,
        );
        return;
    }
    docs.push(doc);
}

/// Number the DIRECTORY, never the file.
///
/// `<dir>/index.html` is the only shape `to_pretty_url` gives a trailing
/// slash, and a trailing-slash URL is the only kind `ServeDir` routes — so
/// `<dir>/index-2.html` served as `<dir>/index-2` and 404'd with its bytes on
/// disk. `compute_url_path` returns `.../index.html` on every branch; the root
/// home (`index.html`) is the one page with no directory of its own, and gets
/// one here, so the result is directory-shaped unconditionally.
/// See docs/archive/2026-09-01-duplicate-url-override-index-n-404.md.
fn numbered(url_path: &str, n: u32) -> String {
    match url_path.strip_suffix("/index.html") {
        Some(dir) => format!("{}-{}/index.html", dir, n),
        None => format!("{}-{}/index.html", url_path.trim_end_matches(".html"), n),
    }
}

/// Resolves duplicate URL paths with language awareness.
///
/// When multiple documents share the same `url_path`, the document whose
/// language matches `site_lang` keeps the primary slot; folder indexes outrank
/// both. Within a tier, filesystem order (slice order) is preserved.
///
/// Every loser then *claims* a URL: its preferred one — lang-prefixed when its
/// language differs from the keeper's, the original otherwise — or, if that is
/// already spoken for, the next free numbered directory. One claim rule rather
/// than one per branch, because the branches had drifted: the lang-prefix path
/// carried no counter (three `zh-hans` documents against one `en` primary all
/// landed on `zh-hans/about/index.html`) and never checked whether a real page
/// already sat at the prefixed address.
///
/// Returns the collisions an author must settle: those where numbering was
/// needed, minus the ones a numbered ancestor already explains. A
/// cross-language pair resolved by its prefix is the designed outcome and is
/// not a collision. See
/// docs/archive/2026-09-02-url-collision-as-a-frontmatter-diagnostic.md.
pub fn resolve_duplicate_slugs_with_lang(
    documents: &mut [ParsedDocument],
    site_lang: crate::i18n::Language,
) -> Vec<UrlCollision> {
    use crate::build::scan::article_map::to_pretty_url;
    use std::collections::HashMap;

    // Who holds each URL. Seeded with every document so a preferred
    // lang-prefixed address that belongs to a REAL page (a site with a literal
    // `zh-hans/` folder) is seen as taken instead of being clobbered.
    let mut holder: HashMap<String, usize> = HashMap::new();
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, doc) in documents.iter().enumerate() {
        holder.entry(doc.url_path.clone()).or_insert(i);
        groups.entry(doc.url_path.clone()).or_default().push(i);
    }

    // `groups` is a HashMap, so sort by path to keep assignment (and therefore
    // which page gets `-2`) stable across builds.
    let mut collisions: Vec<(&String, &Vec<usize>)> =
        groups.iter().filter(|(_, idx)| idx.len() > 1).collect();
    collisions.sort_by(|a, b| a.0.cmp(b.0));

    // Reported collisions, each paired with the directory it cascades over
    // (`Some` only for a folder index, the only kind whose `url:` cascades).
    let mut found: Vec<(UrlCollision, Option<String>)> = Vec::new();

    for (path, indices) in collisions {
        // Rank so the canonical doc comes first. `sort_by_key` is stable, so
        // filesystem order still decides within a tier: folder indexes (home
        // file winners keep the primary URL slot), then site_lang docs, then
        // the rest.
        let mut sorted_indices: Vec<usize> = indices.clone();
        sorted_indices.sort_by_key(|&i| {
            if documents[i].kind == PageKind::Folder {
                0u8
            } else if documents[i].lang == site_lang {
                1
            } else {
                2
            }
        });

        let keeper_idx = sorted_indices[0];
        let original_path = documents[keeper_idx].url_path.clone();
        let primary_lang = documents[keeper_idx].lang;
        holder.insert(original_path.clone(), keeper_idx);

        for &doc_idx in sorted_indices.iter().skip(1) {
            let preferred = if documents[doc_idx].lang != primary_lang {
                format!("{}/{}", documents[doc_idx].lang.code(), original_path)
            } else {
                original_path.clone()
            };

            // Claim `preferred`, or the first free numbered directory under it.
            // Numbering means someone else already answers at that address, and
            // that someone is the keeper the author needs named.
            let blocked_by = holder.get(&preferred).copied();
            let assigned = match blocked_by {
                None => preferred.clone(),
                Some(_) => (2u32..)
                    .map(|n| numbered(&preferred, n))
                    .find(|cand| !holder.contains_key(cand))
                    .expect("numbering is unbounded"),
            };
            holder.insert(assigned.clone(), doc_idx);
            documents[doc_idx].url_path = assigned.clone();

            // A prefixed cross-language page is the designed outcome; only a
            // page that had to be NUMBERED is one the author must settle.
            // Renaming keeps it reachable but cannot save its images: asset
            // output paths resolve from the source directory, not from
            // `url_path`, so the two trees still overwrite each other.
            let Some(blocker) = blocked_by else { continue };
            let source_of = |i: usize| {
                documents[i]
                    .source_path
                    .clone()
                    .unwrap_or_else(|| documents[i].url_path.clone())
            };
            let loser = source_of(doc_idx);
            let cascades_over = (documents[doc_idx].kind == PageKind::Folder)
                .then(|| parent_dir(&loser).to_string());
            found.push((
                UrlCollision {
                    loser,
                    keeper: source_of(blocker),
                    wanted: to_pretty_url(&preferred),
                    moved_to: to_pretty_url(&assigned),
                },
                cascades_over,
            ));
        }
    }

    let reported = root_causes(found);
    // The author's copy of this goes on the `url` chip, where the field she
    // has to change is; this is for the support read of an uploaded log,
    // which is how the 潮汐·週報 report was diagnosed at all.
    for c in &reported {
        log::warn!(
            target: "build",
            "`{}` and `{}` both publish to /{} — keeping `{}` there and moving the other to /{}. Two folders carrying the same `url:` do this; a folder copied in Finder keeps the original's.",
            c.keeper, c.loser, c.wanted, c.keeper, c.moved_to,
        );
    }
    reported
}

/// The directory part of a source path (`a/b/c.md` → `a/b`, `c.md` → ``).
fn parent_dir(source_path: &str) -> &str {
    source_path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}

/// Drop every collision a numbered folder above it already explains.
///
/// A folder's `url:` cascades to its descendants, so one duplicated folder
/// produces a collision for the folder AND for every page beneath it — the
/// 潮汐·週報 report was ~20 of them for a single copied folder. Every deeper
/// one is a consequence, and reporting them all buries the single edit that
/// fixes the lot: the children's own `url:` segments are relative and correct,
/// so changing the parent's resolves them untouched.
fn root_causes(found: Vec<(UrlCollision, Option<String>)>) -> Vec<UrlCollision> {
    // Paired with its own index: a folder note lives INSIDE the directory it
    // cascades over (`獎項/記憶獎/記憶獎.md`), so a collision compared against
    // its own directory would suppress itself and report nothing at all.
    let cascading: Vec<(usize, String)> = found
        .iter()
        .enumerate()
        .filter_map(|(i, (_, dir))| dir.as_ref().map(|d| (i, format!("{}/", d))))
        .collect();
    found
        .iter()
        .enumerate()
        .filter(|(i, (c, _))| {
            !cascading
                .iter()
                .any(|(j, dir)| j != i && c.loser.starts_with(dir.as_str()))
        })
        .map(|(_, (c, _))| c.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // =========================================================================
    // Tests for generate_slug
    // =========================================================================

    #[test]
    fn test_generate_slug_basic() {
        assert_eq!(generate_slug("Hello World"), "hello-world");
        assert_eq!(generate_slug("My Blog Post"), "my-blog-post");
    }

    #[test]
    fn test_generate_slug_special_characters() {
        assert_eq!(generate_slug("API & Documentation"), "api-and-documentation");
        assert_eq!(generate_slug("Contact @ Email"), "contact-at-email");
        assert_eq!(generate_slug("C++ Programming"), "cplusplus-programming");
        assert_eq!(generate_slug("50% Off"), "50percent-off");
        assert_eq!(generate_slug("FAQ #1"), "faq-hash1");
    }

    #[test]
    fn test_generate_slug_underscores() {
        assert_eq!(generate_slug("file_name_example"), "file-name-example");
        assert_eq!(generate_slug("snake_case_title"), "snake-case-title");
    }

    #[test]
    fn test_generate_slug_remove_invalid_chars() {
        assert_eq!(generate_slug("Title (with) brackets"), "title-with-brackets");
        assert_eq!(generate_slug("Price: $99.99"), "price-99.99");
        assert_eq!(generate_slug("User/Admin/Settings"), "user-admin-settings");
    }

    #[test]
    fn test_generate_slug_consecutive_hyphens() {
        assert_eq!(generate_slug("Multiple   Spaces"), "multiple-spaces");
        assert_eq!(generate_slug("Too---Many-Hyphens"), "too-many-hyphens");
        assert_eq!(generate_slug("Mixed _-_ Separators"), "mixed-separators");
    }

    #[test]
    fn test_generate_slug_edge_cases() {
        assert_eq!(generate_slug(""), "untitled");
        assert_eq!(generate_slug("   "), "untitled");
        assert_eq!(generate_slug("---"), "untitled");
        assert_eq!(generate_slug("$@%!"), "atpercent");
    }

    #[test]
    fn test_generate_slug_unicode() {
        // Unicode letters should be preserved (CJK, accented chars, etc.)
        assert_eq!(generate_slug("Café Menu"), "café-menu");
        assert_eq!(generate_slug("Résumé Template"), "résumé-template");

        // CJK characters should be preserved
        assert_eq!(generate_slug("你好世界"), "你好世界");
        assert_eq!(generate_slug("日本語テスト"), "日本語テスト");
        assert_eq!(generate_slug("한국어 테스트"), "한국어-테스트");

        // Mixed CJK and ASCII
        assert_eq!(generate_slug("Hello 世界"), "hello-世界");
        assert_eq!(generate_slug("文章 123 Test"), "文章-123-test");
    }

    #[test]
    fn test_generate_slug_length_limit() {
        let very_long = "a".repeat(200);
        let result = generate_slug(&very_long);
        assert!(result.len() <= 100);
        assert_eq!(result, "a".repeat(100));
    }

    #[test]
    fn test_generate_slug_trim_hyphens() {
        assert_eq!(generate_slug("-leading hyphen"), "leading-hyphen");
        assert_eq!(generate_slug("trailing hyphen-"), "trailing-hyphen");
        assert_eq!(generate_slug("-both-"), "both");
    }

    // =========================================================================
    // Tests for generate_uid
    // =========================================================================

    #[test]
    fn test_generate_uid_consistent() {
        // Each call produces a new random UID — two calls should differ
        let uid1 = generate_uid("posts/my-article.md");
        let uid2 = generate_uid("posts/my-article.md");
        // Statistically, two random 32-bit values collide 1-in-4-billion times.
        // We assert they are different to document the random behaviour; flakes
        // are negligible in practice.
        assert_ne!(uid1, uid2, "Random UIDs should be different on each call");
    }

    #[test]
    fn test_generate_uid_length() {
        // uid should be exactly 8 hex characters
        let uid = generate_uid("posts/my-article.md");
        assert_eq!(uid.len(), 8, "uid should be 8 characters long");
        assert!(uid.chars().all(|c| c.is_ascii_hexdigit()), "uid should be hex characters only");
    }

    // =========================================================================
    // Tests for replace_uid_in_frontmatter
    // =========================================================================

    #[test]
    fn test_replace_uid_writes_the_quoted_form() {
        // Same rationale as insert_uid_into_frontmatter's quoting: an
        // unquoted digits-e-digits uid (`12e45678`) parses as a YAML float
        // and the next build reads a 13-digit string — a silent identity
        // flip. The replace path writes freshly MINTED uids (the duplicate
        // resolver's losers), so it is exactly where that shape appears.
        let content = "---\ntitle: T\nuid: aabbccdd\n---\n\nbody\n";
        let result = replace_uid_in_frontmatter(content, "12e45678");
        assert!(
            result.contains(r#"uid: "12e45678""#),
            "replace must quote the uid: {result}"
        );
        assert!(result.contains("\n\nbody\n"), "body preserved: {result}");
    }

    #[test]
    fn test_replace_uid_requotes_a_quoted_line_without_doubling() {
        // The helper replaces the whole line, so a previously-quoted uid
        // must come back with exactly one pair of quotes.
        let content = "---\nuid: \"aabbccdd\"\n---\nbody\n";
        let result = replace_uid_in_frontmatter(content, "753659e7");
        assert!(result.contains(r#"uid: "753659e7""#), "{result}");
        assert!(!result.contains(r#""""#), "no doubled quotes: {result}");
    }

    #[test]
    fn test_replace_uid_without_a_uid_line_is_unchanged() {
        let content = "---\ntitle: T\n---\nbody\n";
        assert_eq!(replace_uid_in_frontmatter(content, "12e45678"), content);
    }

    // =========================================================================
    // Tests for insert_uid_into_frontmatter
    // =========================================================================

    /// Byte-exact, both dialects: the uid line is spliced in and NOTHING else
    /// moves. Comments, key order, quoting style and line endings are the
    /// author's, and a save that reflowed them would be worse than the
    /// duplication this consolidation removed.
    #[test]
    fn insert_uid_is_a_splice_and_changes_nothing_else() {
        let cases: &[(&str, &str, &str)] = &[
            (
                "yaml, after the title line",
                "---\ntitle: My Article\ndate: 2024-01-15\n---\n\n# Hello\n\nBody.",
                "---\ntitle: My Article\nuid: \"a7b3c9d2\"\ndate: 2024-01-15\n---\n\n# Hello\n\nBody.",
            ),
            (
                "yaml, no title — appended after the last field",
                "---\ndate: 2024-01-15\nweight: 5\n---\n\nBody.",
                "---\ndate: 2024-01-15\nweight: 5\nuid: \"a7b3c9d2\"\n---\n\nBody.",
            ),
            (
                "yaml comments, key order and quoting all survive",
                "---\n# who wrote it\nauthor: 'Ada'   # trailing note\ntitle: T\ntags:\n  - a\n  - b\n---\nbody\n",
                "---\n# who wrote it\nauthor: 'Ada'   # trailing note\ntitle: T\nuid: \"a7b3c9d2\"\ntags:\n  - a\n  - b\n---\nbody\n",
            ),
            (
                "empty yaml block",
                "---\n---\nbody\n",
                "---\nuid: \"a7b3c9d2\"\n---\nbody\n",
            ),
            (
                "simplified",
                "nav\nlist: grid\n---\n\n# Hello",
                "nav\nlist: grid\nuid: \"a7b3c9d2\"\n---\n\n# Hello",
            ),
            (
                "simplified, after the title line",
                "title: Hi\nnav\n---\nbody\n",
                "title: Hi\nuid: \"a7b3c9d2\"\nnav\n---\nbody\n",
            ),
            (
                "CRLF keeps its line endings",
                "---\r\ntitle: T\r\n---\r\nbody\r\n",
                "---\r\ntitle: T\r\nuid: \"a7b3c9d2\"\r\n---\r\nbody\r\n",
            ),
            (
                // Mixed endings: the block is LF, the body has a CRLF. The eol is
                // read off the line the new one follows, not guessed from
                // whether the file contains a `\r\n` anywhere.
                "mixed line endings — the block's own ending wins",
                "---\ntitle: T\n---\nbody\r\nmore\n",
                "---\ntitle: T\nuid: \"a7b3c9d2\"\n---\nbody\r\nmore\n",
            ),
            (
                "CRLF with an empty block reads the opening delimiter's ending",
                "---\r\n---\r\nbody\r\n",
                "---\r\nuid: \"a7b3c9d2\"\r\n---\r\nbody\r\n",
            ),
            (
                "already stamped — idempotent",
                "---\ntitle: My Article\nuid: existing123\n---\n\nBody.",
                "---\ntitle: My Article\nuid: existing123\n---\n\nBody.",
            ),
            (
                "no frontmatter at all",
                "# Just content\n\nNo frontmatter here.",
                "# Just content\n\nNo frontmatter here.",
            ),
        ];
        for (label, input, expected) in cases {
            assert_eq!(&insert_uid_into_frontmatter(input, "a7b3c9d2"), expected, "{label}");
        }
    }

    /// `replace_uid_in_frontmatter` used to scan the WHOLE file for the first
    /// `uid:` line, so a `uid:` an author wrote in prose or inside a fenced
    /// YAML example was rewritten instead. It is bounded to the frontmatter now.
    #[test]
    fn replace_uid_never_touches_the_body() {
        let with_body_uid = "---\ntitle: T\nuid: aabbccdd\n---\n\n```yaml\nuid: dontTouchMe\n```\n";
        assert_eq!(
            replace_uid_in_frontmatter(with_body_uid, "753659e7"),
            "---\ntitle: T\nuid: \"753659e7\"\n---\n\n```yaml\nuid: dontTouchMe\n```\n",
        );
        // Only the body has one: nothing to replace, file comes back untouched.
        let body_only = "---\ntitle: T\n---\n\nuid: notAField\n";
        assert_eq!(replace_uid_in_frontmatter(body_only, "753659e7"), body_only);
        // No frontmatter at all: likewise.
        let no_fm = "# Notes\n\nuid: notAField\n";
        assert_eq!(replace_uid_in_frontmatter(no_fm, "753659e7"), no_fm);
    }

    /// The shape of a real, in-production `footer.md` (harbor.mosspub.com):
    /// no frontmatter, opens with literal HTML, and its site map is a
    /// `:::grid 3` whose cells are separated by `---`. A live build stamped
    /// `uid: "2af1e0f6"` in front of that first grid `---`, which rendered as a
    /// stray `<p>uid: "2af1e0f6"</p>` at the end of the first column — and,
    /// worse, rewrote the author's file on disk to match.
    ///
    /// Byte-identical is the whole assertion. Anything else means moss edited a
    /// file it had no business editing.
    const HARBOR_FOOTER: &str = concat!(
        "<div class=\"fs-signup\">\n",
        "<p class=\"fs-invite\">訂閱潮汐，第一手收到消息。</p>\n",
        "\n",
        ":::subscribe {placeholder=\"你的電子郵件\" button=\"訂閱\"}\n",
        ":::\n",
        "\n",
        "</div>\n",
        "\n",
        "<nav class=\"fs-map\" aria-label=\"網站地圖\">\n",
        "\n",
        ":::grid 3\n",
        "**[[獎項]]**\n",
        "\n",
        "- [[現正徵件]]\n",
        "\n",
        "---\n",
        "\n",
        "**[[社群]]**\n",
        "\n",
        "- [[活動]]\n",
        "\n",
        "---\n",
        "\n",
        "**[[關於]]**\n",
        "\n",
        "- [[支持我們]]\n",
        ":::\n",
        "\n",
        "</nav>\n",
    );

    /// The second file the same build corrupted, and the reason to believe it
    /// was one bug and not two: a plain README with no frontmatter, opening
    /// with an `#` heading, whose first `---` is the thematic break under the
    /// status list. `uid: "dd54e60c"` was spliced immediately above that break,
    /// exactly as in the footer. Neither file's first line is a field line, so
    /// today both are refused at the first line of the scan.
    const HARBOR_README: &str = concat!(
        "# Harbor Weekly (潮汐 · 週報) → moss port\n",
        "\n",
        "Entrance / status board for porting https://www.harborweekly.io/ (a\n",
        "Strikingly site) to moss.\n",
        "\n",
        "- **The vault:** [`潮汐/`](潮汐/) — the moss site.\n",
        "- **Port records:** [`archive/`](archive/) — the completed port machinery.\n",
        "\n",
        "---\n",
        "\n",
        "## To finish the site\n",
    );

    #[test]
    fn test_insert_uid_leaves_the_two_corrupted_harbor_files_byte_identical() {
        // Both files were found carrying a spliced `uid:` line on disk. Whatever
        // wrote them, nothing may write them again.
        for (label, content) in
            [("footer.md", HARBOR_FOOTER), ("README.md", HARBOR_README)]
        {
            assert_eq!(
                &insert_uid_into_frontmatter(content, "2af1e0f6"),
                content,
                "{label}: a file with no frontmatter must come back byte-for-byte"
            );
        }
    }

    #[test]
    fn test_insert_uid_leaves_body_openers_that_shadow_a_field_line() {
        // Each of these opens the BODY, not frontmatter, and each is a shape
        // that could plausibly be mistaken for a `key: value` field line and so
        // let the scan run on to a `---` further down. The `---` in every case
        // is a grid-cell separator, a fenced example or a thematic break —
        // never a delimiter. Where the judgement itself lives and why each shape
        // fails it is moss-core's business (`frontmatter_span`);
        // what this test pins is that the *writing* path honours it, because
        // this is the path that edits the author's file on disk.
        let bodies: &[(&str, &str)] = &[
            ("leading blank line", "\n<div class=\"x\">\n\n---\n\n</div>\n"),
            ("HTML comment", "<!-- built from the site map -->\n\n---\n\ntail\n"),
            // `Note:` looks exactly like `key: value` but for the capital N —
            // the one thing that tells an English sentence from a field.
            ("sentence with a colon", "Note: the deadline moved.\n\n---\n\ntail\n"),
            // An HTML attribute value carries a colon too; the "key" here is
            // `<div style="color`, which no field name could be.
            ("HTML attribute colon", "<div style=\"color: red\">\n\n---\n\n</div>\n"),
            // A directive line begins with the colons, so the text before the
            // first `:` is empty — also not a field name.
            ("directive on line 1", ":::grid 3\nA\n\n---\n\nB\n:::\n"),
            // Setext: `Heading\n---` is CommonMark for an `<h2>`, and a bare
            // word is not a known flag.
            ("setext heading", "Introduction\n---\n\ntail\n"),
            // A bare URL splits on the scheme colon; `https` is lowercase and
            // alphanumeric, so only the setext `---` below it stands between
            // this and a splice.
            ("bare URL then dashes", "https://example.com/x\n---\n\ntail\n"),
            // A docs page showing a YAML example: the `---` pair belongs to the
            // example, and the intro line above it is prose.
            ("backtick fence", "Some text.\n\n```yaml\n---\ntitle: Example\n---\n```\n\nMore.\n"),
            ("tilde fence", "Intro.\n\n~~~yaml\n---\ntitle: Example\n---\n~~~\n\nOutro.\n"),
            // Once body has gone by, a `---` is a thematic break. Nine of moss's
            // own docs pages look exactly like this.
            ("thematic break after a fence", "# Notes\n\n```sh\nmoss build\n```\n\nOlder:\n\n---\n\n## 0.1\n"),
            // Everything above again with CRLF: byte offsets must survive the
            // extra `\r` or the splice lands mid-line.
            ("CRLF html opener", "<div class=\"x\">\r\n\r\n---\r\n\r\n</div>\r\n"),
        ];
        for (label, content) in bodies {
            assert_eq!(
                &insert_uid_into_frontmatter(content, "2af1e0f6"),
                content,
                "{label}: body opener must not be read as frontmatter"
            );
        }
    }

    #[test]
    fn test_insert_uid_stamps_a_real_simplified_frontmatter_prefix() {
        // The guard must stay narrow enough to still do its job.
        let content = "nav\ntitle: Hi\n---\n\n# Body";
        let result = insert_uid_into_frontmatter(content, "a7b3c9d2");

        assert!(
            result.contains(r#"uid: "a7b3c9d2""#),
            "field lines closed by --- are frontmatter: {result}"
        );
        assert!(result.ends_with("---\n\n# Body"), "body untouched: {result}");
    }

    // =========================================================================
    // Tests for resolve_duplicate_slugs
    // =========================================================================

    /// Helper function to create a minimal ParsedDocument for testing
    fn make_test_doc(title: &str, url_path: &str) -> ParsedDocument {
        ParsedDocument {
            title: title.to_string(),
            label: title.to_string(),
            url_path: url_path.to_string(),
            reading_time: 1,
            slug: title.to_lowercase().replace(' ', "-"),
            permalink: format!("/{}", url_path), // allow:served-path-url-construct (test fixture — permalink field, not HTML-emitted URL)
            kind: PageKind::Article,
            ..Default::default()
        }
    }

    /// A `zh-hans` document — the losing side of a cross-language contest.
    fn zh(title: &str, url_path: &str) -> ParsedDocument {
        ParsedDocument { lang: crate::i18n::Language::ZhHans, ..make_test_doc(title, url_path) }
    }

    /// A folder index with a source path, which is what a collision names.
    fn folder(title: &str, url_path: &str, source: &str) -> ParsedDocument {
        ParsedDocument {
            kind: PageKind::Folder,
            source_path: Some(source.to_string()),
            ..make_test_doc(title, url_path)
        }
    }

    #[test]
    fn test_resolve_duplicate_slugs_no_duplicates() {
        let mut docs = vec![
            make_test_doc("First", "first.html"),
            make_test_doc("Second", "second.html"),
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, crate::i18n::Language::En);

        assert_eq!(docs[0].url_path, "first.html");
        assert_eq!(docs[1].url_path, "second.html");
    }

    #[test]
    fn test_resolve_duplicate_slugs_with_duplicates() {
        let mut docs = vec![
            make_test_doc("Hello World", "hello.html"),
            make_test_doc("Hello Again", "hello.html"),
            make_test_doc("Hello Once More", "hello.html"),
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, crate::i18n::Language::En);

        assert_eq!(docs[0].url_path, "hello.html", "First should keep original");
        assert_eq!(
            docs[1].url_path, "hello-2/index.html",
            "Second is numbered into its own directory, so it stays servable"
        );
        assert_eq!(
            docs[2].url_path, "hello-3/index.html",
            "Third likewise"
        );
    }

    #[test]
    fn test_resolve_duplicate_slugs_with_directories() {
        let mut docs = vec![
            make_test_doc("Post One", "posts/test.html"),
            make_test_doc("Post Two", "posts/test.html"),
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, crate::i18n::Language::En);

        assert_eq!(docs[0].url_path, "posts/test.html");
        assert_eq!(docs[1].url_path, "posts/test-2/index.html");
    }

    #[test]
    fn test_resolve_duplicate_slugs_mixed_paths() {
        // Different directories should NOT conflict
        let mut docs = vec![
            make_test_doc("Post", "posts/test.html"),
            make_test_doc("Page", "pages/test.html"),
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, crate::i18n::Language::En);

        // Different directories - no conflict
        assert_eq!(docs[0].url_path, "posts/test.html");
        assert_eq!(docs[1].url_path, "pages/test.html");
    }

    /// The regression that earned the directory-numbering rule.
    ///
    /// A shape assertion alone does not say why the shape matters, so compose
    /// the two functions the real bug ran through: the deduplicated page has to
    /// come out of `to_pretty_url` as a trailing-slash directory URL, because
    /// that is the only form the preview's `ServeDir` can route. The old
    /// `<dir>/index-2.html` came out as the extensionless `<dir>/index-2` and
    /// 404'd with its bytes on disk.
    /// See docs/archive/2026-09-01-duplicate-url-override-index-n-404.md.
    #[test]
    fn every_deduplicated_page_gets_a_servable_url() {
        use crate::build::scan::article_map::to_pretty_url;
        use crate::i18n::Language;

        // Two folders carrying the same `url:` override — a duplicated folder
        // keeps the original's — plus the root home, which has no directory of
        // its own to number.
        let mut docs = vec![
            make_test_doc("Comics", "awards/comics/index.html"),
            make_test_doc("Memory", "awards/comics/index.html"),
            make_test_doc("Home", "index.html"),
            make_test_doc("Home copy", "index.html"),
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, Language::En);

        for doc in &docs {
            let url = to_pretty_url(&doc.url_path);
            assert!(
                url.is_empty() || url.ends_with('/'),
                "`{}` serves as `/{}`, which ServeDir has no route for",
                doc.url_path,
                url
            );
        }
        assert_eq!(docs[1].url_path, "awards/comics-2/index.html");
        assert_eq!(docs[3].url_path, "index-2/index.html");
    }

    /// Three same-language losers against one primary of another language.
    ///
    /// The old lang-prefix branch had no counter, so all three landed on
    /// `zh-hans/about/index.html` — one address, three documents, last render
    /// wins. Filed as moss#1172; fixed by making both branches claim.
    #[test]
    fn a_lang_prefix_is_claimed_once_and_then_numbered() {
        use crate::i18n::Language;

        let mut docs = vec![
            zh("About", "about/index.html"),
            zh("About copy", "about/index.html"),
            zh("About copy 2", "about/index.html"),
        ];
        docs[0].lang = Language::En;

        resolve_duplicate_slugs_with_lang(&mut docs, Language::En);

        assert_eq!(docs[0].url_path, "about/index.html");
        assert_eq!(docs[1].url_path, "zh-hans/about/index.html");
        assert_eq!(docs[2].url_path, "zh-hans/about-2/index.html");
    }

    /// A real page already sitting at the prefixed address is not clobbered.
    #[test]
    fn a_lang_prefix_never_overwrites_an_existing_page() {
        use crate::i18n::Language;

        let mut docs = vec![
            make_test_doc("About", "about/index.html"),
            zh("About zh", "about/index.html"),
            // A site with a literal `zh-hans/` folder of its own.
            make_test_doc("Squatter", "zh-hans/about/index.html"),
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, Language::En);

        assert_eq!(docs[2].url_path, "zh-hans/about/index.html");
        assert_eq!(docs[1].url_path, "zh-hans/about-2/index.html");
    }

    /// One duplicated folder is one collision, not one per page beneath it.
    ///
    /// The children's own `url:` segments are relative and correct — changing
    /// the parent's resolves them untouched — so reporting them buries the
    /// single edit that fixes the lot.
    #[test]
    fn only_the_folder_that_caused_the_cascade_is_reported() {
        use crate::i18n::Language;

        let mut docs = vec![
            folder("Comics", "awards/comics/index.html", "獎項/漫畫獎/漫畫獎.md"),
            folder("Memory", "awards/comics/index.html", "獎項/記憶獎/記憶獎.md"),
            folder("S1 orig", "awards/comics/s1/index.html", "獎項/漫畫獎/第一季/第一季.md"),
            folder("S1 copy", "awards/comics/s1/index.html", "獎項/記憶獎/第一季/第一季.md"),
        ];
        docs[2].kind = PageKind::Folder;
        docs[3].kind = PageKind::Folder;

        let reported = resolve_duplicate_slugs_with_lang(&mut docs, Language::En);

        assert_eq!(reported.len(), 1, "got {:?}", reported);
        assert_eq!(reported[0].loser, "獎項/記憶獎/記憶獎.md");
        assert_eq!(reported[0].keeper, "獎項/漫畫獎/漫畫獎.md");
        assert_eq!(reported[0].wanted, "awards/comics/");
        assert_eq!(reported[0].moved_to, "awards/comics-2/");
    }

    /// A cross-language pair resolved by its prefix is the designed outcome,
    /// not something to tell a bilingual author about on every rebuild.
    #[test]
    fn a_prefixed_translation_is_not_reported_as_a_collision() {
        use crate::i18n::Language;

        let mut docs = vec![
            make_test_doc("About", "about/index.html"),
            zh("About zh", "about/index.html"),
        ];

        assert!(resolve_duplicate_slugs_with_lang(&mut docs, Language::En).is_empty());
    }

    #[test]
    fn test_resolve_duplicate_slugs_with_lang_site_lang_wins_primary() {
        // ZH doc listed first, EN doc second. site_lang=En.
        // EN should get the primary slot, ZH should get lang-prefix directory.
        use crate::i18n::Language;

        let mut docs = vec![
            {
                let mut d = make_test_doc("你好世界", "hello/index.html");
                d.lang = Language::ZhHans;
                d
            },
            {
                let mut d = make_test_doc("Hello World", "hello/index.html");
                d.lang = Language::En;
                d
            },
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, Language::En);

        // EN doc should keep primary slot
        let en_doc = docs.iter().find(|d| d.lang == Language::En).unwrap();
        assert_eq!(en_doc.url_path, "hello/index.html", "EN should keep primary slot");

        // ZH doc should get lang-prefix directory
        let zh_doc = docs.iter().find(|d| d.lang == Language::ZhHans).unwrap();
        assert_eq!(zh_doc.url_path, "zh-hans/hello/index.html", "ZH should get lang-prefix directory");
    }

    #[test]
    fn test_resolve_duplicate_slugs_with_lang_same_lang_no_reorder() {
        // Both docs are same language — first one keeps primary (filesystem order)
        use crate::i18n::Language;

        let mut docs = vec![
            make_test_doc("First", "test/index.html"),
            make_test_doc("Second", "test/index.html"),
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, Language::En);

        assert_eq!(docs[0].url_path, "test/index.html");
        assert_eq!(docs[1].url_path, "test-2/index.html");
    }

    #[test]
    fn test_resolve_duplicate_slugs_homepage_translation() {
        // When index.md (is_index=true) and index.zh-hans.md (is_index=false, demoted)
        // both get url_path "index.html", the home file winner (is_index=true)
        // keeps the primary slot regardless of site language detection.
        // Regression test: previously index.zh-hans.md got url_path "index/index.html"
        // (a separate directory) instead of being deduplicated with the homepage.
        use crate::i18n::Language;

        let mut docs = vec![
            {
                let mut d = make_test_doc("My Site", "index.html");
                d.lang = Language::En;
                d.clean_stem = "index".to_string();
                d.kind = PageKind::Folder; // Winner of detect_home_file_in_folder
                d
            },
            {
                let mut d = make_test_doc("我的网站", "index.html");
                d.lang = Language::ZhHans;
                d.clean_stem = "index".to_string();
                d.kind = PageKind::Article; // Demoted translation
                d
            },
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, Language::En);

        let en_doc = docs.iter().find(|d| d.lang == Language::En).unwrap();
        assert_eq!(en_doc.url_path, "index.html", "EN homepage (winner) should keep primary slot");

        let zh_doc = docs.iter().find(|d| d.lang == Language::ZhHans).unwrap();
        assert_eq!(zh_doc.url_path, "zh-hans/index.html", "ZH homepage translation should get lang-prefix directory");
    }

    #[test]
    fn test_resolve_duplicate_slugs_homepage_winner_beats_site_lang() {
        // Even when site language is Chinese, the home file winner (is_index=true)
        // should keep index.html. This prevents language detection from displacing
        // the canonical homepage chosen by detect_home_file_in_folder.
        use crate::i18n::Language;

        let mut docs = vec![
            {
                let mut d = make_test_doc("My Site", "index.html");
                d.lang = Language::En;
                d.clean_stem = "index".to_string();
                d.kind = PageKind::Folder; // Winner
                d
            },
            {
                let mut d = make_test_doc("我的网站", "index.html");
                d.lang = Language::ZhHans;
                d.clean_stem = "index".to_string();
                d.kind = PageKind::Article; // Demoted translation
                d
            },
        ];

        // Site language is ZH, but the winner should still keep index.html
        resolve_duplicate_slugs_with_lang(&mut docs, Language::ZhHans);

        let en_doc = docs.iter().find(|d| d.lang == Language::En).unwrap();
        assert_eq!(en_doc.url_path, "index.html", "Home file winner should keep primary slot even when site_lang differs");

        let zh_doc = docs.iter().find(|d| d.lang == Language::ZhHans).unwrap();
        assert_eq!(zh_doc.url_path, "zh-hans/index.html", "Demoted translation should get lang-prefix directory");
    }

    #[test]
    fn test_resolve_duplicate_slugs_three_languages() {
        // EN, ZH-Hans, and ZH-Hant all produce the same slug.
        // EN (site_lang) keeps primary, each translation gets its own lang-prefix dir.
        use crate::i18n::Language;

        let mut docs = vec![
            {
                let mut d = make_test_doc("About", "about/index.html");
                d.lang = Language::En;
                d
            },
            {
                let mut d = make_test_doc("关于", "about/index.html");
                d.lang = Language::ZhHans;
                d
            },
            {
                let mut d = make_test_doc("關於", "about/index.html");
                d.lang = Language::ZhHant;
                d
            },
        ];

        resolve_duplicate_slugs_with_lang(&mut docs, Language::En);

        let en = docs.iter().find(|d| d.lang == Language::En).unwrap();
        assert_eq!(en.url_path, "about/index.html", "EN keeps primary slot");

        let zh_hans = docs.iter().find(|d| d.lang == Language::ZhHans).unwrap();
        assert_eq!(zh_hans.url_path, "zh-hans/about/index.html", "ZH-Hans gets lang-prefix");

        let zh_hant = docs.iter().find(|d| d.lang == Language::ZhHant).unwrap();
        assert_eq!(zh_hant.url_path, "zh-hant/about/index.html", "ZH-Hant gets lang-prefix");
    }
}
