//! Scrape configuration and frontmatter rendering.
//!
//! [`ScrapeConfig`] is the single shared config struct passed into
//! [`super::run::scrape_to_folder`] from both the Tauri command and the
//! CLI. [`generate_frontmatter`] renders an [`super::metadata::ArticleMetadata`]
//! + source URL as YAML for the head of each imported `.md`.

use std::path::PathBuf;
use std::sync::LazyLock;

use regex::Regex;
use url::Url;

use super::metadata::ArticleMetadata;
use super::scope::{is_within_scope, UrlScope};

// `[text](url)` markdown link, captured once and reused across pages.
static LINK_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]]*)\]\(([^)]+)\)").expect("link regex"));

/// User-Agent used by all outbound requests from the scrape pipeline.
///
/// Built at runtime, not by `concat!(env!("CARGO_PKG_VERSION"))`: the module
/// crossed into moss-build on 2026-09-07, where that macro would expand to
/// moss-build's own version number rather than the running binary's, and a
/// site owner reading their access log would see a version nobody ships.
/// `system::app_version()` is what both binaries seed at startup.
pub fn default_user_agent() -> String {
    format!("moss-import/{} (+https://moss.pub)", crate::system::app_version())
}

/// Configuration for one scrape invocation.
#[derive(Debug, Clone)]
pub struct ScrapeConfig {
    /// Starting URL to import.
    pub start_url: String,
    /// Output directory (also the import's site-root on disk).
    pub output_dir: PathBuf,
    /// If true, BFS-walk every in-scope URL reachable from `start_url`.
    /// If false (default), import only the start URL.
    pub recursive: bool,
    /// Cap on pages visited during a recursive walk. None disables the cap.
    pub max_pages: Option<usize>,
    /// User-Agent used for HTTP requests.
    pub user_agent: String,
}

impl ScrapeConfig {
    /// Sensible defaults for a single-URL import.
    pub fn new<S: Into<String>, P: Into<PathBuf>>(start_url: S, output_dir: P) -> Self {
        Self {
            start_url: start_url.into(),
            output_dir: output_dir.into(),
            recursive: false,
            max_pages: Some(200),
            user_agent: default_user_agent(),
        }
    }
}

/// Render YAML frontmatter for an imported page.
///
/// Keys are aligned with moss's `BUILTIN_FIELDS` schema (see
/// `crates/moss-core/src/schema_fields.rs`). Empty/None fields are omitted.
pub fn generate_frontmatter(metadata: &ArticleMetadata, source_url: &str) -> String {
    let mut out = String::from("---\n");

    push_string(&mut out, "title", metadata.title.as_deref());
    push_string(&mut out, "date", metadata.date.as_deref());
    push_string(&mut out, "author", metadata.author.as_deref());
    push_string(&mut out, "publisher", metadata.publisher.as_deref());
    push_string(&mut out, "lang", metadata.lang.as_deref());
    push_string(&mut out, "description", metadata.description.as_deref());
    push_string(&mut out, "cover", metadata.cover.as_deref());
    // An import is the user's own content republished here (POSSE): the vault
    // copy is canonical and `source_url` is a syndication mirror. Record it in
    // a `syndicated` list — the same field the comment renderer reads to link a
    // douban/matters comment back out to its origin (see
    // `build/features/comment/render.rs::syndicated_link_for_source`). A local
    // file with no known origin (`source_url` empty) has nothing to syndicate.
    let source = source_url.trim();
    if !source.is_empty() {
        out.push_str("syndicated:\n");
        out.push_str(&format!("  - {}\n", yaml_scalar(source)));
    }
    out.push_str("---\n\n");
    out
}

fn push_string(out: &mut String, key: &str, value: Option<&str>) {
    if let Some(v) = value {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            return;
        }
        // Strip stray C0/C1 control chars before escaping: this metadata comes
        // from scraped/imported HTML, which is untrusted, and the same
        // write-boundary defense-in-depth applied in
        // `moss_core::frontmatter::serialize` (macOS Tauri multiwebview
        // arrow-key bug, tauri-apps/tauri#10194) must hold here too, since
        // this hand-rolled writer bypasses that path entirely.
        let clean = moss_core::frontmatter::strip_control_chars_str(trimmed);
        // YAML double-quoted: escape \ and "
        let escaped = clean.replace('\\', "\\\\").replace('"', "\\\"");
        out.push_str(&format!("{}: \"{}\"\n", key, escaped));
    }
}

/// Render a value as a YAML scalar. A well-formed http(s) URL is a safe plain
/// scalar and is emitted bare (matching the vault's hand-authored
/// `syndicated:` lists); anything else — a malformed `source_url` that reached
/// us (e.g. a crafted MHTML `Snapshot-Content-Location` that `Url::parse`
/// rejected, so it flows through verbatim) — is double-quoted and escaped so a
/// `": "`, a leading indicator (`*`, `@`, …), or whitespace can't corrupt or
/// break the frontmatter.
fn yaml_scalar(value: &str) -> String {
    // Strip stray C0/C1 control chars first — same untrusted-input rationale
    // as `push_string` above (tauri-apps/tauri#10194): `value` here can be a
    // `source_url` derived from scraped/imported HTML.
    let value = &moss_core::frontmatter::strip_control_chars_str(value);
    let safe_plain = (value.starts_with("http://") || value.starts_with("https://"))
        && !value.contains(": ")
        && !value.chars().any(|c| c.is_whitespace());
    if safe_plain {
        value.to_string()
    } else {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// Rewrite in-scope links in markdown to point at the local mirror.
///
/// Only used when running in `--recursive` mode where the same scope contains
/// multiple pages on disk; for a single-URL import this is a no-op (no
/// in-scope links exist on disk).
pub fn rewrite_links(markdown: &str, scope: &UrlScope) -> String {
    LINK_PATTERN
        .replace_all(markdown, |caps: &regex::Captures| {
            let text = &caps[1];
            let url = &caps[2];
            if is_within_scope(scope, url) {
                if let Some(rel) = url_to_relative_md_path(url, scope) {
                    return format!("[{}]({})", text, rel);
                }
            }
            caps[0].to_string()
        })
        .to_string()
}

fn url_to_relative_md_path(url_str: &str, scope: &UrlScope) -> Option<String> {
    let url = Url::parse(url_str).ok()?;
    let path = url.path();
    let scope_path = scope.path_prefix();
    let relative = if scope_path == "/" {
        path.trim_start_matches('/')
    } else {
        path.strip_prefix(scope_path)?.trim_start_matches('/')
    };

    if relative.is_empty() {
        return Some("./index.md".to_string());
    }
    if relative.ends_with('/') {
        let dir = relative.trim_end_matches('/');
        return Some(format!("./{}/index.md", dir));
    }
    Some(format!("./{}.md", relative))
}

/// Body content used when a page failed to fetch.
pub fn generate_error_page(source_url: &str, error: &str) -> String {
    format!(
        "This page could not be imported.\n\n**Reason:** {}\n\n[View original page]({})",
        error, source_url
    )
}

/// Render the complete markdown file content (frontmatter + body) for a page
/// that could not be fetched.
///
/// Unlike [`generate_frontmatter`], this function does not require an
/// [`ArticleMetadata`] struct — error pages are not articles and should not
/// pretend to be one. The frontmatter contains only the minimal keys that are
/// meaningful for an error record: `title`, `external_url`, and `scrape_error`.
pub fn render_error_markdown(source_url: &str, error: &str) -> String {
    let escaped_url = source_url.replace('\\', "\\\\").replace('"', "\\\"");
    let escaped_error = error.replace('\\', "\\\\").replace('"', "\\\"");

    let frontmatter = format!(
        "---\ntitle: \"Page Unavailable\"\nexternal_url: \"{}\"\nscrape_error: \"{}\"\n---\n\n",
        escaped_url, escaped_error
    );
    let body = generate_error_page(source_url, error);
    format!("{}{}", frontmatter, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// The regex scan over a whole markdown body, which every recursive page
    /// goes through. Subsumes the two `url_to_relative_md_path` unit tests it
    /// replaced — a flat link, a nested one, and an out-of-scope one that must
    /// be left alone, all through the public door.
    fn rewrite_links_redirects_in_scope_and_leaves_the_rest() {
        let scope = UrlScope::new("https://example.com/blog/").unwrap();
        let markdown = "See [post 2](https://example.com/blog/post-2), \
                        [nested](https://example.com/blog/2024/post) and \
                        [external](https://other.com/page).";

        let rewritten = rewrite_links(markdown, &scope);

        assert!(rewritten.contains("[post 2](./post-2.md)"), "{rewritten}");
        assert!(rewritten.contains("[nested](./2024/post.md)"), "{rewritten}");
        assert!(
            rewritten.contains("[external](https://other.com/page)"),
            "an out-of-scope link must survive untouched: {rewritten}"
        );
    }

    #[test]
    fn test_generate_error_page() {
        let content = generate_error_page("https://example.com/page", "404 Not Found");
        assert!(content.contains("404 Not Found"));
        assert!(content.contains("https://example.com/page"));
    }

    #[test]
    fn frontmatter_emits_syndicated_not_external_url() {
        // moss import brings in the user's OWN content that was published
        // elsewhere (POSSE): the vault copy is canonical, the source URL is a
        // syndication mirror. So the source belongs in a `syndicated` list, not
        // the linkblog `external_url` field.
        let meta = ArticleMetadata {
            title: Some("笔记".into()),
            ..Default::default()
        };
        let fm = generate_frontmatter(&meta, "https://book.douban.com/review/8218385/");
        assert!(
            fm.contains("syndicated:\n  - https://book.douban.com/review/8218385/\n"),
            "expected a syndicated YAML list pointing at the source; got:\n{fm}"
        );
        assert!(
            !fm.contains("external_url"),
            "must not emit external_url for the user's own content; got:\n{fm}"
        );
    }

    #[test]
    fn frontmatter_syndicated_escapes_yaml_hostile_source() {
        // A malformed source_url (e.g. from a crafted MHTML Snapshot-Content-
        // Location that Url::parse rejected) must not corrupt the frontmatter:
        // a leading `*` is a YAML alias indicator and `": "` opens a mapping.
        let meta = ArticleMetadata {
            title: Some("t".into()),
            ..Default::default()
        };
        for hostile in ["*weird", "not a url: with colon", "https://x/#a: b"] {
            let fm = generate_frontmatter(&meta, hostile);
            let escaped = hostile.replace('\\', "\\\\").replace('"', "\\\"");
            assert!(
                fm.contains(&format!("  - \"{}\"\n", escaped)),
                "hostile source must be quoted; got:\n{fm}"
            );
            // And it must parse back to exactly the original string.
            let doc: serde_yaml::Value =
                serde_yaml::from_str(fm.trim_start_matches("---\n").trim_end_matches("---\n\n"))
                    .expect("frontmatter must be valid YAML");
            assert_eq!(
                doc["syndicated"][0].as_str(),
                Some(hostile),
                "syndicated[0] must round-trip to the original source"
            );
        }
    }

    #[test]
    fn frontmatter_omits_syndicated_when_no_source() {
        // A local file with no known origin URL (e.g. a plain .html snapshot)
        // has nothing to syndicate to — omit the key entirely.
        let meta = ArticleMetadata {
            title: Some("T".into()),
            ..Default::default()
        };
        let fm = generate_frontmatter(&meta, "");
        assert!(!fm.contains("syndicated"), "got:\n{fm}");
        assert!(!fm.contains("external_url"), "got:\n{fm}");
    }

    #[test]
    fn frontmatter_includes_all_set_fields() {
        let meta = ArticleMetadata {
            title: Some("Finding China's Voice".into()),
            description: Some("The new diaspora.".into()),
            author: Some("Yi Liu".into()),
            date: Some("2024-12-22".into()),
            publisher: Some("The Wire China".into()),
            lang: Some("en".into()),
            cover: Some("./assets/imported/abcd.jpg".into()),
            og_image: None,
        };
        let fm = generate_frontmatter(&meta, "https://x.example/post");
        assert!(fm.starts_with("---\n"));
        assert!(fm.contains("title: \"Finding China's Voice\""));
        assert!(fm.contains("date: \"2024-12-22\""));
        assert!(fm.contains("author: \"Yi Liu\""));
        assert!(fm.contains("publisher: \"The Wire China\""));
        assert!(fm.contains("lang: \"en\""));
        assert!(fm.contains("description: \"The new diaspora.\""));
        assert!(fm.contains("cover: \"./assets/imported/abcd.jpg\""));
        assert!(fm.contains("syndicated:\n  - https://x.example/post\n"));
    }

    #[test]
    fn frontmatter_omits_empty_fields() {
        let meta = ArticleMetadata {
            title: Some("Just A Title".into()),
            ..Default::default()
        };
        let fm = generate_frontmatter(&meta, "https://x.example/p");
        assert!(fm.contains("title: \"Just A Title\""));
        assert!(!fm.contains("author:"));
        assert!(!fm.contains("date:"));
    }

    #[test]
    fn frontmatter_quotes_are_escaped() {
        let meta = ArticleMetadata {
            title: Some(r#"Quote "marks""#.into()),
            ..Default::default()
        };
        let fm = generate_frontmatter(&meta, "https://x.example/p");
        assert!(fm.contains(r#"title: "Quote \"marks\"""#));
    }

    #[test]
    fn render_error_markdown_contains_url_and_reason() {
        let md = render_error_markdown("https://example.com/broken", "503 Service Unavailable");
        // Frontmatter must open correctly.
        assert!(md.starts_with("---\n"), "must start with YAML front-matter fence");
        // The source URL must appear in the frontmatter external_url field.
        assert!(
            md.contains("external_url: \"https://example.com/broken\""),
            "external_url must be present in frontmatter"
        );
        // The error reason must appear in the frontmatter scrape_error field.
        assert!(
            md.contains("scrape_error: \"503 Service Unavailable\""),
            "scrape_error must be present in frontmatter"
        );
        // The body must contain the human-readable reason.
        assert!(
            md.contains("503 Service Unavailable"),
            "body must include the error reason"
        );
        // The body must contain a link back to the original page.
        assert!(
            md.contains("https://example.com/broken"),
            "body must reference the source URL"
        );
    }

    #[test]
    fn render_error_markdown_does_not_use_article_metadata() {
        // The rendered content must NOT include article-only fields like
        // author:, date:, publisher:, or lang: — those would indicate that
        // an ArticleMetadata struct was fabricated and passed through
        // generate_frontmatter.
        let md = render_error_markdown("https://example.com/page", "Connection timeout");
        assert!(!md.contains("author:"), "error page must not emit author:");
        assert!(!md.contains("date:"), "error page must not emit date:");
        assert!(!md.contains("publisher:"), "error page must not emit publisher:");
        assert!(!md.contains("lang:"), "error page must not emit lang:");
        assert!(!md.contains("description:"), "error page must not emit description:");
        assert!(!md.contains("cover:"), "error page must not emit cover:");
    }

    #[test]
    fn render_error_markdown_escapes_quotes_in_error() {
        let md = render_error_markdown(
            "https://example.com/page",
            r#"Server said "try again""#,
        );
        assert!(
            md.contains(r#"scrape_error: "Server said \"try again\"""#),
            "double-quotes in error must be escaped in YAML"
        );
    }

    #[test]
    fn frontmatter_strips_stray_control_chars_from_untrusted_metadata() {
        // Regression test: metadata extracted from scraped/imported HTML is
        // untrusted and can carry stray C0 control chars (e.g. via the same
        // macOS Tauri multiwebview arrow-key bug this mirrors defense-in-depth
        // for, tauri-apps/tauri#10194, or simply malformed upstream markup).
        // `push_string` must strip them before they ever reach disk.
        let control = "\u{1D}".repeat(3);
        let meta = ArticleMetadata {
            title: Some(format!("x{control}")),
            description: Some(format!("x{control}")),
            ..Default::default()
        };
        let fm = generate_frontmatter(&meta, "");

        assert!(!fm.contains('\u{1D}'), "control chars must be stripped; got:\n{fm}");

        let doc: serde_yaml::Value =
            serde_yaml::from_str(fm.trim_start_matches("---\n").trim_end_matches("---\n\n"))
                .expect("frontmatter must be valid YAML");
        assert_eq!(doc["title"].as_str(), Some("x"));
        assert_eq!(doc["description"].as_str(), Some("x"));
    }

    #[test]
    fn yaml_scalar_strips_stray_control_chars() {
        // Same untrusted-input rationale as above, but for the `syndicated:`
        // list value, which goes through `yaml_scalar` rather than
        // `push_string`.
        let control = "\u{1D}".repeat(3);
        let hostile_source = format!("https://example.com/post{control}");
        let meta = ArticleMetadata {
            title: Some("t".into()),
            ..Default::default()
        };
        let fm = generate_frontmatter(&meta, &hostile_source);

        assert!(!fm.contains('\u{1D}'), "control chars must be stripped; got:\n{fm}");
        assert!(
            fm.contains("syndicated:\n  - https://example.com/post\n"),
            "got:\n{fm}"
        );
    }
}
