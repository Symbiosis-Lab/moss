//! Sitemap and robots.txt generation
//!
//! Generates sitemap.xml (Sitemap Protocol 0.9) and robots.txt from site pages.

use crate::build::scan::article_map::to_pretty_url;

/// A page entry for the sitemap.
pub struct SitemapEntry {
    /// Relative URL path (e.g., "posts/hello/index.html")
    pub url_path: String,
    /// Optional last-modified date in YYYY-MM-DD format
    pub lastmod: Option<String>,
}

/// Escape special XML characters in text content.
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Generates a sitemap.xml string from page entries.
///
/// # Arguments
/// * `entries` - Page entries with URL paths and optional dates
/// * `site_url` - Base URL of the site (e.g., "https://example.com")
pub fn generate_sitemap(entries: &[SitemapEntry], site_url: &str) -> String {
    let base = site_url.trim_end_matches('/');

    // Sort: root first, then alphabetical
    let mut sorted: Vec<&SitemapEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| {
        let a_is_root = a.url_path == "index.html";
        let b_is_root = b.url_path == "index.html";
        match (a_is_root, b_is_root) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.url_path.cmp(&b.url_path),
        }
    });

    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n");

    for entry in sorted {
        let pretty = to_pretty_url(&entry.url_path);
        // to_pretty_url("index.html") returns "index" (not empty)
        let full_url = if pretty == "index" {
            format!("{}/", base)
        } else {
            format!("{}/{}", base, pretty)
        };

        xml.push_str("<url>\n");
        xml.push_str(&format!("<loc>{}</loc>\n", escape_xml(&full_url)));
        if let Some(ref date) = entry.lastmod {
            xml.push_str(&format!("<lastmod>{}</lastmod>\n", escape_xml(date)));
        }
        xml.push_str("</url>\n");
    }

    xml.push_str("</urlset>");
    xml
}

/// The sitemap entries for `pages`, the served paths of the pages a build
/// produced. Leaves out a page whose document keeps it out of listings, and a
/// linkblog page (an absolute `external_url:`): its canonical home is
/// elsewhere on the web, and listing the local URL beside the canonical tag
/// would split crawler attention between two URLs claiming the same content
/// (moss#679).
pub(crate) fn entries_for_pages<'a>(
    pages: impl IntoIterator<Item = &'a String>,
    documents: &[crate::build::types::ParsedDocument],
) -> Vec<SitemapEntry> {
    let is_linkblog = |d: &crate::build::types::ParsedDocument| -> bool {
        crate::build::scan::page_map::external_url(&d.raw_frontmatter).is_some()
    };
    pages
        .into_iter()
        .filter(|k| !documents.iter().any(|d| d.url_path == **k && (!d.is_listable() || is_linkblog(d))))
        .map(|url_path| SitemapEntry {
            url_path: url_path.clone(),
            lastmod: documents.iter().find(|d| d.url_path == *url_path).and_then(|d| d.date.clone()),
        })
        .collect()
}

/// Generates a robots.txt string with an AI-policy preamble and a Sitemap directive.
///
/// # Arguments
/// * `site_url` - Base URL of the site (e.g., "https://example.com")
/// * `ai_policy` - Site-level AI crawler policy from `[site].ai_policy`. One of
///   `"standard"` (default), `"unrestricted"`, or `"restricted"`. `None` and any
///   unrecognized value fall back to the `"standard"` policy.
///
/// Policies:
/// - `"standard"` (default): allow search + AI-input, refuse AI training. Emits a
///   `Content-Signal` line plus explicit `Disallow: /` blocks for the known
///   training crawlers.
/// - `"unrestricted"`: allow everything, including AI training. No per-crawler blocks.
/// - `"restricted"`: refuse search, AI-input, and AI-training. `Disallow: /` for all.
pub fn generate_robots_txt(site_url: &str, ai_policy: Option<&str>) -> String {
    let base = site_url.trim_end_matches('/');
    // Build the sitemap URL via ServedPath so the path is canonically typed.
    let sitemap_rel = crate::build::served_path::ServedPath::for_sitemap().to_relative_url();

    // The "standard" policy block, shared by the default and the
    // unrecognized-value fallback. NOTE: the per-agent Disallow list is
    // best-effort and decays as new training crawlers appear — the declarative
    // `Content-Signal: ai-train=no` line is the crawler-agnostic contract; the
    // per-agent blocks are belt-and-suspenders for crawlers that honor robots
    // but not Content-Signal.
    let standard_block = || {
        let mut block = String::from(
            "User-agent: *\nContent-Signal: search=yes, ai-input=yes, ai-train=no\nAllow: /\n",
        );
        for agent in [
            "GPTBot",
            "Google-Extended",
            "ClaudeBot",
            "CCBot",
            "Bytespider",
            "meta-externalagent",
            "Amazonbot",
            "Applebot-Extended",
        ] {
            block.push_str(&format!("User-agent: {}\nDisallow: /\n", agent));
        }
        block
    };

    let policy_block = match ai_policy {
        Some("unrestricted") => {
            "User-agent: *\nContent-Signal: search=yes, ai-input=yes, ai-train=yes\nAllow: /\n"
                .to_string()
        }
        Some("restricted") => {
            "User-agent: *\nContent-Signal: search=no, ai-input=no, ai-train=no\nDisallow: /\n"
                .to_string()
        }
        Some("standard") | None => standard_block(),
        // Unrecognized value: fall back to the safe default, but warn loudly — a
        // typo like "unrestrcted" must not silently flip a site's AI policy.
        Some(other) => {
            log::warn!("Unknown [site].ai_policy {:?}; falling back to \"standard\"", other);
            standard_block()
        }
    };

    format!("{}\nSitemap: {}{}\n", policy_block, base, sitemap_rel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generates_valid_xml_structure() {
        let entries = vec![
            SitemapEntry {
                url_path: "posts/first-post/index.html".to_string(),
                lastmod: Some("2025-01-15".to_string()),
            },
        ];

        let sitemap = generate_sitemap(&entries, "https://example.com");

        assert!(sitemap.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(sitemap.contains("<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">"));
        assert!(sitemap.contains("</urlset>"));
    }

    #[test]
    fn test_includes_all_pages_with_correct_urls() {
        let entries = vec![
            SitemapEntry {
                url_path: "index.html".to_string(),
                lastmod: None,
            },
            SitemapEntry {
                url_path: "about.html".to_string(),
                lastmod: None,
            },
            SitemapEntry {
                url_path: "posts/hello/index.html".to_string(),
                lastmod: None,
            },
        ];

        let sitemap = generate_sitemap(&entries, "https://example.com");

        // Root index.html -> site root
        assert!(sitemap.contains("<loc>https://example.com/</loc>"));
        // about.html -> /about
        assert!(sitemap.contains("<loc>https://example.com/about</loc>"));
        // posts/hello/index.html -> /posts/hello/
        assert!(sitemap.contains("<loc>https://example.com/posts/hello/</loc>"));
    }

    #[test]
    fn test_lastmod_present_only_when_date_exists() {
        let entries = vec![
            SitemapEntry {
                url_path: "posts/dated/index.html".to_string(),
                lastmod: Some("2025-03-01".to_string()),
            },
            SitemapEntry {
                url_path: "about.html".to_string(),
                lastmod: None,
            },
        ];

        let sitemap = generate_sitemap(&entries, "https://example.com");

        // Dated page should have lastmod
        assert!(sitemap.contains("<lastmod>2025-03-01</lastmod>"));
        // Count lastmod occurrences — should be exactly 1
        let lastmod_count = sitemap.matches("<lastmod>").count();
        assert_eq!(lastmod_count, 1, "Only pages with dates should have <lastmod>");
    }

    #[test]
    fn test_escapes_xml_special_characters_in_urls() {
        let entries = vec![
            SitemapEntry {
                url_path: "page&special/index.html".to_string(),
                lastmod: None,
            },
        ];

        let sitemap = generate_sitemap(&entries, "https://example.com");

        assert!(sitemap.contains("&amp;special"));
        assert!(!sitemap.contains("&special"));
    }

    #[test]
    fn test_empty_entries_produces_valid_sitemap() {
        let entries = vec![];
        let sitemap = generate_sitemap(&entries, "https://example.com");

        assert!(sitemap.contains("<urlset"));
        assert!(sitemap.contains("</urlset>"));
        assert!(!sitemap.contains("<url>"));
    }

    #[test]
    fn test_trailing_slash_in_site_url_handled() {
        let entries = vec![
            SitemapEntry {
                url_path: "about.html".to_string(),
                lastmod: None,
            },
        ];

        let sitemap = generate_sitemap(&entries, "https://example.com/");

        // Should not produce double slashes
        assert!(sitemap.contains("<loc>https://example.com/about</loc>"));
        assert!(!sitemap.contains("example.com//"));
    }

    #[test]
    fn test_entries_sorted_deterministically() {
        let entries = vec![
            SitemapEntry {
                url_path: "z-page.html".to_string(),
                lastmod: None,
            },
            SitemapEntry {
                url_path: "index.html".to_string(),
                lastmod: None,
            },
            SitemapEntry {
                url_path: "a-page.html".to_string(),
                lastmod: None,
            },
        ];

        let sitemap = generate_sitemap(&entries, "https://example.com");

        // Root (index.html) should come first, then alphabetical
        let root_pos = sitemap.find("https://example.com/</loc>").unwrap();
        let a_pos = sitemap.find("https://example.com/a-page</loc>").unwrap();
        let z_pos = sitemap.find("https://example.com/z-page</loc>").unwrap();
        assert!(root_pos < a_pos, "Root should come before a-page");
        assert!(a_pos < z_pos, "a-page should come before z-page");
    }

    #[test]
    fn test_robots_txt_contains_sitemap_reference() {
        let robots = generate_robots_txt("https://example.com", None);

        assert!(robots.contains("User-agent: *"));
        assert!(robots.contains("Allow: /"));
        assert!(robots.contains("Sitemap: https://example.com/sitemap.xml"));
    }

    #[test]
    fn test_robots_txt_trailing_slash_handling() {
        let robots = generate_robots_txt("https://example.com/", None);

        assert!(robots.contains("Sitemap: https://example.com/sitemap.xml"));
        assert!(!robots.contains("example.com//"));
    }

    /// The DEFAULT / "standard" policy: search + AI-input allowed, AI-training
    /// refused via a Content-Signal line plus explicit Disallow blocks for the
    /// eight known training crawlers, followed by the Sitemap directive.
    const EXPECTED_STANDARD: &str = "\
User-agent: *
Content-Signal: search=yes, ai-input=yes, ai-train=no
Allow: /
User-agent: GPTBot
Disallow: /
User-agent: Google-Extended
Disallow: /
User-agent: ClaudeBot
Disallow: /
User-agent: CCBot
Disallow: /
User-agent: Bytespider
Disallow: /
User-agent: meta-externalagent
Disallow: /
User-agent: Amazonbot
Disallow: /
User-agent: Applebot-Extended
Disallow: /

Sitemap: https://example.com/sitemap.xml
";

    #[test]
    fn test_robots_txt_default_policy_byte_for_byte() {
        let robots = generate_robots_txt("https://example.com", Some("standard"));
        assert_eq!(robots, EXPECTED_STANDARD);
    }

    #[test]
    fn test_robots_txt_none_equals_standard() {
        let from_none = generate_robots_txt("https://example.com", None);
        let from_standard = generate_robots_txt("https://example.com", Some("standard"));
        assert_eq!(from_none, from_standard);
        assert_eq!(from_none, EXPECTED_STANDARD);
    }

    #[test]
    fn test_robots_txt_unrestricted_policy_byte_for_byte() {
        let expected = "\
User-agent: *
Content-Signal: search=yes, ai-input=yes, ai-train=yes
Allow: /

Sitemap: https://example.com/sitemap.xml
";
        let robots = generate_robots_txt("https://example.com", Some("unrestricted"));
        assert_eq!(robots, expected);
        // No per-crawler training blocks in the unrestricted policy.
        assert!(!robots.contains("GPTBot"));
        assert!(!robots.contains("Disallow: /"));
        assert!(robots.contains("ai-train=yes"));
    }

    #[test]
    fn test_robots_txt_restricted_policy_byte_for_byte() {
        let expected = "\
User-agent: *
Content-Signal: search=no, ai-input=no, ai-train=no
Disallow: /

Sitemap: https://example.com/sitemap.xml
";
        let robots = generate_robots_txt("https://example.com", Some("restricted"));
        assert_eq!(robots, expected);
        assert!(robots.contains("Disallow: /"));
        assert!(!robots.contains("Allow: /"));
    }

    #[test]
    fn test_robots_txt_unrecognized_policy_falls_back_to_standard() {
        let robots = generate_robots_txt("https://example.com", Some("bogus"));
        assert_eq!(robots, EXPECTED_STANDARD);
    }

    #[test]
    fn test_robots_txt_default_trailing_slash_handling() {
        // The new default block must still avoid double slashes in the Sitemap URL.
        let robots = generate_robots_txt("https://example.com/", None);
        assert!(robots.contains("Sitemap: https://example.com/sitemap.xml"));
        assert!(!robots.contains("example.com//"));
    }
}
