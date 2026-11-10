//! The `[site]` keys the build reads, one entry each, and the check that names
//! the keys it does not read. `moss describe` prints this table too, and the
//! drift test in `cli::describe` fails when a key is read in code and missing here.

use moss_core::contract::describe::SiteSettingInfo;

use crate::infra::edit_distance::levenshtein;

/// Read but not listed: `heading_anchors = false` still turns the marks off,
/// and `domain` is read as `url` in a folder written before `url` existed.
pub(crate) const UNLISTED_SITE_KEYS: [&str; 2] = ["domain", "heading_anchors"];

/// `[site]` table in `.moss/config.toml` — one entry per key the build reads
/// through `site_bool` / `site_str`. The drift test in `cli::describe` fails when
/// a key is read in code and missing here.
pub(crate) fn site_settings() -> Vec<SiteSettingInfo> {
    vec![
        SiteSettingInfo {
            name: "ai_answers",
            r#type: "bool",
            default: None,
            description: "Whether AI assistants may read pages to answer a person's question. false asks them not to in robots.txt and stops publishing llms.txt. Unset follows `ai_policy`.",
        },
        SiteSettingInfo {
            name: "ai_policy",
            r#type: "string",
            default: Some("standard"),
            description: "A name for a state of the three switches: \"standard\" (search engines and AI answers allowed, AI training asked not to), \"unrestricted\" (all allowed) or \"restricted\" (all asked not to). A site that sets none of `search_engines`, `ai_answers` and `ai_training` keeps the robots.txt this name has always written; \"restricted\" also stops llms.txt.",
        },
        SiteSettingInfo {
            name: "ai_training",
            r#type: "bool",
            default: None,
            description: "Whether AI companies may train models on the site. false asks the known training crawlers not to in robots.txt, adds `noai, noimageai` to every page and reserves text-and-data mining in /.well-known/tdmrep.json. Unset follows `ai_policy`.",
        },
        SiteSettingInfo {
            name: "archival",
            r#type: "bool",
            default: Some("false"),
            description: "Publish a dated, citable copy: every page shows its edition line and a citation, and the source files travel with the site.",
        },
        SiteSettingInfo {
            name: "atproto_did",
            r#type: "string",
            default: None,
            description: "The account identifier (`did:plc:…` or `did:web:…`) that lets the site's domain serve as a Bluesky handle. The build writes it to `.well-known/atproto-did`.",
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
            name: "color_scheme",
            r#type: "string",
            default: Some("auto"),
            description: "Colour scheme: \"auto\" follows the reader's saved choice or OS preference; \"light\" and \"dark\" fix the palette and omit the theme toggle, keeping search and language controls.",
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
            name: "header_scroll",
            r#type: "string",
            default: Some("static"),
            description: "Whether the header stays at the top while the reader scrolls: \"static\" scrolls away with the page; \"sticky\" stays; \"hide\" stays, slides out of view once the reader scrolls down past it and slides back on any scroll up. Without JavaScript or under reduced motion \"hide\" is \"sticky\". Ignored when `header` is \"none\".",
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
            name: "selection_tools",
            r#type: "bool",
            default: Some("false"),
            description: "When a reader selects text, offer Share (a picture of the passage with the page's QR code) and Copy (the passage with its source). false ships neither, and no QR codes.",
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
            name: "me",
            r#type: "string[]",
            default: None,
            description: "Profile addresses on other sites, e.g. `[\"https://social.example/@ana\"]`. Each is written as `<link rel=\"me\">` in the head of every page but media and notebook viewer pages, which is how Mastodon marks the site as verified on that profile. http and https only.",
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
            name: "search_engines",
            r#type: "bool",
            default: None,
            description: "Whether search engines may list the site. false adds `noindex` to every page (search engines stay free to read pages, so they see it), asks AI search crawlers not to in robots.txt and drops its Sitemap line. Unset follows `ai_policy`.",
        },
        SiteSettingInfo {
            name: "typesetting",
            r#type: "string",
            default: None,
            description: "\"vertical\" sets the text in vertical columns, for CJK sites. Unset is horizontal.",
        },
        SiteSettingInfo {
            name: "url",
            r#type: "string",
            default: None,
            description: "The address the site is published at, path included when it is served under one: \"https://example.org/\" or \"https://example.edu/~alice/\". Canonical links, feeds and the sitemap start with it, and every link the site makes is put under its path. Replaces `domain`, which is still read when `url` is absent.",
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
