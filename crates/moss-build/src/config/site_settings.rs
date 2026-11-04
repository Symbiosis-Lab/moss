//! The `[site]` keys the build reads, one entry each, and the check that names
//! the keys it does not read. `moss describe` prints this table too, and the
//! drift test in `cli::describe` fails when a key is read in code and missing here.

use moss_core::contract::describe::SiteSettingInfo;

use crate::infra::edit_distance::levenshtein;

/// `[site]` table in `.moss/config.toml` — one entry per key the build reads
/// through `site_bool` / `site_str`. The drift test in `cli::describe` fails when
/// a key is read in code and missing here.
pub(crate) fn site_settings() -> Vec<SiteSettingInfo> {
    vec![
        SiteSettingInfo {
            name: "ai_policy",
            r#type: "string",
            default: Some("standard"),
            description: "AI crawler policy written to robots.txt: \"standard\" (search and AI input allowed, AI training refused), \"unrestricted\", or \"restricted\" (all refused).",
        },
        SiteSettingInfo {
            name: "archival",
            r#type: "bool",
            default: Some("false"),
            description: "Publish a dated, citable copy: every page shows its edition line and a citation, and the source files travel with the site.",
        },
        SiteSettingInfo {
            name: "author",
            r#type: "string",
            default: None,
            description: "The edition's maker, named in a page's citation when the page has no author of its own.",
        },
        SiteSettingInfo {
            name: "colophon",
            r#type: "bool",
            default: Some("true"),
            description: "Show moss's one-line colophon as the footer's last line; false leaves it out of every page.",
        },
        SiteSettingInfo {
            name: "comments",
            r#type: "bool",
            default: None,
            description: "false hides comments on every page; true shows them on pages that do not set `comments:` themselves. Unset follows each page's plugin default.",
        },
        SiteSettingInfo {
            name: "content_width",
            r#type: "string",
            default: None,
            description: "Width preset of the reading column, \"wide\", \"full\", or \"screen\" (the full column, with every top-level grid and band as wide as the window). A page's own `content_width:` overrides it.",
        },
        SiteSettingInfo {
            name: "credit",
            r#type: "string",
            default: None,
            description: "The site's own one-line credit, inline Markdown, shown before moss's mark in the colophon: `Designed by [Example Studio](https://example.org) · (mark)`. Not shown when `colophon` is false.",
        },
        SiteSettingInfo {
            name: "domain",
            r#type: "string",
            default: None,
            description: "Custom domain the site is published under. `moss domain` reads and checks it.",
        },
        SiteSettingInfo {
            name: "floating_nav",
            r#type: "bool",
            default: Some("false"),
            description: "Show the contents ruler in the left margin on wide screens, on every page, not only pages that set `contents: open`.",
        },
        SiteSettingInfo {
            name: "hard_line_breaks",
            r#type: "bool",
            default: Some("true"),
            description: "A single line break in Markdown renders as a line break, as in Obsidian. false restores CommonMark's soft wrap.",
        },
        SiteSettingInfo {
            name: "header",
            r#type: "string",
            default: Some("brand"),
            description: "Header layout: \"brand\" shows the site name first; \"nav\" shows the link list first, opening with a Home link.",
        },
        SiteSettingInfo {
            name: "heading_anchors",
            r#type: "bool",
            default: Some("true"),
            description: "Give each heading a `#` link to itself. false removes the links.",
        },
        SiteSettingInfo {
            name: "implicit_figure",
            r#type: "bool",
            default: Some("true"),
            description: "A paragraph holding only an image becomes a figure, as in Pandoc. false keeps it a plain paragraph.",
        },
        SiteSettingInfo {
            name: "lang",
            r#type: "string",
            default: None,
            description: "Language of the default edition, published at `/`. Unset, it is detected from the pages.",
        },
        SiteSettingInfo {
            name: "license",
            r#type: "string",
            default: None,
            description: "The licence of the edition, stated on every page of an archival copy and in its source README.",
        },
        SiteSettingInfo {
            name: "link_preview",
            r#type: "bool",
            default: Some("true"),
            description: "Show a preview popup when a reader hovers a link. false ships no preview script.",
        },
        SiteSettingInfo {
            name: "locator",
            r#type: "string",
            default: Some("none"),
            description: "Small locator map after each place line: \"align-right\", or \"none\" to leave it off.",
        },
        SiteSettingInfo {
            name: "math",
            r#type: "bool",
            default: Some("true"),
            description: "Typeset math in pages. false turns math rendering off.",
        },
        SiteSettingInfo {
            name: "rss_footer",
            r#type: "bool",
            default: Some("false"),
            description: "Show a link to the RSS feed in the footer, when the site has a feed. Unset is off.",
        },
        SiteSettingInfo {
            name: "search",
            r#type: "bool",
            default: Some("false"),
            description: "Build the client-side search index and show the search button.",
        },
        SiteSettingInfo {
            name: "typesetting",
            r#type: "string",
            default: None,
            description: "\"vertical\" sets the text in vertical columns, for CJK sites. Unset is horizontal.",
        },
    ]
}

/// The known key `key` most likely meant: the closest one within a limit that
/// grows with the key's length, so a short key cannot match a different short key.
pub(crate) fn nearest_known_key<'a>(key: &str, known: &[&'a str]) -> Option<&'a str> {
    let limit = (key.len() / 3).max(1);
    known
        .iter()
        .map(|k| (levenshtein(key, k), *k))
        .min()
        .filter(|(d, _)| *d <= limit)
        .map(|(_, k)| k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_within_the_limit_names_its_known_key() {
        // Six bytes allow two edits; "langxx" is two from "lang".
        assert_eq!(nearest_known_key("langxx", &["lang"]), Some("lang"));
    }

    #[test]
    fn a_key_one_edit_past_the_limit_names_nothing() {
        // Seven bytes allow two edits; "langxxx" is three from "lang".
        assert_eq!(nearest_known_key("langxxx", &["lang"]), None);
    }

    #[test]
    fn a_short_key_needs_an_exact_or_one_edit_match() {
        assert_eq!(nearest_known_key("map", &["math", "lang"]), None);
        assert_eq!(nearest_known_key("mat", &["math", "lang"]), Some("math"));
    }
}
