//! sitemap.xml (Sitemap Protocol 0.9) from the site's pages. robots.txt is
//! `robots.rs`.

/// A page entry for the sitemap.
pub struct SitemapEntry {
    /// Relative URL path (e.g., "posts/hello/index.html")
    pub url_path: String,
    /// Optional last-modified date in YYYY-MM-DD format
    pub lastmod: Option<String>,
}

/// Whether a raw `.html` file's head carries a `<meta name="robots">` or
/// `<meta name="googlebot">` with a `noindex` or `none` token. Read until the
/// body starts, at most 4 MiB; past that, or on a read error, the page counts
/// as noindex, so a tag that was not seen never publishes a hidden address.
pub(crate) fn declares_noindex(path: &std::path::Path) -> bool {
    use lol_html::{element, HtmlRewriter, Settings};
    use std::io::Read;

    const LIMIT: usize = 4 << 20;
    let Ok(mut file) = std::fs::File::open(path) else { return false };
    let found = std::cell::Cell::new(false);
    let head_over = std::cell::Cell::new(false);
    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![
                element!("meta[name]", |el| {
                    let for_crawlers = el
                        .get_attribute("name")
                        .is_some_and(|n| n.eq_ignore_ascii_case("robots") || n.eq_ignore_ascii_case("googlebot"));
                    let blocks = el.get_attribute("content").is_some_and(|c| {
                        c.split(|ch: char| ch == ',' || ch.is_whitespace())
                            .any(|t| t.eq_ignore_ascii_case("noindex") || t.eq_ignore_ascii_case("none"))
                    });
                    if for_crawlers && blocks && !head_over.get() {
                        found.set(true);
                    }
                    Ok(())
                }),
                element!("body", |_| {
                    head_over.set(true);
                    Ok(())
                }),
            ],
            ..Settings::default()
        },
        |_: &[u8]| {},
    );
    let (mut buf, mut read) = ([0u8; 16384], 0);
    while !found.get() && !head_over.get() {
        if read >= LIMIT {
            return true;
        }
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                read += n;
                if rewriter.write(&buf[..n]).is_err() {
                    return true;
                }
            }
            Err(_) => return true,
        }
    }
    found.get()
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
        // The host serves `x/index.html` at `x/` but a static `foo.html` only at
        // `foo.html`, so the extension stays on every path that is not a directory index.
        let full_url = match entry.url_path.strip_suffix("index.html") {
            Some("") => format!("{}/", base),
            Some(dir) if dir.ends_with('/') => format!("{}/{}", base, dir),
            _ => format!("{}/{}", base, entry.url_path),
        };

        xml.push_str("<url>\n");
        xml.push_str(&format!("<loc>{}</loc>\n", moss_core::media::xml_escape(&full_url)));
        if let Some(ref date) = entry.lastmod {
            xml.push_str(&format!("<lastmod>{}</lastmod>\n", moss_core::media::xml_escape(date)));
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
/// would split crawler attention between two URLs claiming the same content.
pub(crate) fn entries_for_pages<'a>(
    pages: impl IntoIterator<Item = &'a String>,
    documents: &[crate::build::types::ParsedDocument],
) -> Vec<SitemapEntry> {
    let is_linkblog = |d: &crate::build::types::ParsedDocument| -> bool {
        crate::build::scan::page_map::external_url(&d.raw_frontmatter).is_some()
    };
    pages
        .into_iter()
        .filter(|k| !crate::build::served_path::is_not_found_address(k))
        .filter(|k| !documents.iter().any(|d| d.url_path == **k && (!d.is_listable() || is_linkblog(d))))
        .map(|url_path| SitemapEntry {
            url_path: url_path.clone(),
            lastmod: documents.iter().find(|d| d.url_path == *url_path).and_then(|d| d.date.clone()),
        })
        .collect()
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
        // about.html -> /about.html (the host serves no extensionless address)
        assert!(sitemap.contains("<loc>https://example.com/about.html</loc>"));
        // posts/hello/index.html -> /posts/hello/
        assert!(sitemap.contains("<loc>https://example.com/posts/hello/</loc>"));
    }

    #[test]
    fn test_static_html_page_keeps_extension_in_sitemap() {
        let entries = vec![
            SitemapEntry {
                url_path: "index.html".to_string(),
                lastmod: None,
            },
            SitemapEntry {
                url_path: "docs/index.html".to_string(),
                lastmod: None,
            },
            SitemapEntry {
                url_path: "ui/editor.html".to_string(),
                lastmod: None,
            },
        ];

        let sitemap = generate_sitemap(&entries, "https://example.com");

        assert!(sitemap.contains("<loc>https://example.com/</loc>"));
        assert!(sitemap.contains("<loc>https://example.com/docs/</loc>"));
        assert!(sitemap.contains("<loc>https://example.com/ui/editor.html</loc>"));
        assert!(!sitemap.contains("<loc>https://example.com/ui/editor</loc>"));
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
        assert!(sitemap.contains("<loc>https://example.com/about.html</loc>"));
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
        let a_pos = sitemap.find("https://example.com/a-page.html</loc>").unwrap();
        let z_pos = sitemap.find("https://example.com/z-page.html</loc>").unwrap();
        assert!(root_pos < a_pos, "Root should come before a-page");
        assert!(a_pos < z_pos, "a-page should come before z-page");
    }
}
