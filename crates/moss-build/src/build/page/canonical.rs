//! Compute canonical URLs per the moss canonical-www-prefix policy.
//!
//! A site canonicalizes to `www.{host}` once its CDN hostname is serving, and
//! to the bare apex until then — one rule whether the domain was bought
//! through moss or brought from another registrar (see
//! `build::site_url::ResolveInputs::cdn_active`). This determines both
//! `og:url` and the `<link rel="canonical">` href so scrapers and search
//! engines agree on a single URL per page.
//!
//! See the canonical-www-prefix architecture decision in moss-seta for the
//! full rationale (CDN portability, apex/email separation).

use crate::build::served_path::ServedPath;
use crate::build::site_url::SiteUrl;

/// Returns the canonical URL for a given page on a given site host.
///
/// * `host` — bare hostname, e.g. `"example.org"` or `"www.example.org"`. Strip
///   scheme and path before passing in.
/// * `path` — typed served path for the page. Built from the page's url_path
///   (e.g. `"about/index.html"`). The function strips the trailing
///   `index.html` so directory pages canonicalize with a trailing slash.
/// * `www_canonical` — when true, the canonical form gets a `www.` prefix
///   unless the host already has one. True exactly when the domain's CDN
///   hostname is active; pass `false` to keep the bare apex.
///
/// Homepage canonical (`https://host/`) is built directly at the call site
/// since `ServedPath` rejects empty inputs at parse time; this function only
/// handles non-empty page paths.
pub fn canonical_url(host: &str, path: &ServedPath, www_canonical: bool) -> String {
    canonical_url_on("https", host, path, www_canonical)
}

/// [`canonical_url`], for a site whose scheme is known. See
/// [`crate::build::site_url::SiteUrl::scheme`] for why that is a question.
fn canonical_url_on(scheme: &str, host: &str, path: &ServedPath, www_canonical: bool) -> String {
    let prefix = if www_canonical && !host.starts_with("www.") {
        format!("www.{}", host)
    } else {
        host.to_string()
    };
    let relative = path.to_relative_url();
    // Strip trailing index.html so directory pages canonicalize as `/about/`
    // not `/about/index.html`. Non-index pages (`/posts/foo.html`) are untouched.
    let pretty = relative.trim_end_matches("index.html");
    format!("{}://{}{}", scheme, prefix, pretty)
}

/// The canonical URL for a page, addressed the way the rest of the build
/// addresses pages: by `url_path` (`"about/index.html"`, `"index.html"`).
///
/// One page has one canonical URL, and more than one thing needs it — the
/// `<link rel="canonical">` in the head and the QR code on the share card. They
/// were computing it separately, and the QR's version was the raw
/// `{site}/{url_path}`, so every code on every site encoded
/// `https://example.com/posts/a/index.html` while the page next to it declared
/// `https://example.com/posts/a/`. Same page, two addresses, and the longer one
/// is the one that got printed at 56pt: `/index.html` costs eleven characters,
/// which is enough to push a code up a QR version and cost it scan reliability
/// at card size.
///
/// The homepage is the case that forces this to be a function rather than a
/// call to [`canonical_url`]: `ServedPath` rejects an empty path at parse time,
/// so `index.html` has to be recognised before the typed path is built.
///
/// It takes the whole [`SiteUrl`] rather than a host because the scheme is part
/// of the answer: an onion site is served over plain http, and a code encoding
/// `https://<addr>.onion/` is one Tor Browser cannot open.
pub fn canonical_for_url_path(
    site_url: &SiteUrl,
    url_path: &str,
) -> Result<String, crate::build::served_path::ServedPathError> {
    let trimmed = url_path.trim_start_matches('/');
    if trimmed.is_empty() || trimmed == "index.html" {
        return Ok(format!("{}://{}/", site_url.scheme(), site_url.host()));
    }
    let path = ServedPath::from_source(trimmed)?;
    Ok(canonical_url_on(site_url.scheme(), site_url.host(), &path, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(url: &str) -> SiteUrl {
        SiteUrl::parse(url).unwrap()
    }

    #[test]
    fn url_path_form_agrees_with_the_typed_form() {
        let path = ServedPath::from_source("about/index.html").unwrap();
        assert_eq!(
            canonical_for_url_path(&site("https://example.com"), "about/index.html").unwrap(),
            canonical_url("example.com", &path, false)
        );
    }

    #[test]
    fn the_homepage_canonicalizes_to_the_bare_host() {
        // ServedPath rejects "", so this arm cannot go through canonical_url.
        assert_eq!(
            canonical_for_url_path(&site("https://example.com"), "index.html").unwrap(),
            "https://example.com/"
        );
        assert_eq!(
            canonical_for_url_path(&site("https://example.com"), "").unwrap(),
            "https://example.com/"
        );
    }

    #[test]
    fn a_directory_page_canonicalizes_with_a_slash_not_index_html() {
        // The share card's QR encodes this string. `/index.html` was eleven
        // wasted characters in every code moss has ever printed.
        assert_eq!(
            canonical_for_url_path(&site("https://example.com"), "posts/a/index.html").unwrap(),
            "https://example.com/posts/a/"
        );
    }

    #[test]
    fn an_onion_site_keeps_its_scheme() {
        // Onion services are plain http. A QR encoding `https://…onion/` is a
        // code that scans cleanly and then fails to load.
        let onion = site("http://abcdefghij.onion");
        assert_eq!(
            canonical_for_url_path(&onion, "posts/a/index.html").unwrap(),
            "http://abcdefghij.onion/posts/a/"
        );
        assert_eq!(
            canonical_for_url_path(&onion, "index.html").unwrap(),
            "http://abcdefghij.onion/"
        );
    }

    #[test]
    fn www_canonical_gets_www_prefix() {
        let path = ServedPath::from_source("about/index.html").unwrap();
        assert_eq!(canonical_url("example.org", &path, true), "https://www.example.org/about/");
    }

    #[test]
    fn already_www_is_left_alone() {
        let path = ServedPath::from_source("about/index.html").unwrap();
        assert_eq!(
            canonical_url("www.example.org", &path, true),
            "https://www.example.org/about/"
        );
    }

    #[test]
    fn stays_bare_until_www_is_canonical() {
        let path = ServedPath::from_source("page/index.html").unwrap();
        assert_eq!(
            canonical_url("example.com", &path, false),
            "https://example.com/page/"
        );
    }

    #[test]
    fn non_index_html_page_preserves_extension() {
        let path = ServedPath::from_source("about.html").unwrap();
        assert_eq!(
            canonical_url("example.com", &path, false),
            "https://example.com/about.html"
        );
    }
}
