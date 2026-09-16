//! Unit tests for the `--css` scanner. Extracted per the inline-test convention.

use super::*;

#[test]
fn mentions_requires_a_token_boundary() {
    assert!(mentions(".moss-card", ".moss-card"));
    assert!(mentions(".moss-card:hover", ".moss-card"));
    assert!(mentions(".moss-card > img", ".moss-card"));
    assert!(mentions("a, .moss-card, b", ".moss-card"));
    assert!(mentions(".moss-card[data-x]", ".moss-card"));
    // The whole point: a prefix query must not drag in the 60-rule family.
    assert!(!mentions(".moss-card-cover", ".moss-card"));
    assert!(!mentions(".moss-card-grid-title", ".moss-card"));
}

#[test]
fn scan_reports_the_enclosing_media_query() {
    let css = "\
.a { color: red; }
@media (max-width: 32rem) {
  .a { color: blue; }
}
";
    let mut out = Vec::new();
    scan(css, ".a", &mut out);
    assert_eq!(out.len(), 2, "both rules match");
    assert!(out[0].conditions.is_empty());
    assert_eq!(out[1].conditions, vec!["@media (max-width: 32rem)"]);
}

/// A rule copied out of a media query and pasted at top level applies at every
/// width. Reporting the condition is what stops that.
#[test]
fn rendered_output_wraps_matches_in_their_conditions() {
    let out = rules_matching(".moss-hero");
    assert!(out.contains("@media"), "hero has responsive rules; got:\n{out}");
}

#[test]
fn comment_above_a_rule_is_attached_to_it() {
    let css = "\
/* why this cap exists */
.a { max-height: 10px; }
";
    let mut out = Vec::new();
    scan(css, ".a", &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].comment, Some("/* why this cap exists */"));
}

/// A comment trailing a declaration belongs to the line it is on, not to the
/// next rule — attaching it would put unrelated prose above every rule.
#[test]
fn a_trailing_comment_is_not_attached_to_the_next_rule() {
    let css = "\
.a { color: red; } /* trailing */
.b { color: blue; }
";
    let mut out = Vec::new();
    scan(css, ".b", &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].comment, None);
}

#[test]
fn hero_query_surfaces_the_escape_hatch_and_its_default() {
    let out = rules_matching(".moss-hero");
    assert!(out.contains("--moss-hero-max-height"), "must surface the hook");
    assert!(out.contains("min(80vb, 800px)"), "must surface the wrapper default");
    assert!(out.contains("70vb"), "must surface the media default");
    assert!(!out.contains(".moss-search"), "must not dump unrelated components");
}

/// The size ceiling is the feature. Dumping the sheet is what this replaces.
#[test]
fn a_component_slice_stays_readable() {
    let out = rules_matching(".moss-hero");
    assert!(!out.is_empty());
    assert!(
        out.len() < 20_000,
        "a component slice must stay readable, got {} bytes",
        out.len()
    );
}

/// `.moss-subscribe` lives in email.css, not site.css. Scanning only site.css
/// would report "no rules match" for a component that is very much styled.
#[test]
fn the_query_reaches_every_shipped_stylesheet() {
    let out = rules_matching(".moss-subscribe");
    assert!(out.contains("email.css"), "must reach email.css; got:\n{out}");
}

/// Every gated partial must be reachable, not just the fixed sheets.
///
/// The partials were carved out of `site.css` on 2026-08-04. The carve made
/// `moss describe --css ".callout"` and `--css "data-typesetting"` return
/// **empty** — ~14 KB of rules invisible to the one tool that exists so an
/// agent does not have to read the sheet. This test names each registered
/// partial rather than one hand-picked token, so a future partial cannot be
/// added without being queryable.
#[test]
fn the_query_reaches_every_registered_partial() {
    for partial in crate::build::emit::stylesheet::CSS_PARTIALS {
        // The first selector token in the file is, by the ownership rule the
        // partial had to pass, a token core does not use — so a hit can only
        // have come from the partial itself.
        let needle = first_selector_token(partial.source);
        let out = rules_matching(&needle);
        assert!(
            out.contains(&format!("site/{}.css", partial.name)),
            "query for {needle:?} must reach partial {}; got:\n{out}",
            partial.name
        );
    }
}

/// First `.class` or `[attr` token appearing in a stylesheet's selectors.
fn first_selector_token(css: &str) -> String {
    for line in css.lines() {
        let line = line.trim();
        if !line.ends_with('{') || line.starts_with('@') {
            continue;
        }
        for (i, ch) in line.char_indices() {
            if ch != '.' && ch != '[' {
                continue;
            }
            let rest = &line[i + 1..];
            let end = rest
                .find(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
                .unwrap_or(rest.len());
            if end > 0 {
                return format!("{ch}{}", &rest[..end]);
            }
        }
    }
    panic!("no selector token found in partial");
}

/// Every sheet the query can print must have a shipping note. The fallback
/// string is a placeholder, never an answer — a theme author reading
/// "shipping conditions unknown" learns nothing about whether they must fight
/// the rule.
#[test]
fn every_sheet_declares_when_it_ships() {
    for (name, _) in sheets() {
        assert_ne!(
            shipping_note(&name),
            "shipping conditions unknown — add a row to shipping_note()",
            "stylesheet {name:?} has no shipping note"
        );
    }
}

#[test]
fn an_unknown_selector_returns_empty_rather_than_erroring() {
    assert!(rules_matching(".nope-not-real").trim().is_empty());
}

/// A custom property is a legitimate query: "what reads this?" is exactly the
/// question an agent asks before setting one.
#[test]
fn a_custom_property_can_be_queried() {
    let out = rules_matching("--moss-grid-image-ratio");
    assert!(out.contains("aspect-ratio"), "got:\n{out}");
}
