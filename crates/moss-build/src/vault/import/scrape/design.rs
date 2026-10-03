//! A site's design decisions, read from its home page and written beside the
//! pages the crawl produced.
//!
//! A page's words travel through [`super::converter`]; a site's chrome — its
//! name, its navigation, its logo, its footer, its icon — is not part of any
//! page, so it is read once, from the home page's raw HTML, and written once.
//! This half covers the chrome; [`facts`] reads it (pure, no network),
//! [`footer`] turns the footer into markdown, [`write`] puts the facts where
//! moss looks for them.
//!
//! Convention, from every migration importer: write only when the file or key
//! is absent. A re-run, or a folder the author already shaped, is never
//! overwritten.
//!
//! The family has a module of its own, not a stage inside the page pipeline:
//! none of it touches a page's body, and each fact is decided from the whole
//! home page rather than from one extracted article.

mod facts;
mod footer;
mod write;

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use specta::Type;

use super::crawl_state::HostPacer;
use super::scope::UrlScope;

pub(crate) use facts::norm_url;

/// What the import did about one piece of chrome.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum ChromePart {
    /// The home page had none, or it could not be fetched.
    #[default]
    NotFound,
    /// Written by this run.
    Written,
    /// Found, but the folder already had its own: left as it was.
    LeftUnchanged,
}

/// What a recursive import wrote for the site's chrome. All defaults for a run
/// that did not look (a single-page import).
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct ChromeSummary {
    /// True when the crawl read a home page for chrome at all.
    pub ran: bool,
    /// Pages given `nav: true`.
    pub nav_items: usize,
    pub nav: ChromePart,
    pub logo: ChromePart,
    pub footer: ChromePart,
    pub favicon: ChromePart,
}

impl ChromeSummary {
    /// The one summary line, `None` when no chrome was looked for.
    pub fn line(&self) -> Option<String> {
        if !self.ran {
            return None;
        }
        let part = |name: &str, p: ChromePart| match p {
            ChromePart::Written => Some(name.to_string()),
            ChromePart::LeftUnchanged => Some(format!("{name} left unchanged")),
            ChromePart::NotFound => None,
        };
        let nav = match self.nav {
            ChromePart::Written => Some(format!("nav {} item(s)", self.nav_items)),
            other => part("nav", other),
        };
        let parts: Vec<String> = [
            nav,
            part("logo", self.logo),
            part("footer", self.footer),
            part("favicon", self.favicon),
        ]
        .into_iter()
        .flatten()
        .collect();
        Some(if parts.is_empty() {
            "site chrome: none found".to_string()
        } else {
            format!("site chrome: {}", parts.join(", "))
        })
    }
}

/// One home page, as the crawl saw it.
pub(crate) struct HomePage {
    pub url: String,
    pub html: String,
    /// True when `url` is the site's own start page, whose note carries the
    /// site's name; a fallback first page contributes chrome but no home note.
    pub is_start: bool,
}

/// Everything one chrome pass needs from the finished crawl.
pub(crate) struct Capture<'a> {
    pub out_dir: &'a Path,
    pub scope: &'a UrlScope,
    pub user_agent: &'a str,
    pub home: &'a HomePage,
    /// Normalised URL ([`norm_url`]) → the note written for it, relative to
    /// `out_dir`.
    pub written: &'a HashMap<String, String>,
}

/// Read the home page's chrome and write it into the crawl's output folder.
pub(crate) async fn capture_site_chrome(
    capture: &Capture<'_>,
    pacer: &mut HostPacer,
) -> Result<ChromeSummary, String> {
    let facts = facts::read(&capture.home.html, &capture.home.url);
    write::apply(capture, &facts, pacer).await
}

#[cfg(test)]
#[path = "design/crawl_tests.rs"]
mod crawl_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_names_what_was_written_and_what_was_left() {
        let summary = ChromeSummary {
            ran: true,
            nav_items: 3,
            nav: ChromePart::Written,
            logo: ChromePart::Written,
            footer: ChromePart::LeftUnchanged,
            favicon: ChromePart::Written,
        };
        assert_eq!(
            summary.line().unwrap(),
            "site chrome: nav 3 item(s), logo, footer left unchanged, favicon"
        );
    }

    #[test]
    fn the_line_says_so_when_nothing_was_found_and_is_absent_when_nothing_ran() {
        let found_nothing = ChromeSummary { ran: true, ..Default::default() };
        assert_eq!(found_nothing.line().unwrap(), "site chrome: none found");
        assert_eq!(ChromeSummary::default().line(), None);
    }
}
