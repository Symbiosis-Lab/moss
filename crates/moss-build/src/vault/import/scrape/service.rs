//! Scrape configuration and frontmatter rendering.
//!
//! [`ScrapeConfig`] is the single shared config struct passed into
//! [`super::run::scrape_to_folder`] from both the Tauri command and the
//! CLI. [`imported_frontmatter`] types an [`super::metadata::ArticleMetadata`]
//! + source URL as the fields at the head of each imported `.md`.

use std::path::PathBuf;
use std::sync::LazyLock;

use regex::Regex;

use super::metadata::ArticleMetadata;
use super::scope::UrlScope;

// `[text](url)` or `[text](url "title")` markdown link, captured once and reused
// across pages. Group 3 is the title with its leading space, in any of the
// three CommonMark forms: `"t"`, `'t'` or `(t)`.
static LINK_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\[((?:[^\]\[]|!\[[^\]]*\]\([^)]*\))*)\]\(([^)\s]+)(\s+(?:"[^"]*"|'[^']*'|\([^)]*\)))?\s*\)"#)
        .expect("link regex")
});

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

/// [`ScrapeConfig::new`]'s default `max_pages` — the recursive-crawl page
/// cap. Named so the CLI's `--help` text and progress summary can cite the
/// same number the config actually enforces, rather than a second copy of
/// the literal that could drift from it.
pub const DEFAULT_MAX_PAGES: usize = 200;

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
    /// Largest single file a page's link may pull into the site.
    pub(crate) linked_file_max_bytes: u64,
    /// A source's menu (a WordPress export's), the nav bar when the home
    /// page shows none.
    pub(crate) menu: Vec<super::design::NavEntry>,
}

impl ScrapeConfig {
    /// Sensible defaults for a single-URL import.
    pub fn new<S: Into<String>, P: Into<PathBuf>>(start_url: S, output_dir: P) -> Self {
        Self {
            start_url: start_url.into(),
            output_dir: output_dir.into(),
            recursive: false,
            max_pages: Some(DEFAULT_MAX_PAGES),
            user_agent: default_user_agent(),
            linked_file_max_bytes: super::fetch::LINKED_FILE_MAX_BYTES,
            menu: Vec::new(),
        }
    }
}

/// The typed fields of an imported page; empty values are omitted.
pub(crate) fn imported_frontmatter(metadata: &ArticleMetadata, source_url: &str) -> moss_core::frontmatter_typed::FrontMatter {
    let clean = |s: Option<&str>| s.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    moss_core::frontmatter_typed::FrontMatter {
        title: clean(metadata.title.as_deref()),
        date: clean(metadata.date.as_deref()),
        author: clean(metadata.author.as_deref()).map(|s| vec![s]),
        publisher: clean(metadata.publisher.as_deref()),
        lang: clean(metadata.lang.as_deref()),
        description: clean(metadata.description.as_deref()),
        cover: clean(metadata.cover.as_deref()),
        tags: (!metadata.tags.is_empty()).then(|| metadata.tags.clone()),
        start: clean(metadata.event.start.as_deref()),
        end: clean(metadata.event.end.as_deref()),
        location: clean(metadata.event.location.as_deref()).map(|s| vec![s]),
        status: clean(metadata.event.status.as_deref()),
        tickets: clean(metadata.event.tickets.as_deref()),
        online: clean(metadata.event.online.as_deref()),
        origin: clean(Some(source_url)),
        ..Default::default()
    }
}

/// `url` without a `ref` query parameter whose value is the imported site's
/// own host (with or without `www.`): some platforms tag every outbound link
/// that way, and it names a host that means nothing after a port. Other
/// parameters and the fragment stay as written.
pub(crate) fn strip_outbound_ref(url: &str, site_host: &str) -> String {
    let (without_fragment, fragment) = match url.split_once('#') {
        Some((u, f)) => (u, Some(f)),
        None => (url, None),
    };
    let Some((base, query)) = without_fragment.split_once('?') else { return url.to_string() };
    let own = |pair: &&str| {
        pair.split_once('=').is_some_and(|(k, v)| k == "ref" && super::scope::bare_host(v) == site_host)
    };
    let kept: Vec<&str> = query.split('&').filter(|p| !own(p)).collect();
    let query = if kept.is_empty() { String::new() } else { format!("?{}", kept.join("&")) };
    let fragment = fragment.map(|f| format!("#{f}")).unwrap_or_default();
    format!("{base}{query}{fragment}")
}

/// [`strip_outbound_ref`] over every markdown link destination.
pub(crate) fn strip_outbound_refs(markdown: &str, scope: &UrlScope) -> String {
    let site = scope.bare_host();
    LINK_PATTERN
        .replace_all(markdown, |caps: &regex::Captures| {
            let title = caps.get(3).map_or("", |m| m.as_str());
            format!("[{}]({}{})", &caps[1], strip_outbound_ref(&caps[2], &site), title)
        })
        .to_string()
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
/// Unlike [`imported_frontmatter`], this function does not require an
/// [`ArticleMetadata`] struct — error pages are not articles and should not
/// pretend to be one. The frontmatter contains only the minimal keys that are
/// meaningful for an error record: `title`, `external_url`, `scrape_error`, and
/// `listed: false`, so the stub stays out of navigation and listings.
pub fn render_error_markdown(source_url: &str, error: &str) -> String {
    use crate::vault::import::page::{render_frontmatter_with, PageStyle};
    let fields = moss_core::frontmatter_typed::FrontMatter {
        title: Some("Page Unavailable".into()),
        external_url: Some(source_url.into()),
        listed: Some(false),
        ..Default::default()
    };
    let frontmatter = render_frontmatter_with(&fields, &[("scrape_error", error)], PageStyle::Html);
    format!("{frontmatter}{}", generate_error_page(source_url, error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::metadata::derive;

    fn generate_frontmatter(metadata: &ArticleMetadata, source_url: &str) -> String {
        crate::vault::import::page::render_frontmatter(&imported_frontmatter(metadata, source_url),
            crate::vault::import::page::PageStyle::Html)
    }

    #[test]
    fn test_generate_error_page() {
        let content = generate_error_page("https://example.com/page", "404 Not Found");
        assert!(content.contains("404 Not Found"));
        assert!(content.contains("https://example.com/page"));
    }

    #[test]
    fn frontmatter_emits_origin_not_external_url() {
        // moss import records where a page came FROM as provenance, not as a
        // syndication claim: the vault copy is canonical, and the source URL
        // is not asserted to also be the content's home (that is what the
        // linkblog `external_url` field means).
        let meta = ArticleMetadata {
            title: Some("笔记".into()),
            ..Default::default()
        };
        let fm = generate_frontmatter(&meta, "https://book.douban.com/review/8218385/");
        assert!(
            fm.contains("origin: \"https://book.douban.com/review/8218385/\"\n"),
            "expected an origin field pointing at the source; got:\n{fm}"
        );
        assert!(
            !fm.contains("syndicated"),
            "import must not write the POSSE syndicated field; got:\n{fm}"
        );
        assert!(
            !fm.contains("external_url"),
            "must not emit external_url for the user's own content; got:\n{fm}"
        );
    }

    #[test]
    fn frontmatter_origin_escapes_yaml_hostile_source() {
        // A malformed source_url (e.g. from a crafted MHTML Snapshot-Content-
        // Location that Url::parse rejected) must not corrupt the frontmatter:
        // a leading `*` is a YAML alias indicator and `": "` opens a mapping.
        // `push_string` always double-quotes, so this is the same guarantee
        // every other field gets — no field-specific bare-scalar case to keep
        // safe.
        let meta = ArticleMetadata {
            title: Some("t".into()),
            ..Default::default()
        };
        for hostile in ["*weird", "not a url: with colon", "https://x/#a: b"] {
            let fm = generate_frontmatter(&meta, hostile);
            let escaped = hostile.replace('\\', "\\\\").replace('"', "\\\"");
            assert!(
                fm.contains(&format!("origin: \"{}\"\n", escaped)),
                "hostile source must be quoted; got:\n{fm}"
            );
            // And it must parse back to exactly the original string.
            let doc: serde_yaml::Value =
                serde_yaml::from_str(fm.trim_start_matches("---\n").trim_end_matches("---\n\n"))
                    .expect("frontmatter must be valid YAML");
            assert_eq!(
                doc["origin"].as_str(),
                Some(hostile),
                "origin must round-trip to the original source"
            );
        }
    }

    #[test]
    fn frontmatter_omits_origin_when_no_source() {
        // A local file with no known source URL (e.g. a plain .html snapshot)
        // has no origin to record — omit the key entirely.
        let meta = ArticleMetadata {
            title: Some("T".into()),
            ..Default::default()
        };
        let fm = generate_frontmatter(&meta, "");
        assert!(!fm.contains("origin"), "got:\n{fm}");
        assert!(!fm.contains("syndicated"), "got:\n{fm}");
        assert!(!fm.contains("external_url"), "got:\n{fm}");
    }

    #[test]
    fn frontmatter_includes_all_set_fields() {
        let meta = ArticleMetadata {
            title: Some("The Reporter's Notebook".into()),
            description: Some("The new diaspora.".into()),
            author: Some("Jane Doe".into()),
            date: Some("2024-12-22".into()),
            publisher: Some("Example Times".into()),
            lang: Some("en".into()),
            cover: Some("./assets/imported/abcd.jpg".into()),
            og_image: None,
            chrome_images: Vec::new(),
            tags: Vec::new(),
            event: Default::default(),
            listing: false,
        };
        let fm = generate_frontmatter(&meta, "https://x.example/post");
        assert!(fm.starts_with("---\n"));
        assert!(fm.contains("title: \"The Reporter's Notebook\""));
        assert!(fm.contains("date: \"2024-12-22\""));
        assert!(fm.contains("author: \"Jane Doe\""));
        assert!(fm.contains("publisher: \"Example Times\""));
        assert!(fm.contains("lang: \"en\""));
        assert!(fm.contains("description: \"The new diaspora.\""));
        assert!(fm.contains("cover: \"./assets/imported/abcd.jpg\""));
        assert!(fm.contains("origin: \"https://x.example/post\"\n"));
    }

    #[test]
    fn article_tag_metas_become_a_tags_list() {
        let html = r#"<html><head><title>Example Field Notes</title>
            <meta property="article:tag" content="Field notes">
            <meta property="article:tag" content="Say &quot;hi&quot;: a #b">
            <meta property="article:tag" content="Tea &amp; ceremony">
            <meta property="article:tag" content="Field notes">
            <meta property="article:tag" content="  ">
            </head><body><p>Hello</p></body></html>"#;
        let fm = generate_frontmatter(&derive(html), "https://example.test/notes/first/");
        assert!(
            fm.contains("tags: [\"Field notes\", \"Say \\\"hi\\\": a #b\", \"Tea & ceremony\"]\n"),
            "{fm}"
        );
        let yaml = fm.trim_start_matches("---\n").split("---").next().unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(parsed["tags"][1].as_str(), Some("Say \"hi\": a #b"));

        let plain = "<html><head><title>Example Field Notes</title></head><body></body></html>";
        let fm = generate_frontmatter(&derive(plain), "https://example.test/notes/first/");
        assert!(!fm.contains("tags"), "{fm}");
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
    fn render_error_markdown_strips_control_chars_in_error() {
        // Regression: `error` is remote server text (a status line, a reason
        // phrase) and must get the same C0/C1 strip `push_string` applies to
        // scraped metadata — not just the quote/backslash escape. Before this
        // fix the two writers shared the escape but not the strip.
        let control = "\u{7}".repeat(3); // BEL, a C0 control char
        let md = render_error_markdown(
            "https://example.com/page",
            &format!("Connection reset{control}"),
        );
        let frontmatter_end = md.find("---\n\n").expect("frontmatter fence");
        let frontmatter = &md[..frontmatter_end];
        assert!(
            !frontmatter.contains('\u{7}'),
            "control chars must be stripped from frontmatter; got:\n{md}"
        );
        let doc: serde_yaml::Value =
            serde_yaml::from_str(frontmatter.trim_start_matches("---\n"))
                .expect("frontmatter must be valid YAML");
        assert_eq!(doc["scrape_error"].as_str(), Some("Connection reset"));
    }

    #[test]
    fn render_error_markdown_escapes_embedded_newline_in_error() {
        // Regression: the frontmatter boundary finder
        // (`moss_core::frontmatter::yaml_span`) locates the closing fence by
        // scanning line-by-line for a line that trims to exactly "---" — it
        // has no notion of YAML quoting. Before this fix, a raw `\n` reached
        // the written file (the control-char strip explicitly keeps
        // LF/CR/TAB as "legitimate whitespace"), so an `error` string
        // containing "\n---\n" forged a closing fence from inside what was
        // meant to be one quoted scalar. That truncated the real
        // frontmatter early — `title` and `external_url` were lost
        // entirely — and spilled the rest of the (half-escaped) error text
        // into the page body as literal text, `\"` backslashes and all.
        let malicious = "Connection reset\n---\ntitle: \"INJECTED\"\nsneaky: \"yes\"";
        let md = render_error_markdown("https://example.com/page", malicious);
        let doc = moss_core::frontmatter::parse(&md);
        assert_eq!(
            doc.frontmatter.get("title").and_then(|v| v.as_str()),
            Some("Page Unavailable"),
            "a forged '---' fence inside the error text must not truncate \
             the real frontmatter; got:\n{md}"
        );
        assert_eq!(
            doc.frontmatter.get("external_url").and_then(|v| v.as_str()),
            Some("https://example.com/page")
        );
        assert_eq!(
            doc.frontmatter.get("scrape_error").and_then(|v| v.as_str()),
            Some(malicious),
            "the error text must round-trip exactly, as one scalar"
        );
        assert_eq!(
            doc.frontmatter.len(),
            4,
            "no extra key (e.g. the forged 'sneaky') may land in frontmatter; got:\n{:#?}",
            doc.frontmatter
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
    fn origin_strips_stray_control_chars() {
        // Same untrusted-input rationale as above, but for the `origin`
        // value derived from `source_url`.
        let control = "\u{1D}".repeat(3);
        let hostile_source = format!("https://example.com/post{control}");
        let meta = ArticleMetadata {
            title: Some("t".into()),
            ..Default::default()
        };
        let fm = generate_frontmatter(&meta, &hostile_source);

        assert!(!fm.contains('\u{1D}'), "control chars must be stripped; got:\n{fm}");
        assert!(
            fm.contains("origin: \"https://example.com/post\"\n"),
            "got:\n{fm}"
        );
    }

    #[test]
    fn failed_page_stub_is_unlisted_and_a_normal_page_is_not() {
        let stub = render_error_markdown("https://example.com/gone", "404 Not Found");
        assert!(stub.contains("\nlisted: false\n"), "stub must be unlisted; got:\n{stub}");

        let meta = ArticleMetadata { title: Some("Hello".into()), ..Default::default() };
        let page = generate_frontmatter(&meta, "https://example.com/hello");
        assert!(!page.contains("listed"), "a normal page stays listed; got:\n{page}");
    }

    #[test]
    fn generate_frontmatter_writes_event_keys_and_no_date_for_an_event_only_page() {
        let html = r#"<html><head><script type="application/ld+json">
          {"@type":"Event","name":"Example Night","startDate":"2026-11-01T19:30:00-04:00",
           "endDate":"2026-11-01T21:00:00-04:00","eventStatus":"https://schema.org/EventPostponed",
           "offers":{"url":"https://tickets.example/night"},
           "location":[{"@type":"Place","name":"Example Hall"},
                       {"@type":"VirtualLocation","url":"https://stream.example/night"}]}
          </script></head><body></body></html>"#;
        let fm = generate_frontmatter(&super::super::metadata::derive(html), "https://example.com/night");
        assert!(!fm.contains("\ndate:"), "an event never writes date; got:\n{fm}");
        let at = |k: &str| fm.find(&format!("\n{k}: ")).unwrap_or_else(|| panic!("{k} missing in:\n{fm}"));
        assert!(fm.contains("\nstart: \"2026-11-01 19:30\"\n"), "got:\n{fm}");
        assert!(fm.contains("\nend: \"2026-11-01 21:00\"\n"), "got:\n{fm}");
        assert!(fm.contains("\nlocation: \"Example Hall\"\n"), "got:\n{fm}");
        assert!(fm.contains("\nstatus: \"postponed\"\n"), "got:\n{fm}");
        assert!(fm.contains("\ntickets: \"https://tickets.example/night\"\n"), "got:\n{fm}");
        assert!(fm.contains("\nonline: \"https://stream.example/night\"\n"), "got:\n{fm}");
        let keys = ["start", "end", "location", "status", "tickets", "online"];
        assert!(keys.windows(2).all(|w| at(w[0]) < at(w[1])), "key order; got:\n{fm}");
    }
}
